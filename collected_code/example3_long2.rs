#![feature(nll)]

fn main() {
    let mut x = 4;
    let y = foo(&x);
    let d = &x;
    let e = bar(d);
    let mut g = 5;
    let z = bar(&y);
    let f = &mut g;
    let w = foobar(&z );
    let mut a = 32;
    let b = 42;

    let r = w;
    let s = foobar(&w);
    let t = foo(&s);

    x = 5;
    *f = 42;
    take(g);
    take(w);
}

fn foo<T>(p: T) -> T { p}

fn bar<T>(p: T) ->T { p}

fn foobar<T>(p: T) ->T { p}

fn take<T>(p: T) { unimplemented!() }
