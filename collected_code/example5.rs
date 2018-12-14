#![feature(nll)]

fn main() {
    let mut x = 4;
    let mut y = 7;
    let z = foo(&x,&y);
    x = 5;
    take(z);
}
fn foo<'a,'b,'c>(x:&'a i32, y:&'b i32)-> &'c i32 where 'a:'b, 'b:'c{
    x
}
fn take<T>(p: T) { unimplemented!() }
