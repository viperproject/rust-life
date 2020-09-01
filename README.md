# Rust Life

> Simple explanations for some complex Rust lifetime errors.

This repository contains the prototype of a custom Rust compiler driver (`compiler_mod`) and of a VS Code IDE extensions (`rust-life-assistant`) that together provide simple explanations for some complex Rust lifetime errors.

Reports:
* Dominic Dietler, "Visualization of Reference Lifetimes in Rust", AS 2018: [description](https://ethz.ch/content/dam/ethz/special-interest/infk/chair-program-method/pm/documents/Education/Theses/David_Blaser_BA_description.pdf), [report](https://ethz.ch/content/dam/ethz/special-interest/infk/chair-program-method/pm/documents/Education/Theses/David_Blaser_BA_Report.pdf).
* David Blaser, "Simple Explanation of Complex Lifetime Errors in Rust", SS 2019: [description](https://ethz.ch/content/dam/ethz/special-interest/infk/chair-program-method/pm/documents/Education/Theses/Dominik_Dietler_BA_description.pdf), [report](https://ethz.ch/content/dam/ethz/special-interest/infk/chair-program-method/pm/documents/Education/Theses/Dominik_Dietler_BA_report.pdf).

## License

Copyright 2020, ETH Zurich

This project is released under the Mozilla Public License, v. 2.0 except for:

* the file `compiler_mod/src/facts.rs`, which is an adaptation of https://github.com/rust-lang/polonius/blob/master/src/facts.rs and is thus released under the Apache License, v. 2.0

* the files in the `collected_code` folder, each of which specify their origin and are thus released under their respective licenses
