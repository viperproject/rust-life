#![feature(rustc_private)]
extern crate rand;

struct Strc {
    x: i32,
}

fn main () {
    let mut x = Strc{x: 42};
    let mut other = Strc{x: 43};
    let mut x1;
    let mut x2;    

    let x3 = if rand::random() {
        x1 = &mut x;
        x2 = &mut x1;
        &mut x2
    } else {
        &mut other
    };
    // ...
    take(x);
    //...
    take(x3);

}

fn take<T>(_s: T) { unimplemented!() }
