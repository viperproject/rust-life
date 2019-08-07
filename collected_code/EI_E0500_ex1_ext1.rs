

// This example was taken from the "Rust Compiler Error Index", as this is an example for a certain error index that I
// think it is worth testing the tool for. (Remember that the tool deals with erroneous programs, and the error index
// indeed prides examples with errors.) Maybe this example has been modified (or extended). This was sometimes needed
// in order to make sure that there also is an error when using Rust edition 2018 with NLL. (The index sometimes
// presents examples that are only erroneous when using edition 2015 with lexical lifetimes)
//
// This example is specific for error E0500. (example 0)
// NOTE: The original example was slightly extended (add take function and call it in the end) in order to ensure that
// the desired error does still occur, even when using NLL. (Rust edition 2018)
// NOTE: All methods were moved out of the main function to make this example working with a previous version (before 
// 22.06.2019) of our tool that apparently did not handle inner methods at all. (This was fixed now, but the example
// code looks nicer with this change anyway)
// In addition, it was extended to increase it's level of complexity, but (unintentionally) also changed part of the
// error. (But it still is an E0500)

#![allow(unused)]
fn main() {

}

fn take<T>(t: T) { unimplemented!() }
fn you_know_nothing(mut jon_snow: &mut i32) {
    let jon_snow_ref = &mut jon_snow;
    let nights_watch = || {
        *jon_snow = 2;
    };
    let nights_watch_ref = & nights_watch;
    let nights_watch_ref2 = nights_watch_ref;
    let starks = || {
        **jon_snow_ref = 3; // error: closure requires unique access to `jon_snow`
                       //        but it is already and still borrowed
    };
    take(nights_watch_ref2);
}
