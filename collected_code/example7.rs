#![feature(nll)]

fn foo<'a>() -> Vec<&'a i32> {
  let mut x = 22;
  let mut y = 42;
  let mut v = vec![];
  let p = &x;
  let q = &y;
  v.push(p);
  v.push(q);
  x += 1;
  v
}

fn main() {
    let a = foo();
}
