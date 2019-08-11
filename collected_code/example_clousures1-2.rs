// This is a self-written example with a function takes some vale as argument that is then captured in a closure.
// Since the captured value is then reassigned before invoking the created closure, an error E0506 occurs.
// It was then extended with some levels of indirection to increase the complexity and provide Rust Life with things it
// can analyse.

fn main () {
    let mut a: usize = 5;
    let b = &a;
    let c = b;
    let cl =  create_closure_capturing(c);

    // "correct" the value of a to ensure that the right value is printed.
    a = 42;
    cl();
}

fn create_closure_capturing<'a, 'b>(arg: &'a usize) -> Box<FnOnce() -> () + 'b>
    where 'a: 'b {
    Box::new(move || {
        println!("{}", arg);
    })
}

fn take<T>(_s: T) { unimplemented!() }
