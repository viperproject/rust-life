struct Strc {
    x: i32,
}

fn main () {
    let mut x = Strc{x: 42};
    // ...
    let mut x1 = &mut x;
    let mut x2 = &mut x1;
    let x3 = &mut x2;
    // ...
    take(x);
    //...
    take(x3)
}

fn take<T>(_s: T) { unimplemented!() }
