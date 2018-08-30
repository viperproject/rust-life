//https://stackoverflow.com/questions/30435152/why-doesnt-my-struct-live-long-enough/30435544#30435544

#![feature(nll)]

struct MyStruct<'a>
{
    v : Vec<Box<i32>>,
    p : &'a i32
}

impl<'a> MyStruct<'a>
{
    fn new(arg : &'a i32) -> MyStruct<'a>
    {
        let initial = vec![Box::new(1), Box::new(2)];
        let mystruct = MyStruct { v : initial, p : &arg };

        mystruct.update();

        mystruct
    }

    fn update(&'a mut self)
    {
        self.p = &self.v.last().unwrap();
    }

}

fn main() {
    let x = 5;
    let mut obj = MyStruct::new(&x);
}
