// This is a self-written example with a function takes some vale as argument that is then captured in a closure.
// Since the captured value is then reassigned before invoking the created closure, an error E0506 occurs.
// It was then extended with some levels of indirection to increase the complexity and provide Rust Life with things it
// can analyse. Tried to increase it even more, but only leads different error at the same level of indirection, which
// makes sense.

fn main () {
    let mut a: usize = 5;
    let b = &a;
    let c = b;
    let cl =  create_closure_capturing(c);

    // now, the error is here.
    let y = &mut a;
    let x = y;
    // "correct" the value of a (by using the reference x) to ensure that the right value is printed.
    *x = 42;
    cl();
}

fn create_closure_capturing<'a, 'b>(arg: &'a usize) -> Box<FnOnce() -> () + 'b>
    where 'a: 'b {
    Box::new(move || {
        println!("{}", arg);
    })
}

fn take<T>(_s: T) { unimplemented!() }
