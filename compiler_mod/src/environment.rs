#![feature(try_from)]
#![feature(crate_in_paths)]

use super::dump_borrowck_info;

use rustc_driver::driver;
use rustc::ty::TyCtxt;
use rustc::hir::def_id::DefId;
use std::path::PathBuf;
use syntax_pos::FileName;
use syntax_pos::MultiSpan;
use syntax::errors::DiagnosticId;
use rustc::hir;
use rustc::ty;
use syntax::attr;
use data::ProcedureDefId;

//mod procedure;
//mod loops;
//mod collect_prusti_spec_visitor;

//pub mod borrowck;
//pub mod mir_analyses;

//use data::ProcedureDefId;
pub use procedure::{BasicBlockIndex, Procedure, ProcedureImpl};
pub use loops::{ProcedureLoops, PlaceAccess, PlaceAccessKind};
//use collect_prusti_spec_visitor::CollectPrustiSpecVisitor;
use syntax::codemap::CodeMap;

/// A facade to the Rust compiler.
pub trait Environment<'tcx> {
    /// The concrete type that implements the Procedure interface
    type ProcedureImpl: Procedure<'tcx>;

    /// Get the name of an item
    fn get_item_name(&self, proc_def_id: DefId) -> String;

    /// Get a Procedure.
    fn get_procedure(&self, proc_def_id: ProcedureDefId) -> Self::ProcedureImpl;
}

/// Facade to the Rust compiler.
pub struct EnvironmentImpl<'r, 'a: 'r, 'tcx: 'a> {
    state: &'r mut driver::CompileState<'a, 'tcx>,
}

impl<'r, 'a, 'tcx> EnvironmentImpl<'r, 'a, 'tcx> {
    /// Builds an environment given a compiler state.
    pub fn new(state: &'r mut driver::CompileState<'a, 'tcx>) -> Self {
        EnvironmentImpl { state }
    }

    /// Returns the path of the source that is being compiled
    pub fn source_path(&self) -> PathBuf {
        match driver::source_name(self.state.input) {
            FileName::Real(path) => path,
            _ => unreachable!(),
        }
    }

    /// Returns the typing context
    pub fn tcx(&self) -> TyCtxt<'a, 'tcx, 'tcx> {
        self.state.tcx.unwrap()
    }

    /// Returns the type of a `HirId`
    pub fn hir_id_to_type(&self, hir_id: hir::HirId) -> ty::Ty<'tcx> {
        let owner_def_id = hir_id.owner_def_id();
        let typeck_tables = self.tcx().typeck_tables_of(owner_def_id);
        typeck_tables.node_id_to_type(hir_id)
    }

    /// Returns the `CodeMap`
    pub fn codemap(&self) -> &'tcx CodeMap {
        self.state.session.codemap()
    }

    /// Emits a warning message
    pub fn warn(&self, msg: &str) {
        self.state.session.warn(msg);
    }

    /// Emits an error message.
    pub fn err(&self, msg: &str) {
        self.state.session.err(msg);
    }

    /// Emits an error message.
    pub fn span_err_with_code<S: Into<MultiSpan>>(&self, sp: S, msg: &str, code: String) {
        self.state.session.span_err_with_code(sp, msg, DiagnosticId::Error(code));
    }

    /// Emits an error message.
    pub fn err_with_code(&self, msg: &str, code: String) {
        self.span_err_with_code(MultiSpan::new(), msg, code);
    }

    /// Returns true if an error has been emitted
    pub fn has_errors(&self) -> bool {
        self.state.session.has_errors()
    }

    /// Aborts in case of error.
    pub fn abort_if_errors(&self) {
        self.state.session.abort_if_errors();
    }

    /*
    /// Get ids of Rust procedures that are annotated with a Prusti specification
    pub fn get_annotated_procedures(&self) -> Vec<ProcedureDefId> {
        let mut annotated_procedures: Vec<ProcedureDefId> = vec![];
        let tcx = self.tcx();
        {
            let mut visitor = CollectPrustiSpecVisitor::new(tcx, &mut annotated_procedures);
            tcx.hir.krate().visit_all_item_likes(&mut visitor);
        }
        annotated_procedures
    }
    */

    /// Find whether the procedure has a particular attribute
    pub fn has_attribute_name(&self, def_id: ProcedureDefId, name: &str) -> bool {
        let tcx = self.tcx();
        let opt_node_id = tcx.hir.as_local_node_id(def_id);
        match opt_node_id {
            None => {
                warn!("Incomplete encoding of procedures from an external crate");
                false
            }
            Some(node_id) => {
                attr::contains_name(tcx.hir.attrs(node_id), name)
            }
        }
    }

    /// Dump various information from the borrow checker.
    ///
    /// Mostly used for experiments and debugging.
    pub fn dump_borrowck_info(&self) {
        dump_borrowck_info::dump_borrowck_info(self.tcx())
    }
}

impl<'r, 'a, 'tcx> Environment<'tcx> for EnvironmentImpl<'r, 'a, 'tcx> {
    type ProcedureImpl = ProcedureImpl<'a, 'tcx>;

    fn get_item_name(&self, def_id: DefId) -> String {
        self.tcx().item_path_str(def_id)
    }

    /// Get a Procedure.
    fn get_procedure(&self, proc_def_id: ProcedureDefId) -> ProcedureImpl<'a, 'tcx> {
        ProcedureImpl::new(self.tcx(), proc_def_id)
    }
}

