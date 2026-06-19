//! Regression: every shipped example must pass `cairn check` clean.
//!
//! If a future change introduces a diagnostic against `cottage.crn` (or any
//! other example) without also updating the example to match the new rule,
//! one of these assertions catches it before release. The four examples are
//! also the `M3 — examples work` gate, so they double as the "what shape of
//! input do we promise to support" surface.

use cairn_lang_core::{check, lower, parse};

fn check_example(filename: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(filename);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let module = parse(&source).unwrap_or_else(|e| panic!("parse {filename}: {e}"));
    let ir = lower(&module);
    let diagnostics = check(&module, &ir);
    assert!(
        diagnostics.is_empty(),
        "example `{filename}` must lint clean, got: {diagnostics:#?}",
    );
}

#[test]
fn cottage_is_clean() {
    check_example("cottage.crn");
}

#[test]
fn themed_tower_is_clean() {
    check_example("themed-tower.crn");
}

#[test]
fn village_is_clean() {
    check_example("village.crn");
}

#[test]
fn redstone_door_is_clean() {
    check_example("redstone-door.crn");
}
