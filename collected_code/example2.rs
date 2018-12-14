#![feature(nll)]

struct A<'a>
{
    x: &'a u32
}

impl<'a> A<'a>
{
    fn foo(&'a mut self)
    {

    }


}


fn main() {
    let mut a: A=A{x:&5};
    a.foo();
    let x = &mut a;
}
