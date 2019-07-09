
pub extern crate csv;
extern crate datafrog;
pub extern crate polonius_engine;
pub extern crate regex;
pub extern crate rustc;
pub extern crate rustc_data_structures;
pub extern crate serde;
pub extern crate syntax_pos;

use super::facts;
use super::regions;

use std::{cell};
use std::env;
use std::collections::{HashMap,BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Write, BufWriter};
use std::path::PathBuf;
use self::polonius_engine::{Algorithm, Output};
use rustc::hir::{self, intravisit};
use rustc::mir;
use rustc::ty;
use rustc::ty::TyCtxt;
use self::rustc_data_structures::fx::FxHashMap;
use self::datafrog::Relation;
use self::regex::Regex;
use self::facts::{PointIndex, Loan, Region};

pub fn dump_borrowck_info<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>) {
    trace!("[dump_borrowck_info] enter");

    //assert!(tcx.use_mir_borrowck(), "NLL is not enabled."); // maybe use borrowck_mode(&self) -> BorrowckMode instead?

    let mut printer = InfoPrinter {
        tcx: tcx,
    };
    intravisit::walk_crate(&mut printer, tcx.hir().krate());

    trace!("[dump_borrowck_info] exit");
}

struct InfoPrinter<'a, 'tcx: 'a> {
    pub tcx: TyCtxt<'a, 'tcx, 'tcx>,
}

impl<'a, 'tcx> intravisit::Visitor<'tcx> for InfoPrinter<'a, 'tcx> {
    fn nested_visit_map<'this>(&'this mut self) -> intravisit::NestedVisitorMap<'this, 'tcx> {
        let map = &self.tcx.hir();
        intravisit::NestedVisitorMap::All(map)
    }

    fn visit_fn(&mut self, fk: intravisit::FnKind<'tcx>, fd: &'tcx hir::FnDecl,
                b: hir::BodyId, s: syntax_pos::Span, hir_id: hir::HirId) {
        // call walk_fn with all received parameters. This is what the default would do, and
        // required to also process all content of the function (and thereby eventually also handle
        // inner (nested) functions.
        intravisit::walk_fn(self, fk, fd, b, s, hir_id);

        let name = match fk {
            intravisit::FnKind::ItemFn(name, ..) => name,
            intravisit::FnKind::Method(name, ..) => name,
            _ => return, // skip anything else (right now, this seems to include only closures), since we do not know to handle it. (Dealing with closures is deferred for now.)
        };

        trace!("[visit_fn] enter name={:?}", name);

        match env::var_os("PRUSTI_DUMP_PROC").and_then(|value| value.into_string().ok()) {
            Some(value) => {
                if name.as_str() != value {
                    return;
                }
            },
            _ => {},
        };

        let def_id = self.tcx.hir().local_def_id_from_hir_id(hir_id);
        // This trial was disabled (and the old version that comes later in the code was re-enabled) too keep the old version working in a fully consistent way for now.
        //let mir = self.tcx.mir_built(def_id).borrow().clone();

        self.tcx.mir_borrowck(def_id);

        // Read Polonius facts.
        let def_path = self.tcx.hir().def_path(def_id);
        let dir_path = PathBuf::from("nll-facts").join(def_path.to_filename_friendly_no_crate());
        debug!("Reading facts from: {:?}", dir_path);
        let mut facts_loader = facts::FactLoader::new();
        facts_loader.load_all_facts(&dir_path);

        // Read relations between region IDs and local variables.
        let renumber_path = PathBuf::from(format!(
            "log/mir/rustc.{}.-------.renumber.0.mir",
            def_path.to_filename_friendly_no_crate()));
        debug!("Renumber path: {:?}", renumber_path);
		let variable_regions = regions::load_variable_regions(&renumber_path).unwrap();

        let all_facts = facts_loader.facts;
        let output = Output::compute(&all_facts, Algorithm::Naive, true);

        println!("errors: {:?}", output.errors);

        // This was disabled before (isteand, an "older" version of mir was read from tcx before), but was re-enabled now to keep the old code work in a consistent way for now.
        let mir = self.tcx.mir_validated(def_id).borrow();

        let graph_path = PathBuf::from("nll-facts")
            .join(def_path.to_filename_friendly_no_crate())
            .join("graph.dot");
        let graph_file = File::create(graph_path).expect("Unable to create file");
        let graph = BufWriter::new(graph_file);

        let interner = facts_loader.interner;

        let region_to_local_map = regions::load_region_to_local_map(&renumber_path).expect("Error reading mir dump file!");

        debug!("region_to_local_map: {:?}", region_to_local_map);


        let mut mir_info_printer = MirInfoPrinter {
            tcx: self.tcx,
            mir: mir,
            borrowck_in_facts: all_facts,
            borrowck_out_facts: output,
            interner: interner,
			variable_regions: variable_regions,
            region_to_local_map,
            def_path: def_path,
        };
        mir_info_printer.print_info();

        debug!("[visit_fn] ----------> Done with handling function {:?} <----------", name);

        trace!("[visit_fn] exit");
    }
}

struct ExplOutput{
    pub expl_outlives: FxHashMap<PointIndex, BTreeMap<Region, BTreeSet<Region>>>,
    pub expl_subset: FxHashMap<PointIndex, BTreeMap<Region, BTreeSet<Region>>>,
    pub expl_requires: FxHashMap<PointIndex, BTreeMap<Region, BTreeSet<Loan>>>,
    pub expl_borrow_live_at: FxHashMap<PointIndex, Vec<Loan>>,
    pub unordered_expl_outlives: Vec<(Region, Region, PointIndex)>,

}

impl ExplOutput{

    fn new() -> Self{
        ExplOutput{
            expl_outlives: FxHashMap::default(),
            expl_subset: FxHashMap::default(),
            expl_requires: FxHashMap::default(),
            expl_borrow_live_at: FxHashMap::default(),
            unordered_expl_outlives: Vec::default(),
        }
    }

}

fn compute_error_expl(all_facts: &facts::AllInputFacts, output: &facts::AllOutputFacts, error_fact: (PointIndex, Vec<Loan>)) -> ExplOutput {

    let mut result = ExplOutput::new();

    let expl_outlives = {

        let mut iteration = datafrog::Iteration::new();
        // .. some variables, ..
        let subset = iteration.variable::<(Region, Region, PointIndex)>("subset");
        let new_subset = iteration.variable::<((Region, Region, PointIndex),())>("new_subset");
        let outlives = iteration.variable::<(Region, Region, PointIndex)>("outlives");
        let new_outlives = iteration.variable::<((Region, Region, PointIndex),())>("new_outlives");
        let requires = iteration.variable::<(Region, Loan, PointIndex)>("requires");
        let new_requires = iteration.variable::<((Region, Loan, PointIndex),())>("new_requires");
        let borrow_live_at = iteration.variable::<(Loan, PointIndex)>("borrow_live_at");
        let new_borrow_live_at = iteration.variable::<((Loan, PointIndex), ())>("new_borrow_live_at");

        // `invalidates` facts, stored ready for joins
        let invalidates = iteration.variable::<((Loan, PointIndex), ())>("invalidates");

        // different indices for `subset`.
        let subset_r1p = iteration.variable_indistinct("subset_r1p");
        let subset_r2p = iteration.variable_indistinct("subset_r2p");
        let subset_r1r2 = iteration.variable_indistinct("subset_r1r2");
        let subset_p = iteration.variable_indistinct("subset_p");

        let expl_subset_r1p = iteration.variable_indistinct("expl_subset_r1p");
        let expl_subset_r1r2 = iteration.variable_indistinct("expl_subset_r1r2");
        //let expl_subset_p = iteration.variable_indistinct("expl_subset_p");


        // different indexes for `requires`.
        let requires_rp = iteration.variable_indistinct("requires_rp");
        let requires_bp = iteration.variable_indistinct("requires_bp");
        let requires_rb = iteration.variable_indistinct("requires_rb");

        //let expl_requires_rp = iteration.variable_indistinct("expl_requires_rp");
        let expl_requires_bp = iteration.variable_indistinct("expl_requires_bp");
        let expl_requires_rb = iteration.variable_indistinct("expl_requires_rb");

        // temporaries as we perform a multi-way join.
        let subset_1 = iteration.variable_indistinct("subset_1");
        let subset_2 = iteration.variable_indistinct("subset_2");
        let subset_3 = iteration.variable_indistinct("subset_3");
        let subset_4 = iteration.variable_indistinct("subset_4");
        let subset_5 = iteration.variable_indistinct("subset_5");
        let subset_6 = iteration.variable_indistinct("subset_6");
        let requires_1 = iteration.variable_indistinct("requires_1");
        let requires_2 = iteration.variable_indistinct("requires_2");
        let requires_3 = iteration.variable_indistinct("requires_3");
        let requires_4 = iteration.variable_indistinct("requires_4");
        let requires_5 = iteration.variable_indistinct("requires_5");

        let killed = all_facts.killed.clone().into();
        let region_live_at = iteration.variable::<((Region, PointIndex), ())>("region_live_at");
        let cfg_edge_p = iteration.variable::<(PointIndex, PointIndex)>("cfg_edge_p");
        let new_cfg_edge = iteration.variable::<((PointIndex, PointIndex),())>("new_cfg_edge");

        let init_expl_error = iteration.variable::<(PointIndex,Loan)>("init_expl_error");
        let expl_error = iteration.variable::<(Loan,PointIndex)>("expl_error");
        let new_expl_error = iteration.variable::<((Loan,PointIndex),())>("new_expl_error");
        let expl_subset = iteration.variable::<(Region, Region, PointIndex)>("expl_subset");
        let new_expl_subset = iteration.variable::<((Region, Region, PointIndex),())>("new_expl_subset");
        let expl_requires = iteration.variable::<(Region, Loan, PointIndex)>("expl_requires");
        let expl_borrow_live_at = iteration.variable::<(Loan, PointIndex)>("expl_borrow_live_at");

        let expl_borrow_live_at_1 = iteration.variable_indistinct("expl_borrow_live_at_1");
        let expl_borrow_live_at_p = iteration.variable_indistinct("expl_borrow_live_at_p");
        let region_live_at_p = iteration.variable_indistinct("region_live_at_p");

        let expl_outlives = iteration.variable("expl_outlives");


        let expl_error_vec = vec![error_fact];

        expl_error.insert(Relation::from_vec(expl_error_vec.iter().flat_map(
            |(point, loans)| loans.iter().map(move |&loan|  (loan, *point))
        ).collect()));
        // Or should we instead use collect_vec to get the right "thing"/type?
        //   --> probably not, trying to do so leads to a VERY drastic error stating that collect_vec is not found!

        outlives.insert(all_facts.outlives.clone().into());
        requires.insert(all_facts.borrow_region.clone().into());
        region_live_at.insert(Relation::from_vec(
            all_facts.region_live_at.iter().map(|&(r, p)| ((r, p), ())).collect(),
        ));
        invalidates.insert(Relation::from_vec(
            all_facts.invalidates.iter().map(|&(p, b)| ((b, p), ())).collect(),
        ));
        cfg_edge_p.insert(all_facts.cfg_edge.clone().into());

        subset.insert(Relation::from_vec(
            output.subset.iter().flat_map(
                |(&point, region_map)|
                    region_map.iter().flat_map(
                        move |(&region, regions)|
                            regions.iter().map(move |&region2| (region, region2, point))
                    )
            ).collect()
        ));

        borrow_live_at.insert(Relation::from_vec(
            output.borrow_live_at.iter().flat_map(
                |(&point, loans)|
                    loans.iter().map(move |&loan| (loan, point))

            ).collect()
        ));

        requires.insert(Relation::from_vec(
            output.restricts.iter().flat_map(
                |(&point, region_map)|
                    region_map.iter().flat_map(
                        move |(&region, loans)|
                            loans.iter().map(move |&loan| (region, loan, point))
                    )
            ).collect()
        ));

        while iteration.changed() {

            /*subset
                .recent
                .borrow_mut()
                .elements
                .retain(|&(r1, r2, _)| r1 != r2);*/

            // remap fields to re-index by keys.
            subset_r1p.from_map(&subset, |&(r1, r2, p)| ((r1, p), r2));
            subset_r2p.from_map(&subset, |&(r1, r2, p)| ((r2, p), r1));
            subset_r1r2.from_map(&subset, |&(r1, r2, p)| ((r1, r2), p));
            subset_p.from_map(&subset, |&(r1, r2, p)| (p, (r1, r2)));

            requires_rp.from_map(&requires, |&(r, b, p)| ((r, p), b));
            requires_bp.from_map(&requires, |&(r, b, p)| ((b, p), r));
            requires_rb.from_map(&requires, |&(r, b, p)| ((r, b), p));

            new_borrow_live_at.from_map(&borrow_live_at, |&(b, p)| ((b, p), ()));
            new_requires.from_map(&requires, |&(r, b, p)| ((r, b, p), ()));
            new_outlives.from_map(&outlives, |&(r1, r2, p)| ((r1, r2, p), ()));
            new_cfg_edge.from_map(&cfg_edge_p, |&(p1, p2)| ((p1, p2), ()));
            region_live_at_p.from_map(&region_live_at, |&((r, p),())| (p, r));

            //expl_error.from_map(&init_expl_error, |&(p, b)| (b, p));
            new_expl_error.from_map(&expl_error, |&(b, p)| ((b, p), ()));


            //inverted rules
            expl_borrow_live_at_1.from_join(&new_expl_error, &invalidates, |&(b,p),&(),&()| ((b,p),()));
            expl_borrow_live_at.from_join(&expl_borrow_live_at_1, &new_borrow_live_at, |&(b,p),&(), &()| {debug!("1{:?}",(b,p));(b, p)});

            expl_borrow_live_at_p.from_map(&expl_borrow_live_at, |&(b,p)| (p, b));

            requires_1.from_join(&expl_borrow_live_at_p, &region_live_at_p, |&p, &b, &r| ((r, b, p),()));
            expl_requires.from_join(&requires_1, &new_requires, |&(r, b, p), &(), &()| {debug!("2{:?}",(r,b,p));(r,b,p)});

            expl_requires_bp.from_map(&expl_requires, |&(r, b, p)| ((b, p), r));
            new_subset.from_map(&subset, |&(r1, r2, p)| ((r1, r2, p), ()));

            requires_2.from_join(&expl_requires_bp, &requires_bp, |&(b, p), &r2, &r1| ((r1,r2,p),b));
            expl_requires.from_join(&requires_2, &new_subset, |&(r1, r2, p), &b,&()| {debug!("3{:?}",(r1,b,p));(r1,b,p)});


            expl_requires_rb.from_map(&expl_requires, |&(r, b, p)| ((r, b), p));

            requires_3.from_join(&expl_requires_rb, &requires_rb, |&(r, b), &p1, &p2| {debug!("4.1{:?}",((b,p2),(r,p1)));((b,p2),(r,p1))});
            requires_4.from_antijoin(&requires_3, &killed, |&(b,p2),&(r,p1)| {debug!("4.2{:?}",((p2,p1),(b,r)));((p2,p1),(b,r))});
            requires_5.from_join(&requires_4, &new_cfg_edge, |&(p2,p1),&(b,r),&()| {debug!("4.3{:?}",((r,p1),(b,p2)));((r,p1),(b,p2))});
            expl_requires.from_join(&requires_5,&region_live_at,|&(r,p1),&(b,p2),&()| {debug!("4{:?}",(r,b,p2));(r,b,p2)});

            expl_requires_bp.from_map(&expl_requires, |&(r, b, p)| {debug!("5.1{:?}",((b, p), r));((b, p), r)});

            subset_1.from_join(&expl_requires_bp, &requires_bp, |&(b, p), &r2, &r1| {debug!("5.2{:?}",((r1,r2,p),b));((r1,r2,p),b)});
            expl_subset.from_join(&subset_1, &new_subset, |&(r1, r2, p), &b,&()| {debug!("5{:?}",(r1,r2,p));(r1,r2,p)});



            expl_subset_r1p.from_map(&expl_subset, |&(r1, r2, p)| ((r1, p), r2));

            subset_2.from_join(&expl_subset_r1p, &subset_r1p, |&(r1, p), &r3, &r2| {debug!("6.1{:?}",(r2,r3,p));((r2,r3,p),())});
            expl_subset.from_join(&subset_2, &new_subset, |&(r2, r3, p), &(),&()| {debug!("6{:?}",(r2,r3,p));(r2,r3,p)});

            subset_3.from_join(&expl_subset_r1p, &subset_r1p, |&(r1, p), &r3, &r2| {debug!("7.1{:?}",((r2,r3,p),(r1)));((r2,r3,p),(r1))});
            expl_subset.from_join(&subset_3, &new_subset, |&(r2, r3, p), &r1,&()| {debug!("7{:?}",(r1,r2,p));(r1,r2,p)});

            expl_subset_r1r2.from_map(&expl_subset, |&(r1, r2, p)| ((r1, r2), p));

            subset_4.from_join(&expl_subset_r1r2, &subset_r1r2, |&(r1, r2), &p1, &p2| {debug!("8.1{:?}",((p2,p1),(r1,r2)));((p2,p1),(r1,r2))});
            subset_5.from_join(&subset_4, &new_cfg_edge, |&(p2,p1),&(r1,r2),&()| {debug!("8.2{:?}",((r1,p1),(r2,p2)));((r1,p1),(r2,p2))});
            subset_6.from_join(&subset_5, &region_live_at, |&(r1,p1), &(r2,p2), &()| {debug!("8.3{:?}",((r2,p1),(r1,p2)));((r2,p1),(r1,p2))});
            expl_subset.from_join(&subset_6, &region_live_at, |&(r2,p1), &(r1,p2), &()| {debug!("8{:?}",(r1,r2,p2));(r1, r2, p2)});

            new_expl_subset.from_map(&expl_subset, |&(r1,r2,p)| ((r1,r2,p),()));
            expl_outlives.from_join(&new_expl_subset, &new_outlives, |&(r1,r2,p), &(), &()| {debug!("9{:?}",(r1,r2,p));(r1,r2,p)});

        }

        let expl_subset = expl_subset.complete();
        for (r1, r2, location) in &expl_subset.elements {
            result
                .expl_subset
                .entry(*location)
                .or_insert(BTreeMap::new())
                .entry(*r1)
                .or_insert(BTreeSet::new())
                .insert(*r2);
        }

        let expl_requires = expl_requires.complete();
        for (region, borrow, location) in &expl_requires.elements {
            result
                .expl_requires
                .entry(*location)
                .or_insert(BTreeMap::new())
                .entry(*region)
                .or_insert(BTreeSet::new())
                .insert(*borrow);
        }


        let expl_borrow_live_at = expl_borrow_live_at.complete();
        for (borrow, location) in &expl_borrow_live_at.elements {
            result
                .expl_borrow_live_at
                .entry(*location)
                .or_insert(Vec::new())
                .push(*borrow);
        }

        expl_outlives.complete()

    };

    //println!("ex_outlives2: {:?}",expl_outlives.elements);
    for (r1, r2, location) in &expl_outlives.elements {
        result
            .expl_outlives
            .entry(*location)
            .or_insert(BTreeMap::new())
            .entry(*r1)
            .or_insert(BTreeSet::new())
            .insert(*r2);
    }

    result.unordered_expl_outlives = expl_outlives.elements;


    result

}

/// This struct holds the functions and data that is needed to find a path in an outlives graph
/// that shall be sufficient to describe and explain a given error (that was detected by the (naive)
/// Poloinus borrow checker) and should be helpful to understand the causes for the error.
/// After initializing all needed fields (best done by using the provided constructor), call
/// compute_error_path() to run the actual path computation and to get back the resulting path.
struct ErrorPathFinder<'epf> {
    all_facts: &'epf facts::AllInputFacts,
    output: &'epf facts::AllOutputFacts,
    error_fact: (PointIndex, Vec<Loan>),
    outlives: &'epf Vec<(Region, Region, PointIndex)>,
//    start_points_of_error_loan: Vec<PointIndex>,
    error_loan: Loan,
}

impl <'epf> ErrorPathFinder<'epf> {
    /// The constructor to create a new instance and inserting all needed information into it.
    /// When this constructor was executed (with sensible arguments), the struct instance is ready
    /// to run the path computation.
    fn new(all_facts: &'epf facts::AllInputFacts, output: &'epf facts::AllOutputFacts,
           error_fact: (PointIndex, Vec<Loan>), outlives: &'epf Vec<(Region, Region, PointIndex)>) -> Self {
        ErrorPathFinder {
            all_facts,
            output,
            error_fact,
            outlives,
//            start_points_of_error_loan: Vec::default(),
            error_loan: Loan::from(0),
        }
    }

    /// The method that does run the entire path computation, using the information that is provided
    /// by the fields of the struct instance it is called on. Best call this after initiation a
    /// struct instance with the provided constructor.
    /// Note that this method will change some of the fields of the struct, and it is not intended
    /// to run this method more then once on the same struct instance. (This might work, but it was
    /// never tested and no guarantees are provided.) I.e. it is not guaranteed that this method
    /// is idempotent with respect to the struct and it's result.
    /// On completion, the method will return the path as a vector of regions (facts::Region).
    /// This gives a simple, portable and unique representation of the found path. However, please
    /// note that the path is given in backwards direction. I.e. the first element of the vector
    /// it the last of the path in the outlives relation (graph).
    /// When no such path is found an empty vector (Vec::default()) is returned. In this case the
    /// this method will print also a warning to the log. (Since this did never happen while
    /// testing as of now.)
    /// Sometimes also the search for the starting region fails. In this case, also an empty vector
    /// (Vec::default()) is returned, but this does not cause a warning. (As this seems to happen
    /// under some circumstances, especially when multiple errors are found in the input program.)
    /// Especially, for some programs Polunius finds several points for an error. In this case one
    /// should try all as inupt for the path search, as sometimes not all do lead to a successful
    /// search. (For some no starting region is found.)
    fn compute_error_path(&mut self) -> Vec<Region> {
        trace!("[compute_error_path] enter");

        let regions_life_at_error: Vec<Region> = self.all_facts.region_live_at.iter().filter(|&(r, p)|
            *p == self.error_fact.0
        ).map(|&(r, p)| r).collect();

        debug!("regions_life_at_error: {:?}", regions_life_at_error);

        //NOTE It might be possible to simplify this, making the next step superfluous, as we already get a loan form the error in error_fact.

        let loans_invalidated_by_error: Vec<Loan> = self.all_facts.invalidates.iter().filter(|&(p, l)|
            *p == self.error_fact.0
        ).map(|&(p, l)| l).collect();

        debug!("loans_invalidated_by_error: {:?}", loans_invalidated_by_error);

        let mut requires = self.all_facts.borrow_region.clone();

        requires.extend(
            self.output.restricts.iter().flat_map(
                |(&point, region_map)|
                    region_map.iter().flat_map(
                        move |(&region, loans)|
                            loans.iter().map(move |&loan| (region, loan, point))
                    )
            )
        );

        debug!("requires, after adding elements from output.restricts : {:?}", requires);

        let error_region_loan_opt = requires.iter().filter(|&(r, l, p)|
            *p == self.error_fact.0 &&
                loans_invalidated_by_error.contains(l) &&
                regions_life_at_error.contains(r)
        ).map(|&(r, l, _)| (r, l)).next();
        let (error_region, error_loan_var) = match error_region_loan_opt {
            Some(error_descr) => error_descr,
            None => return Vec::default(),
        };
        self.error_loan = error_loan_var;

        debug!("error_point: {:?}", self.error_fact.0);
        debug!("error_region: {:?}", error_region);
        debug!("error_loan: {:?}", self.error_loan);
        debug!("all_facts.region_live_at: {:?}", self.all_facts.region_live_at);
        debug!("all_facts.cfg_edge: {:?}", self.all_facts.cfg_edge);
        debug!("all_facts.borrow_region: {:?}", self.all_facts.borrow_region);

        assert!(self.error_fact.1.contains(&self.error_loan));

//        let mut points_of_error_loan = self.points_of_loan(self.error_loan);
//        points_of_error_loan.sort_unstable();
//
//        debug!("points_of_error_loan: {:?}", points_of_error_loan);

//        let start_points_of_error_region = self.find_start_points(&points_of_error_region);
//
//        debug!("start_points_of_error_region: {:?}", start_points_of_error_region);

//        self.start_points_of_error_loan = self.find_start_points(&points_of_error_loan);
//
//        debug!("start_points_of_error_loan: {:?}", self.start_points_of_error_loan);

        debug!("Start computing path to error:");

        let mut path_to_error: Vec<Region> = Vec::new();
        let res = self.path_to_error_backwards(error_region,&mut path_to_error);

        debug!("path_to_error after done with iteration: {:?}", path_to_error);

        trace!("[compute_error_path] exit");

        if res {
            path_to_error
        } else {
            warn!("No path to explain the error was found for the start point {:?} and error loan \
                    {:?}!", self.error_fact.0, self.error_loan);
            Vec::default()
        }
    }

//    fn points_of_region(&self, region: Region) -> Vec<PointIndex> {
//        self.all_facts.region_live_at.iter().filter(|&(r, p)|
//            *r == region
//        ).map(|&(r, p)| p).collect()
//    }

//    /// Takes a loan, and returns all points where this loan is live. This information is retracted from
//    /// the output of a polonius borrow-check. (Must be available as struct fiels)
//    fn points_of_loan(&self, loan: Loan) -> Vec<PointIndex> {
//        self.output.borrow_live_at.iter().filter(|&(_, loans_of_point)|
//            loans_of_point.contains(&loan)
//        ).map(|(p, _)| *p).collect()
//    }

//    /// Find the points that are start/entry points into a set of points, based on a given cfg.
//    /// This function takes a set of points (given as a vector of points).
//    /// The cfg that will be used is given (as a vector of tuples of points that give the edges
//    /// as part of the all_facts that must have been retrieved from the compiler and be present as
//    /// field.
//    /// Then, it finds all points in the given set that are points where the program might start to
//    /// touch these points. (i.e. the control flow might enter the points set there)
//    /// This is, it finds all points that only have outgoing edges in the cfg when only considering
//    /// the part of the cfg that is fully covered by the given points, i.e. only edges that are
//    /// connecting pints in the set are considered.
//    fn find_start_points(&self, points: &Vec<PointIndex>) -> Vec<PointIndex> {
//        points.iter().filter(|&challenge|
//            ! self.all_facts.cfg_edge.iter().any(|&(p, q)|
//                q == *challenge &&
//                    points.contains(&p)
//            )
//        ).map(|&p| p).collect()
//    }

//    /// finds all regions in the outlives (available as field of the struct) that are directly
//    /// following the region given as start.
//    fn find_next_regions(&self, start: Region)
//                         -> Vec<Region> {
//        self.outlives.iter().filter(|&(r1, r2, _)|
//            *r1 == start
//        ).map(|&(_, r2, _)| r2).collect()
//    }

    /// finds all regions in the outlives (available as field of the struct) that are directly
    /// before the region given as start.
    fn find_prev_regions(&self, start: Region)
                         -> Vec<Region> {
        self.outlives.iter().filter(|&(_, r2, _)|
            *r2 == start
        ).map(|&(r1, _, _)| r1).collect()
    }

//    /// Returns true if there is a point P in points such that this point P is bigger then cmp
//    /// (where bigger means that it is later in the program flow then then previous one)
//    /// NOTE: For now, points are simply compared by their index
//    fn has_bigger_point(&self, points: &Vec<PointIndex>, cmp: PointIndex) -> bool {
//        points.iter().filter(|&p|
//            self.is_later_in_program_or_eq(*p, cmp)
//        ).count() > 0
//    }

//    /// check if goal is later in the cfg (given as part of all_facts, that is available as a filed)
//    /// then start, or at the same point
//    fn is_later_in_program_or_eq(&self, goal: PointIndex, start: PointIndex) -> bool {
//        goal == start ||
//            self.all_facts.cfg_edge.iter().filter(|&(p, q)|
//                *p == start && *q == goal
//            ).count() > 0 ||
//            self.all_facts.cfg_edge.iter().filter(|&(p, q)|
//                *p == start && self.is_later_in_program_or_eq(goal, *q)
//            ).count() > 0
//    }

//    /// Returns true if there is a point P in points such that this point P is smaller then cmp
//    /// (where smaller means that it is earlier in the program flow then then previous one)
//    /// NOTE: For now, points are simply compared by their index, this might be nonsense.
//    fn has_smaller_point(&self, points: &Vec<PointIndex>, cmp: PointIndex) -> bool {
//        // TODO does (presumably) simply compare points by their index, as usize. No idea if this makes any sense or is any good.
//        points.iter().filter(|&p| *p < cmp).count() > 0
//    }

    /// This function finds all loans that belong to a certain region as given by the
    /// all_facts.borrow_region input. (Is available in self)
    /// Note that this does not give all loans that might be "live" for this region, or relevant for
    /// this region. This would be given by the (computed) requires relation. Instead, this only
    /// includes the loans that were considered to belong to a region when they were provided as
    /// input fact (borrow_region) to the borrow checker.
    fn loan_of_reagion(&self, reg: Region) -> Vec<Loan> {
        self.all_facts.borrow_region.iter().filter(|&(r, _, _)|
            *r == reg
        ).map(|&(_, l, _)| l).collect()
    }

    /// This method does implement the (recursive) traversal of the graph described by the outlives
    /// relation (given as self.outlives), and thereby tries to find a path from the Region given as
    /// `start` to a region that includes the error loan/borrow.
    /// The search is (for now) implemented in a depth-first traversal, and the first path that
    /// leads to a region that fulfills the criterion is taken. This graph is then returned as
    /// the out-parameter cur_path.
    /// In addition, the method returns true if it did succeed in finding a path that fulfills the
    /// termination criterion. If non was fround, false is returned and the content of cur_path is
    /// not altered. (The behaviour regarding cur_path in the error case might change in the future,
    /// e.g. for the sake of performance.)
    fn path_to_error_backwards(&self, start: Region, mut cur_path: &mut Vec<Region>)
                               -> bool {
//        let points_of_cur_region: Vec<PointIndex> = self.points_of_region(start);
//        let start_points_of_cur_region = self.find_start_points(&points_of_cur_region);
        debug!("cur_region (start): {:?}", start);
//        debug!("points_of_cur_region: {:?}", points_of_cur_region);
//        debug!("start_points_of_cur_region: {:?}", start_points_of_cur_region);

        // add start to the path, as it will now become part of it.
        cur_path.push(start);

//        for sp in start_points_of_cur_region {
//            if self.start_points_of_error_loan.contains(&sp) {
//                // this region's start intersects with the start of the loan that causes the error,
//                // therefore we consider here to be the end of the relevant path, and therefore stop
//                // (end recursion) and return success (true)
//                return true
//            }
//        }

        let input_loans_of_cur_region =  self.loan_of_reagion(start);
        debug!("input_loans_of_cur_region: {:?}", input_loans_of_cur_region);

        if self.all_facts.borrow_region.iter().filter(|&(r, l, p)|
                    *r == start && *l == self.error_loan
                ).count() > 0 {
            // the start region does include the error loan. (May also be called error borrow.)
            // therefore, stop the recursion here, as we consider this to be gone far enough.
            // Also, this path is considered to lead to success, so return true
            debug!("Success path found by path_to_error_backwards, ending at region {:?}", start);
            return true
        }

        let mut prev_regions = self.find_prev_regions(start);
        prev_regions.dedup();
        debug!("prev_regions: {:?}", prev_regions);

        for pr in prev_regions {
            if cur_path.contains(&pr) {
                // this element already is part of the path, so there would be circle by adding it again, therefore stop here.
                continue
            } else {
                let mut pr_path = cur_path.clone();
                if self.path_to_error_backwards(pr, &mut pr_path) {
                    cur_path.clear();
                    cur_path.append(&mut pr_path);
                    return true
                } else {
                    continue
                }
            }
        }
        // There are no more previous regions to inspect, and apparently none did lead to a path that leads to "success", so this is a dead end, return false.
        false
    }

//    /// WARNING: Old version, using old iteration termination criterion!!! (For testing only!)
//    fn path_to_error_backwards_old(&self, start: Region, mut cur_path: &mut Vec<Region>)
//                               -> bool {
//        let points_of_cur_region: Vec<PointIndex> = self.points_of_region(start);
//        let start_points_of_cur_region = self.find_start_points(&points_of_cur_region);
//        debug!("cur_region (start): {:?}", start);
//        debug!("points_of_cur_region: {:?}", points_of_cur_region);
//        debug!("start_points_of_cur_region: {:?}", start_points_of_cur_region);
//
//        // add start to the path, as it will now become part of it.
//        cur_path.push(start);
//
//            for sp in start_points_of_cur_region {
//                if self.start_points_of_error_loan.contains(&sp) {
//                    // this region's start intersects with the start of the loan that causes the error,
//                    // therefore we consider here to be the end of the relevant path, and therefore stop
//                    // (end recursion) and return success (true)
//                    return true
//                }
//            }
//
//    //    let input_loans_of_cur_region =  self.loan_of_reagion(start);
//    //    debug!("input_loans_of_cur_region: {:?}", input_loans_of_cur_region);
//    //
//    //    if self.all_facts.borrow_region.iter().filter(|&(r, l, p)|
//    //        *r == start && *l == self.error_loan
//    //    ).count() > 0 {
//    //        // the start region does include the error loan. (May also be called error borrow.)
//    //        // therefore, stop the recursion here, as we consider this to be gone far enough.
//    //        // Also, this path is considered to lead to success, so return true
//    //        debug!("Success path found by path_to_error_backwards, ending at region {:?}", start);
//    //        return true
//    //    }
//
//        let mut prev_regions = self.find_prev_regions(start);
//        prev_regions.dedup();
//        debug!("prev_regions: {:?}", prev_regions);
//
//        for pr in prev_regions {
//            if cur_path.contains(&pr) {
//                // this element already is part of the path, so there would be circle by adding it again, therefore stop here.
//                continue
//            } else {
//                let mut pr_path = cur_path.clone();
//                if self.path_to_error_backwards_old(pr, &mut pr_path) {
//                    cur_path.clear();
//                    cur_path.append(&mut pr_path);
//                    return true
//                } else {
//                    continue
//                }
//            }
//        }
//        // There are no more previous regions to inspect, and apparently none did lead to a path that leads to "success", so this is a dead end, return false.
//        false
//    }

}

struct MirInfoPrinter<'a, 'tcx: 'a> {
    pub tcx: TyCtxt<'a, 'tcx, 'tcx>,
    pub mir: cell::Ref<'a, mir::Mir<'tcx>>,
//    pub mir: mir::Mir<'tcx>,
    pub borrowck_in_facts: facts::AllInputFacts,
    pub borrowck_out_facts: facts::AllOutputFacts,
    pub interner: facts::Interner,
	pub variable_regions: HashMap<mir::Local, Region>,
    /// This gives the mapping from regions to the locals that introduced them.
    /// This information can be read form a MIR dump by the method regions::load_region_to_local_map
    pub region_to_local_map: HashMap<Region, mir::Local>,
    pub def_path: rustc::hir::map::DefPath,
}


impl<'a, 'tcx> MirInfoPrinter<'a, 'tcx> {
    pub fn print_info(&mut self) -> Result<(), io::Error> {
        self.print_error();
        Ok(())
    }

    fn print_error(&mut self) {
        let mut expl_output = ExplOutput::new();

        let mut path_to_explain_last_error: Vec<Region> = Vec::default();

        for (point, loans) in self.borrowck_out_facts.errors.iter() {
            let err_point_ind = point;
            let err_loans = loans;

            expl_output = compute_error_expl(&self.borrowck_in_facts, &self.borrowck_out_facts, (*err_point_ind, err_loans.clone()));

            debug!("Start searching the path to the error, old version that searches expl_outlives (from expl_output):");
            let mut error_path_finder_old = ErrorPathFinder::new(&self.borrowck_in_facts,
                                                                 &self.borrowck_out_facts,
                                                                 (*err_point_ind, err_loans.clone()),
                                                                 &expl_output.unordered_expl_outlives); // (probably) could also use &self.borrowck_in_facts.outlives
            // (not really tested, but looks like working, but maybe not always deterministic.)
            error_path_finder_old.compute_error_path();
            debug!("-------------------------------------------------------------------------------------------------------------");
            debug!("Start searching the path to the error, new version that searches (default) outlives (from borrowck_in_facts):");
            let mut error_path_finder = ErrorPathFinder::new(&self.borrowck_in_facts,
                                                             &self.borrowck_out_facts,
                                                             (*err_point_ind, err_loans.clone()),
                                                             &self.borrowck_in_facts.outlives);
            let new_path = error_path_finder.compute_error_path();
            if ! new_path.is_empty() {
                // if the newly found path is non-empty, take it. This prevents that a path that was
                // found before is overwritten by an emtpy (error result)
                path_to_explain_last_error = new_path;
            }
        }

        let mut outlives_at: FxHashMap<(Region, Region), Vec<PointIndex>>;
        let mut outlives_debug = Vec::new();
        outlives_at = FxHashMap::default();

//        for (region,region2,point) in self.borrowck_in_facts.outlives.clone() {
//            outlives_at.entry((region, region2)).or_insert(Vec::new()).push(point);
//            outlives_debug.push((region,region2,point));
//        }

        for (point, region_map) in expl_output.expl_outlives {
            //println!("test: {:?}", point);
            for (region, regions) in region_map {
                for region2 in regions {
                    //println!("{:?} -> {:?} [LABEL=\"{:?}\"]", region, region2, point);
                    outlives_at.entry((region, region2)).or_insert(Vec::new()).push(point);
                    outlives_debug.push((region, region2, point));
                }
            }
        }

        let mut debug_facts = self.borrowck_in_facts.clone();

        debug_facts.outlives = outlives_debug;

        let output = Output::compute(&debug_facts, Algorithm::Naive, false);

        println!("debug_errors: {:?}", output.errors);

        if ! path_to_explain_last_error.is_empty() {
            let mut graph_to_explain_last_error: FxHashMap<(Region, Region),
                Vec<PointIndex>> = FxHashMap::default();
            let mut prev_region = path_to_explain_last_error.pop().unwrap();
            path_to_explain_last_error.iter().rev().for_each(|&r| {
                let mut points_of_edge: Vec<_> =
                    self.borrowck_in_facts.outlives.iter().filter(|&(r1, r2, _)|
                        *r1 == prev_region && *r2 == r
                    ).map(|&(_, _, p)| p).collect();
                points_of_edge.dedup();
                graph_to_explain_last_error.insert((prev_region, r), points_of_edge);
                prev_region = r;
            }
            );

            debug!("borrowck_in_facts.outlives: {:?}", self.borrowck_in_facts.outlives);

            debug!("graph_to_explain_last_error: {:?}", graph_to_explain_last_error);

            debug!("region_to_local_map: {:?}", self.region_to_local_map);

            let old_error_graph_path = PathBuf::from("nll-facts")
                .join(self.def_path.to_filename_friendly_no_crate())
                .join("old_error_graph.dot");

            self.print_outlive_error_graph_legacy(&graph_to_explain_last_error, &old_error_graph_path);

            let error_graph_path = PathBuf::from("nll-facts")
            .join(self.def_path.to_filename_friendly_no_crate())
            .join("error_graph.dot");

            self.print_outlive_error_graph(&graph_to_explain_last_error, &error_graph_path);
        }

        let expl_graph_path = PathBuf::from("nll-facts")
            .join(self.def_path.to_filename_friendly_no_crate())
            .join("outlive_graph.dot");
        self.print_outlive_error_graph_legacy(&outlives_at, &expl_graph_path);
    }

    /// This function will write a graph (in dot/Graphviz format) to a file. This graph either is
    /// intended to describe a lifetime error in a program or it is an outlives graph (of some
    /// portion) of a Rust program. During the printing process, the graph is also enriched with
    /// some information from the program (e.g. source lines) that shall help to understand it and
    /// to associate it with the program it originates form.
    /// The graph that shall be printed is given as an FxHashMap that maps from a tuple of regions
    /// to a vector of points. Note that the actual graph is described by the tuples that are used
    /// as keys. These tuples describe the edges of the graph. The points only provided additional
    /// information about where (in the program) these edges arise.
    /// The path to which the graph that will be written is given by the PathBuf graph_out_path. It
    /// must give a complete path to a file, either relative to the working directory or as absolute
    /// path, and it must also contain the file name. Note that any file that already exists at this
    /// location will be overwritten.
    /// WARNING: This is the legacy method that uses the old method(s) for matching regions and
    /// constraints to source lines and locals. This is only kept for testing and showcasing the new
    /// method an will be removed soon. In addition, it will also print reflexive relations in a
    /// special way that was intended to emphasize that regions connected by reflexive edges are
    /// considered as being equal.
    fn print_outlive_error_graph_legacy(&self,
                                 outlives_at: &FxHashMap<(Region, Region), Vec<PointIndex>>,
                                 graph_out_path: &PathBuf) {

        let mut outlive_graph = File::create(graph_out_path).expect("Unable to create file");

        writeln!(outlive_graph, "digraph G {{");

        let mut regions_done = Vec::new();

        for ((region1, region2), points) in outlives_at.iter() {
            if regions_done.contains(&(((region2, region1), points), 0)) {
                regions_done.remove_item(&(((region2, region1), points), 0));
                regions_done.push((((region2, region1), points), 1))
            }else if !regions_done.contains(&(((region2, region1), points), 0)) && !regions_done.contains(&(((region1, region2), points), 0)) {
                regions_done.push((((region1, region2), points), 0));
            }

        }

        let mut i = 0;

        for (((region1, region2), points), eq) in regions_done.iter() {
            let mut local_name1 = String::default();
            let mut local_name2 = String::default();
            let mut local_source1 = syntax_pos::DUMMY_SP;
            let mut local_source2 = syntax_pos::DUMMY_SP;
            let mut fm_ln1;
            let mut fm_ln2;
            let mut local_source1_line = usize::default();
            let mut local_source2_line= usize::default();
            let mut local_source1_snip = String::default();
            let mut local_source2_snip= String::default();
            let mut point_ln;
            let mut point_snip = String::default();
            //let mut anonym1_snip;

            for (local_x, rv) in self.variable_regions.iter() {
                if *region1 == rv {
                    let local_decl = &self.mir.local_decls[*local_x];
                    if local_decl.name != None {
                        local_name1 = local_decl.name.unwrap().to_string();
                        local_source1 = local_decl.source_info.span;
                    } else {
                        local_name1 = ("anonymous Variable").to_string();
                        for block_data in self.mir.basic_blocks().iter(){
                            for stmt in block_data.statements.iter(){
                                if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
                                    match l.local() {
                                        Some(v) => if v==*local_x{
                                            local_source1 = stmt.source_info.span;
                                        }

                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    fm_ln1 = self.tcx.sess.source_map().lookup_line(local_source1.lo()).unwrap();
                    local_source1_snip = fm_ln1.sf.get_line(fm_ln1.line).unwrap().to_string();
                    //local_source1_snip = self.tcx.sess.codemap().get_source_file(file_name).unwrap().get_line(local_source1_line).unwrap();
                    //local_source1_snip = self.tcx.sess.codemap().span_to_snippet(local_source1).ok().unwrap();
                }
                else if *region2 == rv {
                    let local_decl = &self.mir.local_decls[*local_x];
                    if local_decl.name != None {
                        local_name2 = local_decl.name.unwrap().to_string();
                        local_source2 = local_decl.source_info.span;
                    } else {
                        local_name2 = ("anonymous Variable").to_string();
                        for block_data in self.mir.basic_blocks().iter(){
                            for stmt in block_data.statements.iter(){
                                if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
                                    match l.local() {
                                        Some(v) => if v==*local_x{
                                            local_source2 = stmt.source_info.span;
                                        }

                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
//                    } else {
//                        local_name2 = ("anonymous Variable").to_string();
//                    }
//                    local_source2 = local_decl.source_info.span;
                    fm_ln2 = self.tcx.sess.source_map().lookup_line(local_source2.lo()).unwrap();
                    local_source2_snip = fm_ln2.sf.get_line(fm_ln2.line).unwrap().to_string();
                    //local_source2_line = self.tcx.sess.codemap().lookup_char_pos_adj(local_source2.lo()).line;
                    //local_source2_snip = self.tcx.sess.codemap().span_to_snippet(local_source2).ok().unwrap();
                }
            }

            let mut points_sort = points.clone();
            //println!("points {:?}: {:?}", i, points_sort);
            let mut ind = usize::max_value();
            //let mut point_x = self.interner.get_point(points_sort[0]);
            //println!("unsirted: {:?}",points_sort);
            for point in points_sort.iter(){
                let point1 = self.interner.get_point(*point);
                let point_location = point1.location;
                let point_block = &self.mir[point_location.block];
                let point_span;
                if point_block.statements.len() == point_location.statement_index{
                    let terminator = point_block.terminator.as_ref().unwrap();
                    point_span = terminator.source_info.span;
                }else {
                    let stmt_x = &point_block.statements[point_location.statement_index];
                    point_span = stmt_x.source_info.span;
                }
                let point_line = self.tcx.sess.source_map().lookup_char_pos(point_span.lo()).line;
                if point_line < ind {
                    ind = point_line;
                    point_ln = self.tcx.sess.source_map().lookup_line(point_span.lo()).unwrap();
                    point_snip = point_ln.sf.get_line(ind-1).unwrap().to_string();
                }

            }

            if local_source1_snip != String::default(){
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr></table>> ]", region1, region1, local_name1, region1, local_source1_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            }else {
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr></table>> ]", region1, region1, local_name1, region1);
            }
            if local_source2_snip != String::default(){
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr></table>> ]", region2, region2, local_name2, region2, local_source2_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            }else {
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr></table>> ]", region2, region2, local_name2, region2);
            }


            if *eq==0 {

                writeln!(outlive_graph, "{:?} [ shape=plaintext, label=  <<table><tr><td> Constraint </td></tr><tr><td> {:?} may point to {:?}</td></tr><tr><td> generated at line {:?}: </td></tr><tr><td> {} </td></tr></table>>  ]", i, region2, region1, ind, point_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
                writeln!(outlive_graph, "{:?} -> {:?} -> {:?}\n", region1, i, region2);
            }

            if *eq==1 {
                writeln!(outlive_graph, "{:?} [ shape=plaintext, label=  <<table><tr><td> Equal </td></tr><tr><td> {:?} and {:?}  may point to each other </td></tr><tr><td> generated at line {:?}: </td></tr><tr><td> {} </td></tr></table>>  ]", i, region1, region2, ind, point_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
                writeln!(outlive_graph, "{:?} -> {:?} -> {:?} [color= \"black:invis:black\", arrowhead=none]\n", region1, i, region2);
                writeln!(outlive_graph, "{{rank=same; {:?} {:?} {:?}}}\n", region1, region2, i);
            }

            i += 1;
        }


        writeln!(outlive_graph, "}}");
    }

    /// This function will write a graph (in dot/Graphviz format) to a file. This graph either is
    /// intended to describe a lifetime error in a program or it is an outlives graph (of some
    /// portion) of a Rust program. During the printing process, the graph is also enriched with
    /// some information from the program (e.g. source lines) that shall help to understand it and
    /// to associate it with the program it originates form.
    /// The graph that shall be printed is given as an FxHashMap that maps from a tuple of regions
    /// to a vector of points. Note that the actual graph is described by the tuples that are used
    /// as keys. These tuples describe the edges of the graph. The points only provided additional
    /// information about where (in the program) these edges arise.
    /// The path to which the graph that will be written is given by the PathBuf graph_out_path. It
    /// must give a complete path to a file, either relative to the working directory or as absolute
    /// path, and it must also contain the file name. Note that any file that already exists at this
    /// location will be overwritten.
    /// NOTE: This is the new version that will use new ways to find the locals and source lines
    /// that are relevant for regions and constraints. Also, it will no longer check if two regions
    /// are "equal" (edge from both to each other) and hence no longer print reflexive edges in a
    /// special way, since such edges do no longer exist in the error path graph. (As this graph
    /// describes a single-direction path, that is part of the outlives relation graph.)
    fn print_outlive_error_graph(&self,
                                graph_information: &FxHashMap<(Region, Region), Vec<PointIndex>>,
                                graph_out_path: &PathBuf) {

        let mut outlive_graph = File::create(graph_out_path).expect("Unable to create file");

        writeln!(outlive_graph, "digraph G {{");

//        let mut regions_done = Vec::new();
//
//        for ((region1, region2), points) in outlives_at.iter() {
//            if regions_done.contains(&(((region2, region1), points), 0)) {
//                regions_done.remove_item(&(((region2, region1), points), 0));
//                regions_done.push((((region2, region1), points), 1))
//            }else if !regions_done.contains(&(((region2, region1), points), 0)) && !regions_done.contains(&(((region1, region2), points), 0)) {
//                regions_done.push((((region1, region2), points), 0));
//            }
//
//        }

        let mut i = 0;

        for ((region1, region2), points) in graph_information.iter() {
            let mut local_name1 = String::default();
            let mut local_name2 = String::default();
            let mut local_source1 = syntax_pos::DUMMY_SP;
            let mut local_source2 = syntax_pos::DUMMY_SP;
//            let mut fm_ln1;
//            let mut fm_ln2;
            let mut local_source1_snip = String::default();
            let mut local_source2_snip= String::default();
            let mut point_ln;
            let mut point_snip = String::default();
            //let mut anonym1_snip;

//            for (local_x, rv) in self.variable_regions.iter() {
//                if *region1 == rv {
//                    let local_decl = &self.mir.local_decls[*local_x];
//                    if local_decl.name != None {
//                        local_name1 = local_decl.name.unwrap().to_string();
//                        local_source1 = local_decl.source_info.span;
//                    } else {
//                        local_name1 = ("anonymous Variable").to_string();
//                        for block_data in self.mir.basic_blocks().iter(){
//                            for stmt in block_data.statements.iter(){
//                                if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
//                                    match l.local() {
//                                        Some(v) => if v==*local_x{
//                                            local_source1 = stmt.source_info.span;
//                                        }
//
//                                        _ => {}
//                                    }
//                                }
//                            }
//                        }
//                    }
//
//                    fm_ln1 = self.tcx.sess.source_map().lookup_line(local_source1.lo()).unwrap();
//                    local_source1_snip = fm_ln1.sf.get_line(fm_ln1.line).unwrap().to_string();
//                    //local_source1_snip = self.tcx.sess.codemap().get_source_file(file_name).unwrap().get_line(local_source1_line).unwrap();
//                    //local_source1_snip = self.tcx.sess.codemap().span_to_snippet(local_source1).ok().unwrap();
//                }
//                else if *region2 == rv {
//                    let local_decl = &self.mir.local_decls[*local_x];
//                    if local_decl.name != None {
//                        local_name2 = local_decl.name.unwrap().to_string();
//                        local_source2 = local_decl.source_info.span;
//                    } else {
//                        local_name2 = ("anonymous Variable").to_string();
//                        for block_data in self.mir.basic_blocks().iter(){
//                            for stmt in block_data.statements.iter(){
//                                if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
//                                    match l.local() {
//                                        Some(v) => if v==*local_x{
//                                            local_source2 = stmt.source_info.span;
//                                        }
//
//                                        _ => {}
//                                    }
//                                }
//                            }
//                        }
//                    }
////                    } else {
////                        local_name2 = ("anonymous Variable").to_string();
////                    }
////                    local_source2 = local_decl.source_info.span;
//                    fm_ln2 = self.tcx.sess.source_map().lookup_line(local_source2.lo()).unwrap();
//                    local_source2_snip = fm_ln2.sf.get_line(fm_ln2.line).unwrap().to_string();
//                    //local_source2_line = self.tcx.sess.codemap().lookup_char_pos_adj(local_source2.lo()).line;
//                    //local_source2_snip = self.tcx.sess.codemap().span_to_snippet(local_source2).ok().unwrap();
//                }
//            }

            if let Some(local_x1) = self.region_to_local_map.get(region1) {
                // there is a local (x) for region one, get some details about it
                // (code copied from the old version)
                let local_decl = &self.mir.local_decls[*local_x1];
                if local_decl.name != None {
                    local_name1 = local_decl.name.unwrap().to_string();
                    local_source1 = local_decl.source_info.span;
                } else {
                    local_name1 = ("anonymous Variable").to_string();
                    for block_data in self.mir.basic_blocks().iter() {
                        for stmt in block_data.statements.iter() {
                            if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
                                match l.local() {
                                    Some(v) => if v==*local_x1 {
                                        local_source1 = stmt.source_info.span;
                                    }

                                    _ => {}
                                }
                            }
                        }
                    }
                }
                let fm_ln1 = self.tcx.sess.source_map().lookup_line(local_source1.lo()).unwrap();
                local_source1_snip = fm_ln1.sf.get_line(fm_ln1.line).unwrap().to_string();
            } else {
                debug!("No locale (and hence no extra details) found for region1={:?}", region1);
            }

            if let Some(local_x2) = self.region_to_local_map.get(region2) {
                // there is a local (x) for region tow, get some details about it
                // (code copied from the old version)
                let local_decl = &self.mir.local_decls[*local_x2];
                if local_decl.name != None {
                    local_name2 = local_decl.name.unwrap().to_string();
                    local_source2 = local_decl.source_info.span;
                } else {
                    local_name2 = ("anonymous Variable").to_string();
                    for block_data in self.mir.basic_blocks().iter() {
                        for stmt in block_data.statements.iter() {
                            if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
                                match l.local() {
                                    Some(v) => if v==*local_x2{
                                        local_source2 = stmt.source_info.span;
                                    }

                                    _ => {}
                                }
                            }
                        }
                    }
                }
                let fm_ln2 = self.tcx.sess.source_map().lookup_line(local_source2.lo()).unwrap();
                local_source2_snip = fm_ln2.sf.get_line(fm_ln2.line).unwrap().to_string();
            } else {
                debug!("No locale (and hence no extra details) found for region2={:?}", region2);
            }

            let mut points_sort = points.clone();
            //println!("points {:?}: {:?}", i, points_sort);
            let mut ind = usize::max_value();
            //let mut point_x = self.interner.get_point(points_sort[0]);
            //println!("unsirted: {:?}",points_sort);
            for point in points_sort.iter(){
                let point1 = self.interner.get_point(*point);
                let point_location = point1.location;
                let point_block = &self.mir[point_location.block];
                let point_span;
                if point_block.statements.len() == point_location.statement_index{
                    let terminator = point_block.terminator.as_ref().unwrap();
                    point_span = terminator.source_info.span;
                }else {
                    let stmt_x = &point_block.statements[point_location.statement_index];
                    point_span = stmt_x.source_info.span;
                }
                let point_line = self.tcx.sess.source_map().lookup_char_pos(point_span.lo()).line;
                if point_line < ind {
                    ind = point_line;
                    point_ln = self.tcx.sess.source_map().lookup_line(point_span.lo()).unwrap();
                    point_snip = point_ln.sf.get_line(ind-1).unwrap().to_string();
                }

            }

            if local_source1_snip != String::default(){
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr></table>> ]", region1, region1, local_name1, region1, local_source1_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            }else {
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr></table>> ]", region1, region1, local_name1, region1);
            }
            if local_source2_snip != String::default(){
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr></table>> ]", region2, region2, local_name2, region2, local_source2_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            }else {
                writeln!(outlive_graph, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr></table>> ]", region2, region2, local_name2, region2);
            }

            // write the box (graph node)  with the constraint information, and the edges around it.
            writeln!(outlive_graph, "{:?} [ shape=plaintext, label=  <<table><tr><td> Constraint </td></tr><tr><td> {:?} may point to {:?}</td></tr><tr><td> generated at line {:?}: </td></tr><tr><td> {} </td></tr></table>>  ]", i, region2, region1, ind, point_snip.replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            writeln!(outlive_graph, "{:?} -> {:?} -> {:?}\n", region1, i, region2);

            i += 1;
        }


        writeln!(outlive_graph, "}}");
    }

    /// Method that produces a mpp that links regions to the locals that introduces this region.
    /// This information is read from the MIR that is avilabel as part of self, or more exactly,
    /// from the type information in the MIR.
    /// This map is returned (And can be used to set self.region_to_local_map)
    /// NOTE: This method does not work successfully, since the version of MIR that is available does
    /// not contain all needed information. More specifically, the region "names" in this MIR version
    /// are no longer existing in the form that they were when borrowchking, and therefore the mapping
    /// from facts::Regions to the regions in the MIR will not succeed.
    /// Therefore go back to parsing a dump of MIR that does contain this needed information.
//    fn compute_region_to_local_map(&mut self) -> HashMap<Region, mir::Local> {
//        let mut result = HashMap::new();
//        debug!("MIR phase: {:?}", self.mir.phase);
//
//        for (local, local_decl) in self.mir.local_decls.iter_enumerated() {
//            debug!("local: {:?}, local_decl: {:?}", local, local_decl);
//            let mut locals_regions: Vec<ty::Region> = Vec::new();
//            for local_ty in local_decl.ty.walk() {
//                match local_ty.sty {
//                    ty::Ref(reg, _, _) => locals_regions.push(reg),
//                    ty::Adt(_, substs_ref) => {
//                        for reg in substs_ref.regions() {
//                            locals_regions.push(reg);
//                        }
//                    },
//                    ty::RawPtr(_) => debug!("Hit ty::RawPtr!!!"),
//                    // TODO handle more options, probably ADT is needed, maybe even more (but ev. only for special/corner cases)
//                    _ => {}
//                }
//            }
//            debug!("locals_regions: {:?}", locals_regions);
//            for reg in locals_regions {
////                let fact_reg = match reg {
////                    ty::ReVar(vid) => Region::from(vid.index()),
////                    ty::ReScope(scope) => Region::from(scope.id.index()),
////                    _ => continue,
////                    // TODO maybe need to cover more variants, it is not clear as of now which are needed.
////                };
//                //let fact_reg = Region::from(format!("{}", reg));
//                debug!("{}", reg);
//                //result.insert(fact_reg, local);
//            }
//        }
//
//        result
//    }

    fn get_lines_for_region(&self, reg: Region, map: Vec<(Region, Loan, PointIndex)>)
            -> Vec<(usize, String)>{
        let mut result: Vec<(usize, String)> = Vec::new();

        result
    }

    fn get_points_for_region(&self, reg: Region, map: Vec<(Region, Loan, PointIndex)>)
            -> Vec<PointIndex> {
        map.iter().filter(|&(r, _, _)| *r == reg).map(|(_, _, p)| *p).collect()
    }
}


