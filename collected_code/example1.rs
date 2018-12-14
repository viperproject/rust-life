#![feature(nll)]

fn main() {
  let mut x = 22;
  let mut y = 42;
  let mut v = vec![];
  let p = &x;
  let q = &y;
  v.push(p);
  v.push(q);
  x += 1;
  take(v);
}

fn take<T>(p: T) { unimplemented!() }
