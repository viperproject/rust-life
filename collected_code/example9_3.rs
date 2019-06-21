//https://stackoverflow.com/questions/30435152/why-doesnt-my-struct-live-long-enough/30435544#30435544
// modified (twice) to fix one of the tow error and only keep the one that is relevant for testing our tool.
// (yes, this is somehow quite a different example now, but it now does no longer contain two errors, but only one, and
// this is what we need for testing for now, esp. as this error is part of a member function (method) of a struct
// impl block.)

#![feature(nll)]

struct MyStruct<'a>
{
    v : Vec<Box<i32>>,
    p : &'a i32
}

fn take<T>(_t: T) { unimplemented!() }

impl<'a> MyStruct<'a>
{
    fn new(arg : &'a i32)
    {
        let initial = vec![Box::new(1), Box::new(2)];
        let mut mystruct = MyStruct { v : initial, p : &arg };

        mystruct.update();

        take(mystruct);
    }

    fn update(&'a mut self)
    {
        self.p = &self.v.last().unwrap();
    }

}

fn main() {
    let x = 5;
    MyStruct::new(&x);
}
