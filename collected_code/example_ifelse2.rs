#![feature(rustc_private)]
extern crate rand;

struct Strc {
    x: i32,
}

fn main () {
    let mut x = Strc{x: 42};
    let mut other = Strc{x: 43};

    let x3;

    if rand::random() {
        let mut x1 = &mut x;
        let mut x2 = &mut x1;
        x3 = &mut x2;

            // ...
        take(x);
        //...
        take(x3);
    } else {
        let mut x1 = &mut other;
        let mut x2 = &mut x1;
        x3 = &mut x2;
    };
}

fn take<T>(_s: T) { unimplemented!() }
