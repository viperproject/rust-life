struct Cnt {
    x: i32,
}

impl Cnt {
    fn inc(&mut self) {
        self.x += 1;
    }

    fn chk(&self) -> bool {
        return self.x < 10;
    }
}

fn main () {
    let mut x = Cnt{x: 0};

    let mut x1 = &mut x;
    let mut x2 = &mut x1;
    let x3 = &mut x2;
    
    while x.chk() {
        // ...
        x3.inc();
    }
}

fn take<T>(_s: T) { unimplemented!() }
