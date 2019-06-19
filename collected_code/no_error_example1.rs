fn main() {
    let mut x = 42; // x is mutable ...
    x = 12; // ... so we can change it.
    let x1 = &mut x; // we create a reference, x gets mutable borrowed.
    // let x2 = &mut x; // ERROR: cannot mutable borrow x again here.
    // println!("{}", x); // ERROR: cannot access x here, as it is borrowed to x1.
    *x1 = 42; // we can change the object, as the borrow is mutable.
    println!("{}", x1); // however, we can use x1, as it is still active here.
    
    let y = 42; // y is immutable.
    // y = 43; // ERROR: so we cannot assign to y.
    let y1 = &y; // we can create a (immutable) borrow of it.
    let y2 = &y; // we can also create another (immutable) borrow.
    println!("{}", y); // we can still access (only read) y.
    println!("{}", y1); // we can still access (only read) y1.
    println!("{}", y2); // also y2 is still active here, so we can read it.
}

