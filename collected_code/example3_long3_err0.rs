// A long, complex example with a lot of indirections and also some lines that are not involved in the actual error.
// Note that this example reveals an error in the Rust Life output. More precisely, Rust Life also reports some lines
// that are not directly of interest, but also interact with variables that are of interest.

#![feature(nll)]

fn main() {
    let mut x = 4;
    let y = &x;
    let d = &x;
    let y2 = || {
        println!("{}", y);
    };
    let y3 = y2;
    let e = &d;
    let mut g = 5;
    let z = bar(&y3);
    let f = &mut g;
    let w = foobar(&z);
    let mut a = 32;
    let b = 42;

    let s = &w;
    let r = foo(&s);

    x = 5;
    *f = 42;
    take(g);
    take(w);
}

fn foo<T>(p: T) -> T { p}

fn bar<T>(p: T) ->T { p}

fn foobar<T>(p: T) ->T { p}

fn take<T>(p: T) { unimplemented!() }
