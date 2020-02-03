// © 2020, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![feature(box_syntax)]
#![feature(rustc_private)]
#![feature(vec_remove_item)]

extern crate env_logger;
extern crate getopts;
#[macro_use]
extern crate log;
//extern crate prusti;
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_errors;
extern crate rustc_codegen_utils;
extern crate syntax;
extern crate syntax_pos;

//mod driver_utils;
mod dump_borrowck_info;
mod facts;
mod regions;

use std::env::set_var;
use rustc::hir::def_id::DefId;
use rustc_interface::interface;

pub type ProcedureDefId = DefId;

/// Struct holding the compiler callbacks for rust-life.
/// Could be used to store state for callbacks, but is not done for now.
struct RustLifeCallbacks {}

impl RustLifeCallbacks {
    /// Function that creates a RustLifeCallback.
    fn new() -> RustLifeCallbacks {
        RustLifeCallbacks { }
    }
}

impl rustc_driver::Callbacks for RustLifeCallbacks {
    fn after_parsing(&mut self, compiler: &interface::Compiler) -> bool {
        trace!("[RustLifeCallbacks.after_parsing] enter");

        // TODO pass correct args.
        compiler
            .global_ctxt()
            .unwrap()
            .peek_mut()
            .enter(|tcx| dump_borrowck_info::dump_borrowck_info(tcx));
        // Ev. change the called function to take tcx by reference?

        // Stop!
        false
    }

}

pub fn main() {
    env_logger::init();
    trace!("[main] enter");
    set_var("POLONIUS_ALGORITHM", "Naive");
    let mut args: Vec<String> = std::env::args().collect();
    args.push("-Zborrowck=mir".to_owned());
    //args.push("-Ztwo-phase-borrows".to_owned());
    args.push("-Zpolonius".to_owned());
    args.push("-Znll-facts".to_owned());
    args.push("-Zidentify-regions".to_owned());
    args.push("-Zdump-mir=all".to_owned());
    args.push("-Zdump-mir-dir=log/mir/".to_owned());

    let result = rustc_driver::report_ices_to_stderr_if_any(move || {
        rustc_driver::run_compiler(&args, &mut RustLifeCallbacks::new(), None, None)
    }).and_then(|result| result);

    trace!("[main] exit");
    std::process::exit(result.is_err() as i32);
}
