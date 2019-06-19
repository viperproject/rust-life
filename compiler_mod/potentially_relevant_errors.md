# Errors that might be potentially relevant for the rust-live tool

## Found by looking at the (online) Rust Error Indes
The error index can be found at: https://doc.rust-lang.org/error-index.html

E0007

E0382

E0387

E0478

E0491

E0499

E0500 // Not that relevant, seems to (primarily) only affect non-NLL, i.e. Rust 2015 (at least this applies to the example.)

E0501 // Not that relevant, seems to (primarily) only affect non-NLL, i.e. Rust 2015 (at least this applies to the example.)

E0502 // Maybe not with NLL?

E0503 // Maybe not with NLL?

E0504

E0505

E0506

E0507 // not exactly sure if this is a borrow-checker error, at least no three-point error message is used for reporting it.

E0508 // not exactly sure if this is a borrow-checker error, at least no three-point error message is used for reporting it.

E0596

E0597

E0621

E0626

E0712

E0713

E0716

## More, that were found by looking at rust (rustc) source files
Note that this only includes some that were not part of the list before.

### Found in rust/src/librustc/error_codes.rs
E0311 to E0316 (inclusive bounds): From the short description, I would guess that all of these are related to "lifetimes" etc.
None of these are part of the Error Index. (As of today, 13.06.2019, please also note that I looked at the upstream version
of today of the mentioned file.) None of these errors does have a longer explanation in the file that I (first) found them in,
and therefore there ware also no code examples that showed how to trigger such an error.



