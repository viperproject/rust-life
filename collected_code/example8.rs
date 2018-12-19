#![feature(nll)]

#[derive(Debug)]
struct B<'a> {
    b: &'a i32
}

#[derive(Debug)]
struct A<'a> {
    one: B<'a>
}

impl<'a> A<'a> {
    fn new() -> A<'a> {
        // let mut b = 10i32;
        A {
            one: B{b: &mut 10i32}
        }
    }
}

fn main() {
    let a = A::new();
    println!("A -> {:?}", a);
}
