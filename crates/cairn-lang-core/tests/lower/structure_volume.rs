//! The derived-extent bound sits exactly where it is documented, and a source
//! that crosses it is reported rather than silently absorbed.

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{Dims, MAX_STRUCTURE_VOLUME};

use crate::common::lowered;

/// Exactly the bound: a 256-cube.
const EDGE: u32 = 256;

#[test]
fn an_extent_at_the_bound_is_accepted() {
    let dims = Dims {
        x: EDGE,
        y: EDGE,
        z: EDGE,
    };
    assert_eq!(dims.checked_volume(), Some(MAX_STRUCTURE_VOLUME));
    assert!(
        dims.fits_volume_budget(),
        "the bound is the largest legal extent, not the first illegal one",
    );
}

#[test]
fn one_voxel_past_the_bound_is_refused() {
    let dims = Dims {
        x: EDGE,
        y: EDGE,
        z: EDGE + 1,
    };
    assert!(dims.checked_volume().unwrap() > MAX_STRUCTURE_VOLUME);
    assert!(!dims.fits_volume_budget());
}

#[test]
fn an_extent_whose_product_overflows_is_refused_rather_than_wrapping() {
    let dims = Dims {
        x: u32::MAX,
        y: u32::MAX,
        z: u32::MAX,
    };
    assert_eq!(dims.checked_volume(), None);
    assert!(!dims.fits_volume_budget());
    assert_eq!(
        dims.volume(),
        usize::MAX,
        "the saturating reader must not wrap either",
    );
}

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n\n";

#[test]
fn a_scope_past_the_bound_is_skipped_and_named() {
    // Each key is an ordinary `u32`; only their product is out of reach.
    let source = format!("{THEME}struct huge size=100000x100000\n  walls mat_slot=wall height=3\n");
    let out = lowered(&source);
    assert!(
        !out.structures.contains_key("struct::huge"),
        "a scope that cannot be allocated must not appear in the IR",
    );
    let reported: Vec<&str> = out
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .filter(|code| *code == "W_STRUCTURE_TOO_LARGE")
        .collect();
    assert_eq!(
        reported.len(),
        1,
        "the scope must be reported exactly once; got {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
    );
    let message = &out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::StructureTooLarge)
        .expect("the diagnostic")
        .primary;
    assert!(
        message.contains("huge") && message.contains(&MAX_STRUCTURE_VOLUME.to_string()),
        "the message must name the scope and the bound so the author can act; got {message}",
    );
}

#[test]
fn a_place_past_the_bound_is_skipped_and_named() {
    let source = format!(
        "{THEME}def huge size=100000x100000:\n  walls mat_slot=wall height=3\n\n\
         site s:\n  place id=a use=huge theme=t at=origin\n"
    );
    let out = lowered(&source);
    assert!(out.structures.is_empty(), "{:?}", out.structures.keys());
    assert!(out.placements.is_empty(), "{:?}", out.placements.keys());
    let reported: Vec<&str> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::StructureTooLarge)
        .map(|d| d.primary.as_str())
        .collect();
    assert_eq!(reported.len(), 1, "{:#?}", out.diagnostics);
    assert!(
        reported[0].contains(&MAX_STRUCTURE_VOLUME.to_string()),
        "the message must name the bound; got {}",
        reported[0],
    );
}

#[test]
fn an_ordinary_scope_is_untouched_by_the_bound() {
    let source = format!("{THEME}struct roomy size=64x64\n  walls mat_slot=wall height=64\n");
    let out = lowered(&source);
    let built = out
        .structures
        .get("struct::roomy")
        .expect("an ordinary struct still lowers");
    assert!(built.dims.fits_volume_budget());
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::StructureTooLarge),
    );
}

#[test]
fn an_out_of_range_overhang_is_reported_rather_than_shrinking_the_roof() {
    let source = format!(
        "{THEME}struct t size=9x7\n  walls mat_slot=wall height=3\n\
         \x20\x20roof kind=flat mat_slot=wall overhang=4294967296\n"
    );
    let out = lowered(&source);
    assert!(
        out.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::IgnoredArgument && d.primary.contains("overhang=")
        }),
        "an unusable `overhang=` must be named; got {:?}",
        out.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.primary.as_str()))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn an_out_of_range_height_is_reported_rather_than_saturating() {
    let source = format!("{THEME}struct t size=3x3\n  walls mat_slot=wall height=5000000000\n");
    let out = lowered(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidMember),
        "an unusable `height=` must be named; got {:?}",
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
    );
    let built = out
        .structures
        .get("struct::t")
        .expect("the struct itself still lowers, just without the wall");
    assert!(
        built.dims.y < 10,
        "the unusable height must not reach the extent; got {:?}",
        built.dims,
    );
}
