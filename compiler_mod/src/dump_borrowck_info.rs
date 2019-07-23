
pub extern crate csv;
extern crate datafrog;
pub extern crate polonius_engine;
pub extern crate regex;
pub extern crate rustc;
pub extern crate rustc_data_structures;
pub extern crate serde;
pub extern crate serde_json;
pub extern crate serde_derive;
pub extern crate syntax_pos;

use super::facts;
use super::regions;

use std::{cell};
use std::env;
use std::collections::{HashMap,BTreeMap, BTreeSet};
use std::fs::{File, remove_dir};
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
use facts::Point;

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
        debug!("cur_region (start): {:?}", start);

        // add start to the path, as it will now become part of it.
        cur_path.push(start);

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

            let mut enriched_graph_to_explain_last_error =
                self.create_enriched_graph(&graph_to_explain_last_error, &self.borrowck_in_facts.borrow_region);

            self.print_outlive_error_graph(&enriched_graph_to_explain_last_error, &error_graph_path);

            let error_graph_path_improved = PathBuf::from("nll-facts")
                .join(self.def_path.to_filename_friendly_no_crate())
                .join("error_graph_improved.dot");

            enriched_graph_to_explain_last_error.improve_graph();

            self.print_outlive_error_graph(&enriched_graph_to_explain_last_error, &error_graph_path_improved);

            // Write the JSON dump to a directory that does not depend on the method name. Note that
            // this will not work well with multiple errors (in different methods), since the file
            // would be overwriten for each error. However, for now we limit the tools scope to
            // only deal with files with a single error for simplicity. This could be changed in the
            // future, e.g. by allowing control to the user regarding the method that shall be
            // handled.
            let error_graph_path_json = PathBuf::from("nll-facts")
//                .join(self.def_path.to_filename_friendly_no_crate())
                .join("error_graph.json");

            self.dump_outlive_error_graph_as_json(&enriched_graph_to_explain_last_error, &error_graph_path_json);

//            let error_graph_path_with_requires = PathBuf::from("nll-facts")
//                .join(self.def_path.to_filename_friendly_no_crate())
//                .join("error_graph_with_requires.dot");
//
//            // TODO if we want to keep using "requires" here, e.g. for enriching and printing the
//            // TODO graph, then we should probably compute and store it before calling compute_error_path(...),
//            // TODO and do not compute it again here, and hence computing it twice.
//            let mut requires = self.borrowck_in_facts.borrow_region.clone();
//            requires.extend(
//                self.borrowck_out_facts.restricts.iter().flat_map(
//                    |(&point, region_map)|
//                        region_map.iter().flat_map(
//                            move |(&region, loans)|
//                                loans.iter().map(move |&loan| (region, loan, point))
//                        )
//                )
//            );
//
//            let enriched_graph_to_explain_last_error_requires =
//                self.create_enriched_graph(&graph_to_explain_last_error, &requires);
//
//            self.print_outlive_error_graph(&enriched_graph_to_explain_last_error_requires, &error_graph_path_with_requires);

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
    /// portion) of a Rust program. In addition to the actual graph, also quite some enriching
    /// information from the program (e.g. source lines) that shall help to understand it and
    /// to associate it with the program it originates form is printed.
    /// The graph that shall be printed is given as an EnrichedErrorGraph struct, that does not
    /// only provide the information about the edges of the graph, but also all enriching
    /// information that shall be printed.
    /// The path to which the graph that will be written is given by the PathBuf graph_out_path. It
    /// must give a complete path to a file, either relative to the working directory or as absolute
    /// path, and it must also contain the file name. Note that any file that already exists at this
    /// location will be overwritten.
    /// NOTE: This is the new version that will use enriching information that is given as argument.
    /// The functionality to get this information (and even more) that was present in the legacy
    /// version of this method is now provided by some new methods that can be used to fill the
    /// EnrichedErrorGraph before passing it to this method.
    /// Also, this method will no longer check if two regions are "equal" (edge from both to each
    /// other) and hence no longer print reflexive edges in a special way, since such edges do no
    /// longer exist in the error path graph. (As this graph describes a single-direction path, that
    /// is part of the outlives relation graph.)
    fn print_outlive_error_graph(&self,
                                error_graph: &EnrichedErrorGraph,
                                graph_out_path: &PathBuf) {

        let mut graph_file = File::create(graph_out_path).expect("Unable to create file");

        writeln!(graph_file, "digraph G {{");

        let mut i = 0;

        for (region1, region2) in error_graph.edges.iter() {
            let mut point_snip = String::default();

            let (local_name1, local_source1_snip) = &error_graph.locals_info_for_regions[region1];
            let (local_name2, local_source2_snip) = &error_graph.locals_info_for_regions[region2];

            let (ind, point_snip) = &error_graph.lines_for_edges[&(*region1, *region2)];

            let mut region1_lines_str = String::default();
            let mut region2_lines_str = String::default();
            for (line_nr, line_str) in error_graph.lines_for_regions[region1].iter() {
                region1_lines_str.push_str(&format!("<tr><td>{}: {}</td></tr>", line_nr, line_str.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;")));
            }
            for (line_nr, line_str) in error_graph.lines_for_regions[region2].iter() {
                region2_lines_str.push_str(&format!("<tr><td>{}: {}</td></tr>", line_nr, line_str.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;")));
            }

            if *local_source1_snip != String::default(){
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr>{}</table>> ]", region1, region1, local_name1, region1, local_source1_snip.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"), region1_lines_str);
            }else {
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr>{}</table>> ]", region1, region1, local_name1, region1, region1_lines_str
                );
            }
            if *local_source2_snip != String::default(){
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}</td></tr>{}</table>> ]", region2, region2, local_name2, region2, local_source2_snip.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"), region2_lines_str);
            }else {
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr>{}</table>> ]", region2, region2, local_name2, region2, region2_lines_str);
            }

            // write the box (graph node)  with the constraint information, and the edges around it.
            writeln!(graph_file, "{:?} [ shape=plaintext, label=  <<table><tr><td> Constraint </td></tr><tr><td> {:?} may point to {:?}</td></tr><tr><td> generated at line {:?}: </td></tr><tr><td> {} </td></tr></table>>  ]", i, region2, region1, ind, point_snip.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"));
            writeln!(graph_file, "{:?} -> {:?} -> {:?}\n", region1, i, region2);

            i += 1;
        }


        writeln!(graph_file, "}}");
    }

    /// This function will serialize a graph to JSON and write it to a file. This graph either is
    /// intended to describe a lifetime error in a program or it is an outlives graph (of some
    /// portion) of a Rust program. In addition to the actual graph, also quite some enriching
    /// information from the program (e.g. source lines) that shall help to understand it and
    /// to associate it with the program it originates form is included.
    /// The graph that shall be dumped is given as an EnrichedErrorGraph struct, that does not
    /// only provide the information about the edges of the graph, but also all enriching
    /// information that shall be included.
    /// The path to which the file that will be written is given by the PathBuf graph_out_path. It
    /// must give a complete path to a file, either relative to the working directory or as absolute
    /// path, and it must also contain the file name. Note that any file that already exists at this
    /// location will be overwritten.
    fn dump_outlive_error_graph_as_json(&self,
                                        error_graph: &EnrichedErrorGraph,
                                        graph_out_path: &PathBuf) {
        let mut out_file = File::create(graph_out_path).expect("Unable to create file");
        let res = serde_json::to_writer_pretty(out_file, error_graph);
        // TODO ev. remove pretty when done with debugging!
        debug!("Result from dumping: {:?}", res);
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

    /// Function that creates the enriched error graph for the passed graph information.
    /// This will take a graph as FxJaschMap from pairs of regions (these represent the edges)
    /// to points. (these are some extra information about the edges that can be obtained from the
    /// outlives relation)
    /// The graph is then returned as an EnrichedErrorGraph that contains quite some extra
    /// information about the graph, and especially about it's relation to the source code (i.e.
    /// the program) it originates from. This extra information is extracted from information
    /// that is available as part of self (e.g. a TyCtx or a parsed MIR dump or the input/output
    /// facts of Polonius), by using some methos that are also available in the MirInofPrint (i.e
    /// self)
    /// In addition, this method needs a "map" from points to loans and regions. This is the map
    /// that will be passed to get_lines_for_region(...) for the mapping, so see this method's
    /// documentation for more details.
    fn create_enriched_graph(&self, graph_information: &FxHashMap<(Region, Region), Vec<PointIndex>>,
                             region_loan_point_map: &Vec<(Region, Loan, PointIndex)>)
            -> EnrichedErrorGraph {
        let mut edges: Vec<(Region, Region)> =  graph_information.keys().map(|&(r1, r2)| (r1, r2)).collect();
        edges.dedup();
        let mut locals_mir_for_regions = FxHashMap::default();
        let mut locals_info_for_regions = FxHashMap::default();
        let mut lines_for_edges = FxHashMap::default();
        let mut lines_for_edges_start = FxHashMap::default();
        let mut lines_for_regions = FxHashMap::default();

        for ((r1, r2), pts) in graph_information.iter() {
            if ! locals_info_for_regions.contains_key(r1) {
                let (local_decl, local_name, local_src) = self.find_local_for_region(r1);
                locals_mir_for_regions.insert(*r1, local_decl);
                locals_info_for_regions.insert(*r1, (local_name, local_src));
            }
            if ! locals_info_for_regions.contains_key(r2) {
                let (local_decl, local_name, local_src) = self.find_local_for_region(r2);
                locals_mir_for_regions.insert(*r2, local_decl);
                locals_info_for_regions.insert(*r2, (local_name, local_src));
            }
            let line_for_egge_points = self.find_first_line_for_points(pts);
            lines_for_edges.insert((*r1, *r2), line_for_egge_points.clone());
            lines_for_edges_start.insert(*r1, line_for_egge_points);

            if ! lines_for_regions.contains_key(r1) {
                lines_for_regions.insert(*r1, self.get_lines_for_region(*r1, region_loan_point_map));
            }
            if ! lines_for_regions.contains_key(r2) {
                lines_for_regions.insert(*r2, self.get_lines_for_region(*r2, region_loan_point_map));
            }
        }

        EnrichedErrorGraph{
            function_name: self.def_path.to_filename_friendly_no_crate(),
            edges,
            locals_mir_for_regions,
            locals_info_for_regions,
            lines_for_regions,
            lines_for_edges,
            lines_for_edges_start,
        }

    }

    /// This method finds all lines (of source code) that are involved in a certain region.
    /// For this, it will first look up all points that are affected by this region in the map
    /// that must be passed. Thereby it is intended that the map is either the borrow_region or the
    /// requires relation. (These are obtained from the Polonius input/output facts) The result
    /// might differ depending on the used relation.
    /// Duplicate lines, i.e. lines that have the same line number are ignored, i.e. each line is
    /// present at most once in the result. In adition, the lines will be sorted by ascending line
    /// number in the result.
    /// The resulting set of lines is returned as a vector filled with tupes. The first element is
    /// the number of the line, as usize, and the second is the actual source code (text), as
    /// String.
    fn get_lines_for_region(&self, reg: Region, map: &Vec<(Region, Loan, PointIndex)>)
            -> Vec<(usize, String)>{
        let mut result: Vec<(usize, String)> = Vec::new();
        for pt in self.get_points_for_region(reg, map) {
            let (line_nr, line_str) = self.get_line_for_point(pt);
            if result.iter().find(|(n, _)| *n == line_nr).is_none() {
                result.push((line_nr, line_str));
            }
        }
        // This will sort the vector by line numbers. (First element of the tuples in the vector,
        // and there are no duplicates. Therefore, we can also use the faster unstable sort.)
        result.sort_unstable();
        result
    }

    /// Helper method for get_lines_for_region(...), it obtains all points that are associated with
    /// a given region in the map and returns them as a vector.
    fn get_points_for_region(&self, reg: Region, map: &Vec<(Region, Loan, PointIndex)>)
            -> Vec<PointIndex> {
        map.iter().filter(|&(r, _, _)| *r == reg).map(|(_, _, p)| *p).collect()
    }

    /// Method that maps from a point (given as argument) to a source line. The information about
    /// the line is obtained from the interner, the mir and the TyCtx that are part of self.
    /// The resulting line is returned as a tuple giving first the line number, as usize, and then
    /// the actual source code (text), as String.
    fn get_line_for_point(&self, pt: PointIndex) -> (usize, String) {
        // code for the mapping copied from the (legacy) print_outlive_error_graph method,
        // slightly adapted.
        let point1 = self.interner.get_point(pt);
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
        let point_ln = self.tcx.sess.source_map().lookup_line(point_span.lo()).unwrap();
        let point_snip = point_ln.sf.get_line(point_line-1).unwrap().to_string();
        (point_line, point_snip)
    }

    /// This function takes a Region and (tries to) map it to a local that introduced this region,
    /// using the region_to_local_map from self.
    /// It will return the a tuple of Strings. The first is the name of the local, and the second
    /// the source code (text) that introduced this local and hence it's connection to the region.
    /// If the found local has no name, the text "anonymous variable" is returned instead.
    /// If the mapping to a local fails, an empty string is returned as name and as source, and an
    /// message informing about this is logged at debug level. In addition, in this case, or when
    /// the mapping to a source code snipped fails, an empty string will be returned as well.
    fn find_local_for_region(&self, reg: &Region) -> (Option<mir::LocalDecl>, String, String) {
        let mut local_name = String::default();
        let mut local_source = syntax_pos::DUMMY_SP;
        let mut local_source_snip = String::default();
        let mut local_decl_option = None;

        if let Some(local_x1) = self.region_to_local_map.get(reg) {
            // there is a local (x) for reg, get some details about it
            // (code copied from an old version)
            let local_decl = &self.mir.local_decls[*local_x1];
            if local_decl.name != None {
                local_name = local_decl.name.unwrap().to_string();
                local_source = local_decl.source_info.span;
            } else {
                local_name = ("anonymous Variable").to_string();
                for block_data in self.mir.basic_blocks().iter() {
                    for stmt in block_data.statements.iter() {
                        if let mir::StatementKind::Assign(ref l, ref r) = stmt.kind{
                            match l.local() {
                                Some(v) => if v==*local_x1 {
                                    local_source = stmt.source_info.span;
                                }

                                _ => {}
                            }
                        }
                    }
                }
            }
            let fm_ln1 = self.tcx.sess.source_map().lookup_line(local_source.lo()).unwrap();
            local_source_snip = fm_ln1.sf.get_line(fm_ln1.line).unwrap().to_string();
            local_decl_option = Some(local_decl.clone());
        } else {
            debug!("No locale (and hence no extra details) found for region={:?}", reg);
        }
        (local_decl_option, local_name, local_source_snip)
    }

    /// This function takes a set of points, and returns the first line (line on the lowest line)
    /// that is related to these points.
    /// Such a set of points can e.g. be obtained as extra information to an edged in the outlives
    /// graph.
    /// The result consists of a tuple that contains a line number, as usize, and the source code
    /// (text) of the line, as String.
    fn find_first_line_for_points(&self, pts: &Vec<PointIndex>) -> (usize, String) {
        // most of the was copied from the (legacy) print_outlive_error_graph method
        let mut point_snip = String::default();
        let mut ind = usize::max_value();
        for point in pts.iter(){
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
                let point_ln = self.tcx.sess.source_map().lookup_line(point_span.lo()).unwrap();
                point_snip = point_ln.sf.get_line(ind-1).unwrap().to_string();
            }
        }
        (ind, point_snip)
    }
}

/// This struct describes a graph that explains a lifetime error in a method of a Rust program.
/// The graph is connecting all regions/lifetimes that are relevant for this error by edges.
/// In addition, this struct does also store quite soem extra information about this graph and
/// the regions it touches that can be used to print the graph with a lot of explanatory information
/// that shall be helpful for the programmers.
/// This struct does not provided most methods that are needed for creating this graph and the extra
/// information. Instead, these are provided by the MirInfoPrinter, since it contains a lot of
/// information that is needed to create the enriched graph. This struct is primarily intended to
/// store the information.
#[derive(serde_derive::Serialize)]
struct EnrichedErrorGraph<'tcx> {
    /// This shall give the name of the method/function that this error was found in, and hence the
    /// function/method from whom information is depicted by this graph.
    /// NOTE: For now, no fixed decisions regarding the format of the name were taken.
    function_name: String,
    /// This is the core of the graph, the edges that define it
    edges: Vec<(Region, Region)>,
    /// This map shall contain an entry for all regions that are part of the graph, and give the
    /// local that introduces this region. (If it was found.)
    /// If the local is found, the first element shall be Some(the MIR of the local declaration),
    /// otherwise it shall be None. Note that this field is not included when serializing this
    /// structure to JSON, as mir::LocalDecl is not serializable.
    #[serde(skip_serializing)]
    locals_mir_for_regions: FxHashMap<Region, Option<mir::LocalDecl<'tcx>>>,
    /// This map shall contain an entry for all regions that are part of the graph, and give
    /// (textual) information for the local that introduces this region, i.e mainly the source
    /// code of the corresponding line.
    /// If the local is found, If the local was found, the first element is the name of the local
    /// (or something like "anonymous variable" if it has no name), and the second element is
    /// intended to be the source line that introduced this local and hence the region. (both as
    /// text/String)
    /// The Strings shall be empty if the information was not found for an edge. (This is certainly
    /// the case if the corresponding entry in locals_mir_for_regions is None)
    /// This map will be included in a JSON dump of this structure.
    locals_info_for_regions: FxHashMap<Region, (String, String)>,
    /// This maps from regions to a list of lines that are considered to be relevant for this region
    /// The information could have been obtained by using the method
    /// MirInfoPrinter::get_lines_for_region(...) with an appropriate map.
    /// The lines is always given as it's number (usize) and it's source code (text, String)
    lines_for_regions: FxHashMap<Region, Vec<(usize, String)>>,
    /// This maps from edges (given as tow regions) to a line that is considered to be have created
    /// this edged/constraint.
    /// The information could have been obtained from the points that are associated with this edge
    /// in the outlives relation.
    /// The lines is always given as it's number (usize) and it's source code (text, String)
    /// Since serde(_json) does dislike tuples as keys for maps when serializing (leads to error
    /// "key must be a string"), this field will not be included in a JSON dump of this structure.
    /// Instead, the simplified lines_for_edges_start will be included.
    #[serde(skip_serializing)]
    lines_for_edges: FxHashMap<(Region, Region), (usize, String)>,
    /// This is the same as lines_for_edges, hence it maps from edges to a line that is considered
    /// to be have created this edged/constraint.
    /// However, it only identifies edges by the first region, i.e. the region the edge starts at.
    /// Therefore, this map can (and will) be included when creating a JSON dump of this structure.
    lines_for_edges_start: FxHashMap<Region, (usize, String)>

}

impl<'tcx> EnrichedErrorGraph<'tcx> {
    /// This method operates (only) on the edges of the graph and finds one region that is an entry
    /// region, i.e. a region that is a graph node with no ingoing edges. It will simply return the
    /// first region that fulfils this criterion that is encountered. This is fine, as the error
    /// graphs actually describe a path, and hence they should have only one entry node/region.
    /// If no such region is found (this is not expected for current error paths, but might happen
    /// if the graph would be cyclic), then Region(usize::max_value()) is returned.
    fn find_entry_region (&self) -> Region {
        let mut result= Region::from(usize::max_value());
        for (r1_candidate, _) in self.edges.iter() {
            // we only care about the first node of an edge, since only these can be entry nodes
            // so we check if this region has no predecessors
            if self.edges.iter().find(|(_, r2)| r1_candidate == r2).is_none() {
                result = *r1_candidate;
                break;
            }
        }
        result
    }

    /// This method operates (only) on the edges of the graph and finds one region that is an exit
    /// region, i.e. a region that is a graph node with no outgoing edges. It will simply return the
    /// first region that fulfils this criterion that is encountered. This is fine, as the error
    /// graphs actually describe a path, and hence they should have only one entry node/region.
    /// If no such region is found (this is not expected for current error paths, but might happen
    /// if the graph would be cyclic), then Region(usize::max_value()) is returned.
    fn find_exit_region (&self) -> Region {
        let mut result= Region::from(usize::max_value());
        for (_, r2_candidate) in self.edges.iter() {
            // we only care about the second node of an edge, since only these can be exit nodes
            // so we check if this region has no outgoing edges
            if self.edges.iter().find(|(r1, _)| r2_candidate == r1).is_none() {
                result = *r2_candidate;
                break;
            }
        }
        result
    }

    /// This method will improve the graph that it is called on to make it more readable and
    /// understandable. However, "improving" is somewhat subtle and subjective.
    /// What this method does is removing nodes, and hence regions.
    /// More exactly, it will remove all regions that either are not associated with a local
    /// or that are associated with a local that has no name, hence that is an anonymous variable.
    /// However, the first and the last node in the graph (that actually is a path) will never be
    /// removed. (These regions are found by using the find_entry_region and find_exit_region
    /// methods) The mapping to locals is taken from the locals_for_regions field of self, so it
    /// must be set appropriately before calling this method, otherwise it will not work.
    /// Please note that this method takes a mutable reference to self, and it will indeed mutate
    /// the graph. This is how it will return it's actual results. More exactly, it will change
    /// the set of edges, i.e. it will remove the edges that contain unneeded regions and replace
    /// them with direct edges that connect all previous and posteriors nodes of the removed
    /// node without going over the removed node anymore.
    /// In addition, this method will add an entry to lines_for_edges for all newly created edges.
    /// If two edges are merged (as described before), the information form the first of these tow
    /// edges is inserted as information for the newly created edge. If this information is not
    /// equal to the one of the second edged (based on the line number), a debug message will be
    /// printed to the log in an appropriate log level is set.
    /// All other fields of the EnrichedErrorGraph are not modified, so all information that was
    /// acquired before about the unneeded regions is kept.
    fn improve_graph(&mut self) {
        let first_region = self.find_entry_region();
        let last_region = self.find_exit_region();

        /// internal helper closure that does check if a region si either the first or the last
        /// region in the input path
        let is_not_first_or_last_region = |reg: &Region| {
            *reg != first_region && *reg != last_region
        };

        let mut new_edges = self.edges.clone();
        let mut new_lines_for_edges = self.lines_for_edges.clone();

        /// helper closure that removes regions from the graph by manipulating new_edges.
        /// will never remove entry or exit nodes/regions of a graph, these are ignored.
        /// will also update self.lines_for_edges with info about any newly created edges.
        /// TODO Maybe this could be implemented slightly more efficiently, esp. if using drain_filer on edges. And it might be worth doing so, since it can be used often.
        let mut remove_region_from_edges = |reg: &Region| {
            if is_not_first_or_last_region(reg) {
                let in_edges_start: Vec<(_)> = new_edges.iter().filter(|(_, r2)| reg == r2).map(|&(r1, _)| r1).collect();
                let out_edges_end: Vec<(_)> = new_edges.iter().filter(|(r1, _)| reg == r1).map(|&(_, r2)| r2).collect();

                new_edges = new_edges.iter().filter(|(r1, r2)|
                    r1 != reg && r2 != reg
                ).map(|&edge| edge).collect();

                for r1 in in_edges_start.iter() {
                    for r2 in out_edges_end.iter() {
                        new_edges.push((*r1, *r2));
                        let (in_line_info_nr, in_line_info_src) = &new_lines_for_edges[&(*r1, *reg)];
                        let (out_line_info_nr, _) = &new_lines_for_edges[&(*reg, *r2)];
                        if in_line_info_nr != out_line_info_nr {
                            debug!("graph edges that were merged while improving the graph did not \
                            have the same origin information (line number), for merging edges \
                            ({:?}, {:?}) and ({:?}, {:?}). Will only preserve information for the \
                            first edge.", r1, reg, reg, r2);
                        }
                        new_lines_for_edges.insert((*r1, *r2), (*in_line_info_nr, in_line_info_src.clone()));
                    }
                }
            }
            // else: do nothing, since this is an entry or exit node/region
        };

        for (reg, local_decl_opt) in self.locals_mir_for_regions.iter() {
            match local_decl_opt {
                None => remove_region_from_edges(reg),
                Some(local_decl) => if local_decl.name.is_none() { remove_region_from_edges(reg) }
                                 // else: keep this region
            }
        }
        self.edges = new_edges;
        self.lines_for_edges = new_lines_for_edges;
    }
}
