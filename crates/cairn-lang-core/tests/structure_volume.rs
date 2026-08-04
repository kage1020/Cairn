//! The derived-extent bound, at its boundary and in the pipeline.
//!
//! `cairn-lang-cli/tests/cli_hostile_dimensions.rs` proves the process no
//! longer crashes or hangs on the shapes that reach this bound, which needs
//! a subprocess with a deadline to observe. These pin where the bound sits
//! and that crossing it is reported rather than silently absorbed.

use cairn_lang_core::block_array::{Dims, MAX_STRUCTURE_VOLUME, lower_to_block_array};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{lower, parse};

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
    // `size=4294967295x4294967295` reaches this. `checked_volume` is what
    // makes it a refusal: the plain multiplication panics in a debug build
    // and wraps in a release one, and a wrapped volume disagrees with
    // `index`, which turns the mismatch into an out-of-bounds write rather
    // than an error.
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

fn lower_source(source: &str) -> cairn_lang_core::block_array::BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n\n";

#[test]
fn a_scope_past_the_bound_is_skipped_and_named() {
    // `size=` and `height=` are each a perfectly ordinary `u32`; only their
    // product is out of reach, which is why no per-key range check catches
    // this and the extent has to be checked where it is derived.
    let source = format!("{THEME}struct huge size=100000x100000\n  walls mat_slot=wall height=3\n");
    let out = lower_source(&source);
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
        .find(|d| d.code.as_str() == "W_STRUCTURE_TOO_LARGE")
        .expect("the diagnostic")
        .primary;
    assert!(
        message.contains("huge") && message.contains(&MAX_STRUCTURE_VOLUME.to_string()),
        "the message must name the scope and the bound so the author can act; got {message}",
    );
}

#[test]
fn an_ordinary_scope_is_untouched_by_the_bound() {
    // The guard must not fire on anything anyone would write. This extent
    // is four orders of magnitude inside the bound and still far larger
    // than any shipped example.
    let source = format!("{THEME}struct roomy size=64x64\n  walls mat_slot=wall height=64\n");
    let out = lower_source(&source);
    let built = out
        .structures
        .get("struct::roomy")
        .expect("an ordinary struct still lowers");
    assert!(built.dims.fits_volume_budget());
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "W_STRUCTURE_TOO_LARGE"),
    );
}

#[test]
fn an_out_of_range_overhang_is_reported_rather_than_shrinking_the_roof() {
    // `overhang=` is read in exactly one place, so an unusable value had
    // nowhere else to surface: it was treated as absent and the roof quietly
    // came back to the wall line.
    let source = format!(
        "{THEME}struct t size=9x7\n  walls mat_slot=wall height=3\n\
         \x20\x20roof kind=flat mat_slot=wall overhang=4294967296\n"
    );
    let out = lower_source(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| { d.code.as_str() == "W_DEFERRED_MEMBER" && d.primary.contains("overhang=") }),
        "an unusable `overhang=` must be named; got {:?}",
        out.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.primary.as_str()))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn an_out_of_range_height_is_reported_rather_than_saturating() {
    // Saturating put the wall top at `u32::MAX` because the author asked for
    // `2^33`, which is the outcome `nonneg_int`'s own doc gives as the
    // reason it refuses instead of clamping.
    let source = format!("{THEME}struct t size=3x3\n  walls mat_slot=wall height=5000000000\n");
    let out = lower_source(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "W_DEFERRED_MEMBER"),
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
