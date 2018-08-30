use super::borrowck::{facts, regions};
use super::loops;
use super::mir_analyses::initialization::{
    compute_definitely_initialized,
    DefinitelyInitializedAnalysisResult,
    PlaceSet,
};
use crate::utils;
use datafrog::{Iteration, Relation};
use std::{cell, fmt};
use std::env;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Write, BufWriter};
use std::path::PathBuf;
use polonius_engine::{Algorithm, Output};
use rustc::hir::{self, intravisit};
use rustc::mir;
use rustc::ty::TyCtxt;
use rustc_data_structures::indexed_vec::Idx;
use syntax::ast;
use syntax::codemap::Span;


pub fn dump_borrowck_info<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>) {
    trace!("[dump_borrowck_info] enter");

    assert!(tcx.use_mir_borrowck(), "NLL is not enabled.");

    let mut printer = InfoPrinter {
        tcx: tcx,
    };
    intravisit::walk_crate(&mut printer, tcx.hir.krate());

    trace!("[dump_borrowck_info] exit");
}

struct InfoPrinter<'a, 'tcx: 'a> {
    pub tcx: TyCtxt<'a, 'tcx, 'tcx>,
}

impl<'a, 'tcx> intravisit::Visitor<'tcx> for InfoPrinter<'a, 'tcx> {
    fn nested_visit_map<'this>(&'this mut self) -> intravisit::NestedVisitorMap<'this, 'tcx> {
        let map = &self.tcx.hir;
        intravisit::NestedVisitorMap::All(map)
    }

    fn visit_fn(&mut self, fk: intravisit::FnKind<'tcx>, _fd: &'tcx hir::FnDecl,
                _b: hir::BodyId, _s: Span, node_id: ast::NodeId) {
        let name = match fk {
            intravisit::FnKind::ItemFn(name, ..) => name,
            _ => return,
        };

        trace!("[visit_fn] enter name={:?}", name);

        match env::var_os("PRUSTI_DUMP_PROC").and_then(|value| value.into_string().ok()) {
            Some(value) => {
                if name != value {
                    return;
                }
            },
            _ => {},
        };

        let def_id = self.tcx.hir.local_def_id(node_id);
        self.tcx.mir_borrowck(def_id);

        // Read Polonius facts.
        let def_path = self.tcx.hir.def_path(def_id);
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
        let additional_facts = compute_additional_facts(&all_facts, &output);

        let mir = self.tcx.mir_validated(def_id).borrow();
        let loop_info = loops::ProcedureLoops::new(&mir);

        let graph_path = PathBuf::from("nll-facts")
            .join(def_path.to_filename_friendly_no_crate())
            .join("graph.dot");
        let graph_file = File::create(graph_path).expect("Unable to create file");
        let graph = BufWriter::new(graph_file);

        let interner = facts_loader.interner;
        let loan_position = all_facts.borrow_region
            .iter()
            .map(|&(_, loan, point_index)| {
                let point = interner.get_point(point_index);
                (loan, point.location)
            })
            .collect();

        let initialization = compute_definitely_initialized(&mir, self.tcx, def_path);

        let mut mir_info_printer = MirInfoPrinter {
            tcx: self.tcx,
            mir: mir,
            borrowck_in_facts: all_facts,
            borrowck_out_facts: output,
            additional_facts: additional_facts,
            interner: interner,
            graph: cell::RefCell::new(graph),
            loops: loop_info,
            variable_regions: variable_regions,
            loan_position: loan_position,
            initialization: initialization,
        };
        mir_info_printer.print_info();

        trace!("[visit_fn] exit");
    }
}


/// Additional facts derived from the borrow checker facts.
struct AdditionalFacts {
    /// A list of loans sorted by id.
    pub loans: Vec<facts::Loan>,
    /// The ``reborrows`` facts are needed for removing “fake” loans: at
    /// a specific program point there are often more than one loan active,
    /// but we are interested in only one of them, which is the original one.
    /// Therefore, we find all loans that are reborrows of the original loan
    /// and remove them. Reborrowing is defined as follows:
    ///
    /// ```datalog
    /// reborrows(Loan, Loan);
    /// reborrows(L1, L2) :-
    ///     borrow_region(R, L1, P),
    ///     restricts(R, P, L2).
    /// reborrows(L1, L3) :-
    ///     reborrows(L1, L2),
    ///     reborrows(L2, L3).
    /// ```
    pub reborrows: Vec<(facts::Loan, facts::Loan)>,
}

/// Derive additional facts from the borrow checker facts.
fn compute_additional_facts(all_facts: &facts::AllInputFacts,
                            output: &facts::AllOutputFacts) -> AdditionalFacts {

    use self::facts::{PointIndex as Point, Loan, Region};

    let mut iteration = Iteration::new();

    // Variables that are outputs of our computation.
    let reborrows = iteration.variable::<(Loan, Loan)>("reborrows");

    // Variables for initial data.
    let restricts = iteration.variable::<((Point, Region), Loan)>("restricts");
    let borrow_region = iteration.variable::<((Point, Region), Loan)>("borrow_region");

    // Load initial data.
    restricts.insert(Relation::from(
        output.restricts.iter().flat_map(
            |(&point, region_map)|
            region_map.iter().flat_map(
                move |(&region, loans)|
                loans.iter().map(move |&loan| ((point, region), loan))
            )
        )
    ));
    borrow_region.insert(Relation::from(
        all_facts.borrow_region.iter().map(|&(r, l, p)| ((p, r), l))
    ));

    // Temporaries for performing join.
    let reborrows_1 = iteration.variable_indistinct("reborrows_1");
    let reborrows_2 = iteration.variable_indistinct("reborrows_2");

    while iteration.changed() {

        // reborrows(L1, L2) :-
        //   borrow_region(R, L1, P),
        //   restricts(R, P, L2).
        reborrows.from_join(&borrow_region, &restricts, |_, &l1, &l2| (l1, l2));

        // Compute transitive closure of reborrows:
        // reborrows(L1, L3) :-
        //   reborrows(L1, L2),
        //   reborrows(L2, L3).
        reborrows_1.from_map(&reborrows, |&(l1, l2)| (l2, l1));
        reborrows_2.from_map(&reborrows, |&(l2, l3)| (l2, l3));
        reborrows.from_join(&reborrows_1, &reborrows_2, |_, &l1, &l3| (l1, l3));
    }

    // Remove reflexive edges.
    let reborrows: Vec<_> = reborrows
        .complete()
        .iter()
        .filter(|(l1, l2)| l1 != l2)
        .cloned()
        .collect();
    // Compute the sorted list of all loans.
    let mut loans: Vec<_> = all_facts
        .borrow_region
        .iter()
        .map(|&(_, l, _)| l)
        .collect();
    loans.sort();
    AdditionalFacts {
        loans: loans,
        reborrows: reborrows,
    }
}

struct MirInfoPrinter<'a, 'tcx: 'a> {
    pub tcx: TyCtxt<'a, 'tcx, 'tcx>,
    pub mir: cell::Ref<'a, mir::Mir<'tcx>>,
    pub borrowck_in_facts: facts::AllInputFacts,
    pub borrowck_out_facts: facts::AllOutputFacts,
    pub additional_facts: AdditionalFacts,
    pub interner: facts::Interner,
    pub graph: cell::RefCell<BufWriter<File>>,
    pub loops: loops::ProcedureLoops,
    pub variable_regions: HashMap<mir::Local, facts::Region>,
    /// Position at which a specific loan was created.
    pub loan_position: HashMap<facts::Loan, mir::Location>,
    pub initialization: DefinitelyInitializedAnalysisResult<'tcx>,
}


pub fn main() {
    set_var("POLONIUS_ALGORITHM", "Naive");
    let mut args: Vec<String> = std::env::args().collect();
    args.push("-Zborrowck=mir".to_owned());
    args.push("-Ztwo-phase-borrows".to_owned());
    args.push("-Zpolonius".to_owned());
    args.push("-Znll-facts".to_owned());
    args.push("-Zidentify-regions".to_owned());
    args.push("-Zdump-mir=all".to_owned());
    args.push("-Zdump-mir-dir=log/mir/".to_owned());
}
