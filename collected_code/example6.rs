#![feature(nll)]

fn main() {
    let mut x = 4;
    let y = &x;
    x = 5;
    take(y);
}

fn take<T>(p: T) { unimplemented!() }
