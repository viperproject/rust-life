// © 2020, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

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
use std::collections::{HashMap};
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use self::polonius_engine::{Algorithm, Output};
use rustc::hir::{self, intravisit};
use rustc::mir;
use rustc::ty::TyCtxt;
use self::rustc_data_structures::fx::FxHashMap;
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

        // This was disabled before (instead, an "older" version of mir was read from tcx before), but was re-enabled now to keep the old code work in a consistent way for now.
        let mir = self.tcx.mir_validated(def_id).borrow();

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

        let regions_life_at_error: Vec<Region> = self.all_facts.region_live_at.iter().filter(|&(_r, p)|
            *p == self.error_fact.0
        ).map(|&(r, _p)| r).collect();

        debug!("regions_life_at_error: {:?}", regions_life_at_error);

        //NOTE It might be possible to simplify this, making the next step superfluous, as we already get a loan form the error in error_fact.

        let loans_invalidated_by_error: Vec<Loan> = self.all_facts.invalidates.iter().filter(|&(p, _l)|
            *p == self.error_fact.0
        ).map(|&(_p, l)| l).collect();

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

    /// finds all regions in the outlives (available as field of the struct) that are directly
    /// before the region given as start.
    fn find_prev_regions(&self, start: Region)
                         -> Vec<Region> {
        self.outlives.iter().filter(|&(_, r2, _)|
            *r2 == start
        ).map(|&(r1, _, _)| r1).collect()
    }

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

        if self.all_facts.borrow_region.iter().filter(|&(r, l, _)|
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
        let mut path_to_explain_last_error: Vec<Region> = Vec::default();

        for (point, loans) in self.borrowck_out_facts.errors.iter() {
            let err_point_ind = point;
            let err_loans = loans;

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

            let mut enriched_graph_to_explain_last_error =
                self.create_enriched_graph(&graph_to_explain_last_error, &self.borrowck_in_facts.borrow_region);

            // These lines can be uncommented for debugging purposes
//            let error_graph_path = PathBuf::from("nll-facts")
//                .join(self.def_path.to_filename_friendly_no_crate())
//                .join("error_graph.dot");
//
//            self.print_outlive_error_graph(&enriched_graph_to_explain_last_error, &error_graph_path);

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
                .join("error_graph.json");

            self.dump_outlive_error_graph_as_json(&enriched_graph_to_explain_last_error, &error_graph_path_json);

        }

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
            let (line_number1, local_name1, local_source1_snip) = &error_graph.locals_info_for_regions[region1];
            let (line_number2, local_name2, local_source2_snip) = &error_graph.locals_info_for_regions[region2];

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
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}: {}</td></tr>{}</table>> ]", region1, region1, local_name1, region1, line_number1, local_source1_snip.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"), region1_lines_str);
            }else {
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr>{}</table>> ]", region1, region1, local_name1, region1, region1_lines_str
                );
            }
            if *local_source2_snip != String::default(){
                writeln!(graph_file, "{:?} [ shape=plaintext, color=blue, label =  <<table><tr><td>Lifetime {:?}</td></tr><tr><td>{}: &amp;'{:?}</td></tr><tr><td>{}: {}</td></tr>{}</table>> ]", region2, region2, local_name2, region2, line_number2, local_source2_snip.trim().replace("&","&amp;").replace("<", "&lt;").replace(">", "&gt;"), region2_lines_str);
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
        let out_file = File::create(graph_out_path).expect("Unable to create file");
        let res = serde_json::to_writer_pretty(out_file, error_graph);
        // TODO ev. remove pretty when done with debugging!
        debug!("Result from dumping: {:?}", res);
    }

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
                let (local_decl, line_number, local_name, local_src) = self.find_local_for_region(r1);
                locals_mir_for_regions.insert(*r1, local_decl);
                locals_info_for_regions.insert(*r1, (line_number, local_name, local_src));
            }
            if ! locals_info_for_regions.contains_key(r2) {
                let (local_decl, line_number, local_name, local_src) = self.find_local_for_region(r2);
                locals_mir_for_regions.insert(*r2, local_decl);
                locals_info_for_regions.insert(*r2, (line_number, local_name, local_src));
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
    /// It will return four elements: The first is an option: If the local was found, this is
    /// Option(<MIR structure for the local>), otherwise it will be None.
    /// The second value is the line number (indexed from 1, i.e link when counting lines in an
    /// editor etc.) where the local was found, or usize::default() if it was not found.
    /// Last, there are two Strings. The first is the name of the local, and the second the source
    /// code (text) that introduced this local and hence it's connection to the region.
    /// If the found local has no name, the text "anonymous variable" is returned instead.
    /// If the mapping to a local fails, an empty string is returned as name and as source, and an
    /// message informing about this is logged at debug level. In addition, in this case, or when
    /// the mapping to a source code snipped fails, an empty string will be returned for the source
    /// code as well.
    fn find_local_for_region(&self, reg: &Region) -> (Option<mir::LocalDecl>, usize, String, String) {
        let mut local_name = String::default();
        let mut local_source = syntax_pos::DUMMY_SP;
        let mut line_number = usize::default();
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
                        if let mir::StatementKind::Assign(ref l, ref _r) = stmt.kind{
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
            line_number = fm_ln1.line + 1;
            local_source_snip = fm_ln1.sf.get_line(fm_ln1.line).unwrap().to_string();
            local_decl_option = Some(local_decl.clone());
        } else {
            debug!("No locale (and hence no extra details) found for region={:?}", reg);
        }
        (local_decl_option, line_number, local_name, local_source_snip)
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
    /// If the local is found, the first element shall be the line number where this local is
    /// defined. (Indexed from 1, i.e. like counting lines in an editor) The second element is the
    /// name of the local (or something like "anonymous variable" if it has no name), and the third
    /// element is intended to be the source line that introduced this local and hence the region.
    /// (both as text/String)
    /// The number shall be usize::default() and the Strings shall be empty if the information was
    /// not found for an edge. (This is certainly the case if the corresponding entry in
    /// locals_mir_for_regions is None)
    /// This map will be included in a JSON dump of this structure.
    locals_info_for_regions: FxHashMap<Region, (usize, String, String)>,
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
