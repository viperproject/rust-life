// This is a self-written example with a very simple (and useless) list, that contains one singe error E0506.
// It was quite hard to get an example that really only contains the one error that we desire, since there are also
// a lot of other things that can go wrong.

struct List {
    next: Option<Box<List>>,
}

fn main () {
    let mut test_list = List{next: Some(Box::new(List{next: None}))};
    list_add_last(test_list);
}

fn list_add_last(mut l: List) {
    let mut cur = &mut l;

    loop {
        let loc_next_ref = &mut cur.next;
        cur = match loc_next_ref {
            Some(nl) => nl,
            None => break,
        };
    }

    cur.next = Some(Box::new(List{next: None}));
}

fn take<T>(_s: T) { unimplemented!() }
