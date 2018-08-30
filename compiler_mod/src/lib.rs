#![warn(missing_docs)]
#![feature(nll)]
#![feature(rustc_private)]
#![feature(box_syntax)]
#![feature(box_patterns)]
#![feature(try_from)]
#![feature(crate_in_paths)]

extern crate csv;
extern crate datafrog;
#[macro_use]
extern crate log;
extern crate polonius;
extern crate polonius_engine;
extern crate regex;
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_mir;
extern crate rustc_data_structures;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate syntax;
extern crate syntax_pos;

//pub mod environment;
//pub mod verifier;
//pub mod data;
//pub mod constants;
//pub mod specifications;
//pub mod utils;
//pub mod report;
