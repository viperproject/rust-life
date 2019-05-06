
pub extern crate csv;
extern crate datafrog;
pub extern crate polonius_engine;
pub extern crate regex;
pub extern crate rustc;
pub extern crate rustc_driver;
pub extern crate rustc_mir;
pub extern crate rustc_data_structures;
pub extern crate serde;
pub extern crate syntax;
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
use rustc::ty::TyCtxt;
use self::rustc_data_structures::indexed_vec::Idx;
use self::rustc_data_structures::fx::FxHashMap;
use syntax::ast;
use syntax_pos::symbol::Symbol;
use self::datafrog::Relation;



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

    fn visit_fn(&mut self, fk: intravisit::FnKind<'tcx>, _fd: &'tcx hir::FnDecl,
                _b: hir::BodyId, _s: syntax_pos::Span, hir_id: hir::HirId) {
        let name = match fk {
            intravisit::FnKind::ItemFn(name, ..) => name,
            _ => return,
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


        let mir = self.tcx.mir_validated(def_id).borrow();

        let graph_path = PathBuf::from("nll-facts")
            .join(def_path.to_filename_friendly_no_crate())
            .join("graph.dot");
        let graph_file = File::create(graph_path).expect("Unable to create file");
        let graph = BufWriter::new(graph_file);

        let interner = facts_loader.interner;


        let mut mir_info_printer = MirInfoPrinter {
            tcx: self.tcx,
            mir: mir,
            borrowck_in_facts: all_facts,
            borrowck_out_facts: output,
            interner: interner,
			variable_regions: variable_regions,
            def_path: def_path,
        };
        mir_info_printer.print_info();

        trace!("[visit_fn] exit");
    }
}

struct ExplOutput{
    pub expl_outlives: FxHashMap<facts::PointIndex, BTreeMap<facts::Region, BTreeSet<facts::Region>>>,
    pub expl_subset: FxHashMap<facts::PointIndex, BTreeMap<facts::Region, BTreeSet<facts::Region>>>,
    pub expl_requires: FxHashMap<facts::PointIndex, BTreeMap<facts::Region, BTreeSet<facts::Loan>>>,
    pub expl_borrow_live_at: FxHashMap<facts::PointIndex, Vec<facts::Loan>>,

}

impl ExplOutput{

    fn new() -> Self{
        ExplOutput{
            expl_outlives: FxHashMap::default(),
            expl_subset: FxHashMap::default(),
            expl_requires: FxHashMap::default(),
            expl_borrow_live_at: FxHashMap::default(),
        }
    }

}

fn compute_error_expl(all_facts: &facts::AllInputFacts, output: &facts::AllOutputFacts, error_fact: (facts::PointIndex, Vec<facts::Loan>)) -> ExplOutput {


    use self::facts::{PointIndex as Point, Loan, Region};

    let mut result = ExplOutput::new();

    let expl_outlives = {

        let mut iteration = datafrog::Iteration::new();
        // .. some variables, ..
        let subset = iteration.variable::<(Region, Region, Point)>("subset");
        let new_subset = iteration.variable::<((Region, Region, Point),())>("new_subset");
        let outlives = iteration.variable::<(Region, Region, Point)>("outlives");
        let new_outlives = iteration.variable::<((Region, Region, Point),())>("new_outlives");
        let requires = iteration.variable::<(Region, Loan, Point)>("requires");
        let new_requires = iteration.variable::<((Region, Loan, Point),())>("new_requires");
        let borrow_live_at = iteration.variable::<(Loan, Point)>("borrow_live_at");
        let new_borrow_live_at = iteration.variable::<((Loan, Point), ())>("new_borrow_live_at");

        // `invalidates` facts, stored ready for joins
        let invalidates = iteration.variable::<((Loan, Point), ())>("invalidates");

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
        let region_live_at = iteration.variable::<((Region, Point), ())>("region_live_at");
        let cfg_edge_p = iteration.variable::<(Point, Point)>("cfg_edge_p");
        let new_cfg_edge = iteration.variable::<((Point, Point),())>("new_cfg_edge");

        let init_expl_error = iteration.variable::<(Point,Loan)>("init_expl_error");
        let expl_error = iteration.variable::<(Loan,Point)>("expl_error");
        let new_expl_error = iteration.variable::<((Loan,Point),())>("new_expl_error");
        let expl_subset = iteration.variable::<(Region, Region, Point)>("expl_subset");
        let new_expl_subset = iteration.variable::<((Region, Region, Point),())>("new_expl_subset");
        let expl_requires = iteration.variable::<(Region, Loan, Point)>("expl_requires");
        let expl_borrow_live_at = iteration.variable::<(Loan, Point)>("expl_borrow_live_at");

        let expl_borrow_live_at_1 = iteration.variable_indistinct("expl_borrow_live_at_1");
        let expl_borrow_live_at_p = iteration.variable_indistinct("expl_borrow_live_at_p");
        let region_live_at_p = iteration.variable_indistinct("region_live_at_p");

        let expl_outlives = iteration.variable("expl_outlives");


        let expl_error_vec = vec![error_fact];

        expl_error.insert(Relation::from(expl_error_vec.iter().flat_map(
            |(point, loans)| loans.iter().map(move |&loan|  (loan, *point))
        )));

        outlives.insert(all_facts.outlives.clone().into());
        requires.insert(all_facts.borrow_region.clone().into());
        region_live_at.insert(Relation::from(
            all_facts.region_live_at.iter().map(|&(r, p)| ((r, p), ())),
        ));
        invalidates.insert(Relation::from(
            all_facts.invalidates.iter().map(|&(p, b)| ((b, p), ())),
        ));
        cfg_edge_p.insert(all_facts.cfg_edge.clone().into());

        subset.insert(Relation::from(
            output.subset.iter().flat_map(
                |(&point, region_map)|
                    region_map.iter().flat_map(
                        move |(&region, regions)|
                            regions.iter().map(move |&region2| (region, region2, point))
                    )
            )
        ));

        borrow_live_at.insert(Relation::from(
            output.borrow_live_at.iter().flat_map(
                |(&point, loans)|
                    loans.iter().map(move |&loan| (loan, point))

            )
        ));

        requires.insert(Relation::from(
            output.restricts.iter().flat_map(
                |(&point, region_map)|
                    region_map.iter().flat_map(
                        move |(&region, loans)|
                            loans.iter().map(move |&loan| (region, loan, point))
                    )
            )
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


    result

}




struct MirInfoPrinter<'a, 'tcx: 'a> {
    pub tcx: TyCtxt<'a, 'tcx, 'tcx>,
    pub mir: cell::Ref<'a, mir::Mir<'tcx>>,
    pub borrowck_in_facts: facts::AllInputFacts,
    pub borrowck_out_facts: facts::AllOutputFacts,
    pub interner: facts::Interner,
	pub variable_regions: HashMap<mir::Local, facts::Region>,
    pub def_path: rustc::hir::map::DefPath,
}


impl<'a, 'tcx> MirInfoPrinter<'a, 'tcx> {
    pub fn print_info(&mut self) -> Result<(), io::Error> {
        self.print_error();
        Ok(())
    }

    fn print_error(&self) {
        let expl_graph_path = PathBuf::from("nll-facts")
            .join(self.def_path.to_filename_friendly_no_crate())
            .join("outlive_graph.dot");
        let mut outlive_graph = File::create(expl_graph_path).expect("Unable to create file");

        writeln!(outlive_graph, "digraph G {{");

        let mut expl_output = ExplOutput::new();

        for (point, loans) in self.borrowck_out_facts.errors.iter() {
            let err_point_ind = point;
            let err_loans = loans;

            expl_output = compute_error_expl(&self.borrowck_in_facts, &self.borrowck_out_facts, (*err_point_ind, err_loans.clone()));
        }
        //println!("test: {:?}", expl_output.expl_outlives);

        let mut outlives_at: FxHashMap<(facts::Region, facts::Region), Vec<facts::PointIndex>>;
        let mut outlives_debug = Vec::new();
        outlives_at = FxHashMap::default();
        for (point, region_map) in expl_output.expl_outlives {
            //println!("test: {:?}", point);
            for (region, regions) in region_map {
                for region2 in regions {
                    //println!("{:?} -> {:?} [LABEL=\"{:?}\"]", region, region2, point);
                    outlives_at.entry((region, region2)).or_insert(Vec::new()).push(point);
                    outlives_debug.push((region,region2,point));
                }
            }
        }

        let mut debug_facts = self.borrowck_in_facts.clone();

        debug_facts.outlives = outlives_debug;

        let output = Output::compute(&debug_facts, Algorithm::Naive, false);

        println!("debug_errors: {:?}", output.errors);




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
            let mut local_source2;
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
                                    match l {
                                        mir::Place::Local(v) => if v==local_x{
                                            local_source1 = stmt.source_info.span;
                                        }

                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    fm_ln1 = self.tcx.sess.codemap().lookup_line(local_source1.lo()).unwrap();
                    local_source1_snip = fm_ln1.fm.get_line(fm_ln1.line).unwrap().to_string();
                    //local_source1_snip = self.tcx.sess.codemap().get_source_file(file_name).unwrap().get_line(local_source1_line).unwrap();
                    //local_source1_snip = self.tcx.sess.codemap().span_to_snippet(local_source1).ok().unwrap();
                }
                else if *region2 == rv {
                    let local_decl = &self.mir.local_decls[*local_x];
                    if local_decl.name != None {
                        local_name2 = local_decl.name.unwrap().to_string();
                    } else {
                        local_name2 = ("anonymous Variable").to_string();
                    }
                    local_source2 = local_decl.source_info.span;
                    fm_ln2 = self.tcx.sess.codemap().lookup_line(local_source2.lo()).unwrap();
                    local_source2_snip = fm_ln2.fm.get_line(fm_ln2.line).unwrap().to_string();
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
                let point_line = self.tcx.sess.codemap().lookup_char_pos_adj(point_span.lo()).line;
                if point_line < ind {
                    ind = point_line;
                    point_ln = self.tcx.sess.codemap().lookup_line(point_span.lo()).unwrap();
                    point_snip = point_ln.fm.get_line(ind-1).unwrap().to_string();
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
}


