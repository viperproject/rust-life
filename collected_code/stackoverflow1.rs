/**
  * Example with a lifetime error, copied from a question from stackoverflow:
  * https://stackoverflow.com/questions/55678288/why-does-this-cast-to-a-trait-object-seem-to-change-the-borrow-semantics
  * Rust playground: https://play.rust-lang.org/?version=stable&mode=debug&edition=2018&gist=fa07f104331bf154480431cdd61c7459
**/

type LongTraitObjType<'collection, 'data> =
    Box<dyn CreatesIterator<'collection, 'data, IteratorWithRef<'collection, 'data>> + 'data>;

fn main() {
    let data = 1;

    // Works: concrete implementation:
    {
        let creates_iterator_impl = Box::new(CreatesIteratorImpl(vec![Wrapper(&data)]));
        let _ = creates_iterator_impl.iterate().count();
    }

    // Doesn't work: same as above, but cast to a trait object.
    {
        let creates_iterator_dyn: LongTraitObjType =
            Box::new(CreatesIteratorImpl(vec![Wrapper(&data)]));
        let _ = creates_iterator_dyn.iterate().count();
    }
}

#[derive(Clone)]
struct Wrapper<'data>(&'data u32);

struct IteratorWithRef<'collection, 'data: 'collection> {
    reference: &'collection CreatesIteratorImpl<'data>,
    i: usize,
}
impl<'collection, 'data: 'collection> Iterator for IteratorWithRef<'collection, 'data> {
    type Item = Wrapper<'data>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i < self.reference.0.len() {
            let ret = Some(self.reference.0[self.i].clone());
            self.i += 1;
            ret
        } else {
            None
        }
    }
}

trait CreatesIterator<'collection, 'data, E>
where
    'data: 'collection,
    E: Iterator + 'collection,
    <E as Iterator>::Item: 'data,
{
    fn iterate(&'collection self) -> E;
}

struct CreatesIteratorImpl<'data>(Vec<Wrapper<'data>>);

impl<'collection, 'data: 'collection>
    CreatesIterator<'collection, 'data, IteratorWithRef<'collection, 'data>>
    for CreatesIteratorImpl<'data>
{
    fn iterate(&'collection self) -> IteratorWithRef<'collection, 'data> {
        IteratorWithRef {
            reference: self,
            i: 0,
        }
    }
}
