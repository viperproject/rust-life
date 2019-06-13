// This example was taken from the "Rust Compiler Error Index", as this is an example for a certain error index that I
// think it is worth testing the tool for. (Remember that the tool deals with erroneous programs, and the error index
// indeed prides examples with errors.) Maybe this example has been modified (or extended). This was sometimes needed
// in order to make sure that there also is an error when using Rust edition 2018 with NLL. (The index sometimes
// presents examples that are only erroneous when using edition 2015 with lexical lifetimes)
//
// This example is specific for error E0621. (example 0)

#![allow(unused)]
fn main() {
fn foo<'a>(x: &'a i32, y: &i32) -> &'a i32 { // error: explicit lifetime
                                             //        required in the type of
                                             //        `y`
    if x > y { x } else { y }
}
}
