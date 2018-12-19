#![feature(nll)]

fn main(){

    let x;

    {

        let y = 5;

        x = &y;

    }

    let z = *x + 42;

}
