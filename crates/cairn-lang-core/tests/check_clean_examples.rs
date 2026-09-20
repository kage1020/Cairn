//! Regression: every shipped example must pass `cairn check` clean.
//!
//! If a future change introduces a diagnostic against `cottage.crn` (or any
//! other example) without also updating the example to match the new rule,
//! this sweep catches it before release. The shipped examples also define
//! the "what shape of input do we promise to support" surface, so this
//! suite doubles as the contract for that promise.
//!
//! The sweep reads `examples/` rather than naming files, so a new example
//! is covered the moment it lands; the guard in [`common::examples`]
//! refuses a set too small to be the shipped one.

mod common;
use common::{diagnose, examples};

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
