//! Every shipped example passes `cairn check` with no finding. The sweep reads
//! `examples/` rather than naming files, so a new example is covered the moment
//! it lands, and `common::examples` refuses a set too small to be the shipped
//! one.

use crate::common::{diagnose, examples};

#[test]
fn every_shipped_example_is_clean() {
    for (name, source) in examples() {
        let diagnostics = diagnose(&source);
        assert!(
            diagnostics.is_empty(),
            "example `{name}` must lint clean, got: {diagnostics:#?}",
        );
    }
}
