
// This example was taken from the "Rust Compiler Error Index", as this is an example for a certain error index that I
// think it is worth testing the tool for. (Remember that the tool deals with erroneous programs, and the error index
// indeed prides examples with errors.) Maybe this example has been modified (or extended). This was sometimes needed
// in order to make sure that there also is an error when using Rust edition 2018 with NLL. (The index sometimes
// presents examples that are only erroneous when using edition 2015 with lexical lifetimes)
//
// This example is specific for error E0502. (example 0)
// NOTE: The original example was slightly extended (add take function and call it in the end) in order to ensure that
// the desired error does still occur, even when using NLL. (Rust edition 2018)
// NOTE: All methods were moved out of the main function to make this example working with a previous version (before 
// 22.06.2019) of our tool that apparently did not handle inner methods at all.
// In addition, it was extended to increase it's level of complexity, and to test a `ref mut` "variable (let statement)
// modifier" combination (`mut ref` is not allowed by the compiler)
// NOTE: Therefore, this example now contains an error E0499, and no longer an E0502

#![allow(unused)]
fn main() {

}

fn bar(x: &mut i32) {}
fn take<T>(t: T) { unimplemented!() }
fn foo(mut a: &mut i32) {
    let ref mut y = a;
    let x = y;
    let z = x;
    bar(a); // error: cannot borrow `*a` as mutable because `a` is also borrowed
            //        as immutable and the imutable borrow lives longer
    take(z);
}
