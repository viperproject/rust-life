#![feature(nll)]

fn main() {
    let mut x = 4;
    let y = foo(&x);
    let z = bar(&y);
    let w = foobar(&z );
    let mut a = 32;
    let b = 42;

    let r = w;
    let s = foobar(&w);
    let t = foo(&s);

    x = 5;
    take(w);
}

fn foo<T>(p: T) -> T { p}

fn bar<T>(p: T) ->T { p}

fn foobar<T>(p: T) ->T { p}

fn take<T>(p: T) { unimplemented!() }
