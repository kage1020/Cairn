//! A member that paints nothing shapes nothing (`spec/compilation` "Level grouping and volume
//! derivation"): the volume equals the same source's with its line deleted.

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::BlockArrayIr;

use crate::common::{lowered, notes, slice};

fn dims_of(out: &BlockArrayIr, key: &str) -> (u32, u32, u32) {
    let built = out
        .structures
        .get(key)
        .unwrap_or_else(|| panic!("{key} must lower"));
    (built.dims.x, built.dims.y, built.dims.z)
}

fn codes(out: &BlockArrayIr) -> Vec<&str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The dims of `source`, and of `source` with `line` deleted.
fn with_and_without(source: &str, line: &str, key: &str) -> ((u32, u32, u32), (u32, u32, u32)) {
    assert_eq!(
        source.matches(line).count(),
        1,
        "the fixture must contain the line it deletes exactly once: {line:?}",
    );
    let without = source.replace(line, "");
    (
        dims_of(&lowered(source), key),
        dims_of(&lowered(&without), key),
    )
}

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @oak_stairs\n\n";

// --- A. A roof with no lowering shapes nothing ------------------------------

#[test]
fn a_roof_with_no_kind_does_not_widen_the_footprint() {
    let line = "  roof mat_slot=roof overhang=3\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(
        with, without,
        "a roof that draws nothing must not widen the array",
    );
    assert!(
        codes(&lowered(&source)).contains(&"W_DEFERRED_MEMBER"),
        "the missing `kind=` is still reported: {:?}",
        codes(&lowered(&source)),
    );
}

#[test]
fn a_roof_with_no_kind_and_an_unusable_overhang_reports_both_and_widens_nothing() {
    let line = "  roof mat_slot=roof overhang=nope\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
    let out = lowered(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::DeferredMember && d.primary.contains("kind=")),
        "the missing `kind=` is reported: {:?}",
        codes(&out),
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.primary.contains("overhang=")),
        "and the unusable `overhang=` is too — this is its only reader: {:?}",
        codes(&out),
    );
}

#[test]
fn a_roof_that_draws_still_widens_the_footprint() {
    let line = "  roof kind=gable mat_slot=roof overhang=2\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(
        with.0,
        without.0 + 4,
        "two blocks of overhang on each side of the x axis",
    );
    assert_eq!(with.2, without.2 + 4, "and of the z axis");
    assert!(with.1 > without.1, "and a gable is taller than no roof");
}

#[test]
fn a_shed_with_no_slope_to_does_not_widen_the_footprint() {
    let line = "  roof kind=shed mat_slot=roof overhang=3\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(
        with, without,
        "a shed that draws nothing must not widen the array",
    );
}

#[test]
fn a_shed_with_a_slope_to_still_widens_the_footprint() {
    let line = "  roof kind=shed slope_to=front mat_slot=roof overhang=3\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with.0, without.0 + 6, "three blocks of eave on each side");
    assert!(with.1 > without.1, "and a slope over the wall top");
}

/// A roof with `overhang=2` and no `kind=` draws no overhang for the plate to sit on.
#[test]
fn a_plate_outside_a_wall_beside_a_roof_that_draws_nothing_only_defers() {
    let roof = "roof mat_slot=roof overhang=2";
    let plate = "pressure_plate id=p at=front.outside offset=0 y=1 mat_slot=wall";
    let source =
        format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n  {roof}\n  {plate}\n");
    let out = lowered(&source);
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| (d.code, slice(&source, d)))
            .collect::<Vec<_>>(),
        [
            (DiagnosticCode::DeferredMember, roof),
            (DiagnosticCode::DeferredMember, plate),
        ],
    );
    let found = &out.diagnostics[1];
    assert_eq!(
        (found.primary.as_str(), notes(found)),
        (
            "pressure_plate `at=front.outside` at y=1 has no exterior voxel to sit on",
            vec![
                "a `roof` written with an `overhang=` draws nothing, so the struct has no \
                 overhang to hold the plate; it fits once that roof draws",
            ],
        ),
    );
}

/// Both courses are level-scoped, so the surviving one is not the one at offset zero.
#[test]
fn a_mixed_wall_column_keeps_only_the_courses_that_paint() {
    for (first, second) in [
        ("walls mat_slot=wall height=2", "walls height=2"),
        ("walls height=2", "walls mat_slot=wall height=2"),
    ] {
        let source = format!(
            "{THEME}struct s size=5x5\n  \
             level id=lower y=1\n    {first}\n  \
             level id=upper y=6\n    {second}\n"
        );
        let out = lowered(&source);
        let (_, y, _) = dims_of(&out, "struct::s");
        let painting_is_lower = first.contains("mat_slot=");
        let expected = if painting_is_lower { 1 + 3 } else { 1 + 8 };
        assert_eq!(
            y, expected,
            "only the painting course raises the array (first={first:?})",
        );
    }
}

// --- B. Walls that paint nothing shape nothing ------------------------------

#[test]
fn themeless_walls_do_not_raise_the_array() {
    let line = "  walls mat_slot=wall height=6\n";
    let source = format!("struct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without, "walls that paint nothing raise nothing");
    assert!(
        codes(&lowered(&source)).contains(&"W_NO_THEME_BOUND"),
        "and the themeless scope is still reported",
    );
}

/// `E_MISSING_MATERIAL` is the check pass's, so lowering says nothing.
#[test]
fn walls_with_no_material_binding_do_not_raise_the_array() {
    let line = "  walls height=6\n";
    let source = format!("{THEME}struct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
}

#[test]
fn walls_that_paint_still_raise_the_array() {
    let line = "  walls mat_slot=wall height=6\n";
    let source = format!("{THEME}struct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(
        with.1,
        without.1 + 6,
        "six rows of wall over the base plane",
    );
    assert_eq!((with.0, with.2), (without.0, without.2));
}

#[test]
fn a_window_in_walls_that_paint_nothing_is_deferred_and_the_array_shrinks() {
    let line = "  window side=front y=1 offset=1 size=2x2 mat_slot=wall\n";
    let source = format!("struct s size=5x5\n  walls mat_slot=wall height=4\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without, "the window shapes nothing either");
    let out = lowered(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::DeferredMember),
        "a window with no masonry to cut is reported: {:?}",
        codes(&out),
    );
}

#[test]
fn a_door_in_walls_that_paint_nothing_says_what_is_missing() {
    let source = "struct s size=5x5\n  walls mat_slot=wall height=4\n  door side=front at=center mat_slot=wall\n";
    let out = lowered(source);
    let reason = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::DeferredMember && d.primary.contains("door"))
        .unwrap_or_else(|| panic!("the door must be reported: {:?}", codes(&out)))
        .primary
        .clone();
    assert!(
        reason.contains("paint"),
        "the reason must name a wall that paints, not only a positive `height=`: {reason}",
    );
}

/// The CLI always passes a registry; this is the library API.
#[test]
fn walls_whose_abstract_token_cannot_be_resolved_do_not_raise_the_array() {
    let line = "  walls mat_slot=abstract height=6\n";
    let source =
        format!("theme t:\n  slot abstract -> @floor.stone.smooth\n\nstruct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
    assert!(
        codes(&lowered(&source)).contains(&"W_ABSTRACT_TOKEN_DEFERRED"),
        "the deferred token is still reported: {:?}",
        codes(&lowered(&source)),
    );
}

/// A roof falls back to a material of its own, while walls whose material does not resolve are
/// not built.
#[test]
fn a_roof_still_draws_over_walls_that_paint_nothing() {
    let source = "struct s size=5x5\n  walls mat_slot=wall height=6\n  roof kind=gable mat_slot=roof overhang=1\n";
    let with_walls = dims_of(&lowered(source), "struct::s");
    let without_walls = dims_of(
        &lowered(&source.replace("  walls mat_slot=wall height=6\n", "")),
        "struct::s",
    );
    assert_eq!(
        with_walls, without_walls,
        "the walls contribute nothing, so the roof seats where it would with no walls at all",
    );
    let out = lowered(source);
    let built = out.structures.get("struct::s").expect("it lowers");
    assert!(
        built
            .palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:spruce_stairs"),
        "the roof still paints, with the material it falls back to: {:?}",
        built.palette.entries,
    );
}
