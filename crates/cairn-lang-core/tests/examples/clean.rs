//! Every shipped example passes `cairn check` with no finding.

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
