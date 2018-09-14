#![feature(nll)]

fn main() {
  let mut x: i32 = 22;
  let mut v: Vec<&i32> = vec![];
  let r: &mut Vec<&i32> = &mut v;
  let p: &i32 = &x;
  r.push(p);
  x += 1;
  take(v);
}

fn take<T>(p: T) { unimplemented!() }


