// This example was taken from the "Rust Compiler Error Index", as this is an example for a certain error index that I
// think it is worth testing the tool for. (Remember that the tool deals with erroneous programs, and the error index
// indeed prides examples with errors.) Maybe this example has been modified (or extended). This was sometimes needed
// in order to make sure that there also is an error when using Rust edition 2018 with NLL. (The index sometimes
// presents examples that are only erroneous when using edition 2015 with lexical lifetimes)
//
// This example is specific for error E0499. (example 0)
// NOTE: The original example was slightly extended (add take function and call it in the end) in order to ensure that
// the desired error does still occur, even when using NLL. (Rust edition 2018)
// In addition, it was extended to increase it's level of complexity.

#![allow(unused)]
    fn main() {
    fn take<T>(t: T) { unimplemented!() }
    let mut i = 0;
    // none of the following variables actually need to be mutable, only the borrows must be mutable
    let j = &mut i;
    let k = j;
    let x = k;
    let a = &mut i;
    // error: cannot borrow `i` as mutable more than once at a time
    take (x);
}
