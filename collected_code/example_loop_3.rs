struct List {
    next: Box<Option<List>>,
}

fn main () {
    
}

fn list_add_last(mut l: List) {
    let mut cur = &mut l;

    loop {
        // this is somewhat erroneous, but leads to an interesting error as well.
        cur = &mut l.next.unwrap();
    }
}

fn take<T>(_s: T) { unimplemented!() }
