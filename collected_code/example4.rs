struct StructC<'tcx> {
    bla: &'tcx i32
}

impl<'tcx> StructC<'tcx> {
    fn new(x: &StructC<'tcx>) -> Self {
        unimplemented!()
    }
}

struct StructE<'tcx> {
    field_d: StructC<'tcx>,
}

impl<'tcx> StructE<'tcx> {
    fn get_b(&self) -> &StructC {
        unimplemented!()
    }
}

fn compute<'tcx: 'a, 'a>(
    this: &'a StructE<'tcx>
) -> StructC<'tcx>
    where 'tcx: 'a
{
    let definitely_initalised_paths = this.get_b();

    StructC::new(
        &definitely_initalised_paths
    )
}

fn main() {}
