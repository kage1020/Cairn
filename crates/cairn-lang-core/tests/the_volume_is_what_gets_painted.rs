//! A member that paints nothing shapes nothing.
//!
//! `spec/compilation.md` §4.7 derives the volume from `overhang`,
//! `wall_top` and `roof_extra`, and states the invariant the three have to
//! keep: "every member the pass paints is one the volume was sized to
//! hold", two readings of one list, which "is what keeps a member from
//! painting past the end of the array it was handed". These pin the other
//! direction — a member the pass does *not* paint must not be sized for
//! either, whether it drops out for want of a `kind=` or for want of a
//! material.
//!
//! Every dims assertion here is written against the same source with the
//! member's line deleted rather than against a literal, because the claim
//! is "this member contributed nothing", not "the answer is 5x4x5".

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{lower, parse};

fn lower_source(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

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

/// The dims `with` produces, and the dims the same source produces with
/// `line` deleted. A member that contributes nothing makes the two equal.
fn with_and_without(source: &str, line: &str, key: &str) -> ((u32, u32, u32), (u32, u32, u32)) {
    assert_eq!(
        source.matches(line).count(),
        1,
        "the fixture must contain the line it deletes exactly once: {line:?}",
    );
    let without = source.replace(line, "");
    (
        dims_of(&lower_source(source), key),
        dims_of(&lower_source(&without), key),
    )
}

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @oak_stairs\n\n";

// --- A. A roof with no lowering shapes nothing ------------------------------

/// `overhang=` widened the footprint for a roof that could not be drawn.
///
/// `max_roof_extra_height` filters the roof list through `roof_kind_of` and
/// `max_roof_overhang` did not, so the member gave the volume its width and
/// nothing else: the walls moved inward and a ring of air surrounded them.
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
        codes(&lower_source(&source)).contains(&"W_DEFERRED_MEMBER"),
        "the missing `kind=` is still reported: {:?}",
        codes(&lower_source(&source)),
    );
}

/// The same roof with an unusable `overhang=`: the key is still read, so it
/// is still reported, and the member still contributes nothing.
#[test]
fn a_roof_with_no_kind_and_an_unusable_overhang_reports_both_and_widens_nothing() {
    let line = "  roof mat_slot=roof overhang=nope\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
    let out = lower_source(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "W_DEFERRED_MEMBER" && d.primary.contains("kind=")),
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

/// The control: a roof that *is* drawn still widens by its `overhang=`.
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

/// A `shed` with no `slope_to=` is a kind the table knows and a roof the
/// pass will not draw.
///
/// `roof_kind_of` answers "is this name in the table", which is a
/// different question from "will this member paint" — `fill_roof_shed`
/// returns before its first voxel without a `slope_to=`, and the height
/// walk already gave it `0`. Filtering the footprint on the name alone
/// left the two roof walks disagreeing for exactly the reason they
/// disagreed before this change.
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

/// The control on the other side: the same shed with a `slope_to=` draws,
/// and widens.
#[test]
fn a_shed_with_a_slope_to_still_widens_the_footprint() {
    let line = "  roof kind=shed slope_to=front mat_slot=roof overhang=3\n";
    let source = format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with.0, without.0 + 6, "three blocks of eave on each side");
    assert!(with.1 > without.1, "and a slope over the wall top");
}

/// A `pressure_plate` outside the wall needs an overhang to stand on, and
/// the overhang it needs is one a roof draws.
///
/// The same gate as the eave stair, one member over: a roof with
/// `overhang=2` and no `kind=` contributes nothing, so the plate has
/// nowhere to sit — and must not be told the struct has no overhang when
/// the source says otherwise.
#[test]
fn a_plate_outside_a_wall_names_the_overhang_a_roof_draws() {
    let source = format!(
        "{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         roof mat_slot=roof overhang=2\n  \
         pressure_plate id=p at=front.outside offset=0 y=1 mat_slot=wall\n"
    );
    let out = lower_source(&source);
    let reason = out
        .diagnostics
        .iter()
        .find(|d| d.primary.contains("pressure_plate"))
        .unwrap_or_else(|| panic!("the plate must be reported: {:?}", codes(&out)))
        .primary
        .clone();
    assert!(
        reason.contains("draws an overhang"),
        "the reason must name the overhang a roof draws: {reason}",
    );
}

/// A struct with one painting course and one that will not paint keeps the
/// first and drops the second, whichever order they are written in.
///
/// The single-`walls` fixtures above cannot see a filter that gives up on
/// the first miss, or one that keeps the whole list once any member
/// paints. Both courses are level-scoped so the surviving one is not the
/// one at offset zero.
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
        let out = lower_source(&source);
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

/// A struct with no theme bound paints no `mat_slot=` at all, and used to
/// reserve the full wall height anyway: a `3x7x3` array whose palette is
/// air and nothing else.
#[test]
fn themeless_walls_do_not_raise_the_array() {
    let line = "  walls mat_slot=wall height=6\n";
    let source = format!("struct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without, "walls that paint nothing raise nothing");
    assert!(
        codes(&lower_source(&source)).contains(&"W_NO_THEME_BOUND"),
        "and the themeless scope is still reported",
    );
}

/// The same shape reached a different way, and this one says nothing at
/// all: `resolve_member_state` returns `None` for a member with no
/// `mat_slot=` binding without a diagnostic from any pass.
#[test]
fn walls_with_no_material_binding_do_not_raise_the_array() {
    let line = "  walls height=6\n";
    let source = format!("{THEME}struct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
}

/// The control: walls that paint raise the array, and by the height the
/// author asked for.
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

/// A window in walls that paint nothing is deferred, and the array shrinks
/// with it.
///
/// The two halves have to move together. `wall_column` decides whether the
/// rectangle lands in masonry and `max_wall_top` sizes the array; filtering
/// one without the other would leave the carve writing rows that no longer
/// exist. What the window loses is a carve of air out of air — the lowered
/// output of this fixture is byte-identical to the same source without the
/// window today.
#[test]
fn a_window_in_walls_that_paint_nothing_is_deferred_and_the_array_shrinks() {
    let line = "  window side=front y=1 offset=1 size=2x2 mat_slot=wall\n";
    let source = format!("struct s size=5x5\n  walls mat_slot=wall height=4\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without, "the window shapes nothing either");
    let out = lower_source(&source);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "W_DEFERRED_MEMBER"),
        "a window with no masonry to cut is reported: {:?}",
        codes(&out),
    );
}

/// A door in the same walls defers too, and says what is actually missing.
///
/// The gate is `wall_top < 1`, which now reads "no walls member paints"
/// as well as "no walls member has a positive `height=`". The author wrote
/// a positive height, so a reason naming only that sends them to the wrong
/// line.
#[test]
fn a_door_in_walls_that_paint_nothing_says_what_is_missing() {
    let source = "struct s size=5x5\n  walls mat_slot=wall height=4\n  door side=front at=center mat_slot=wall\n";
    let out = lower_source(source);
    let reason = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_str() == "W_DEFERRED_MEMBER" && d.primary.contains("door"))
        .unwrap_or_else(|| panic!("the door must be reported: {:?}", codes(&out)))
        .primary
        .clone();
    assert!(
        reason.contains("paint"),
        "the reason must name a wall that paints, not only a positive `height=`: {reason}",
    );
}

/// An abstract token with no registry to resolve it against falls back to
/// air, so those walls shape nothing either.
///
/// Not reachable from the CLI — every `lower_to_block_array` call site
/// there passes a registry — but this is the library API, and the
/// predicate covers the arm for free because it asks the painter's own
/// question rather than enumerating the ways a material can fail.
#[test]
fn walls_whose_abstract_token_cannot_be_resolved_do_not_raise_the_array() {
    let line = "  walls mat_slot=abstract height=6\n";
    let source =
        format!("theme t:\n  slot abstract -> @floor.stone.smooth\n\nstruct s size=3x3\n{line}");
    let (with, without) = with_and_without(&source, line, "struct::s");
    assert_eq!(with, without);
    assert!(
        codes(&lower_source(&source)).contains(&"W_ABSTRACT_TOKEN_DEFERRED"),
        "the deferred token is still reported: {:?}",
        codes(&lower_source(&source)),
    );
}

/// A roof falls back to a material of its own where walls fall back to
/// air, so a themeless struct with a gable still draws one — now seated on
/// the ground plane instead of on walls that are not there.
#[test]
fn a_roof_still_draws_over_walls_that_paint_nothing() {
    let source = "struct s size=5x5\n  walls mat_slot=wall height=6\n  roof kind=gable mat_slot=roof overhang=1\n";
    let with_walls = dims_of(&lower_source(source), "struct::s");
    let without_walls = dims_of(
        &lower_source(&source.replace("  walls mat_slot=wall height=6\n", "")),
        "struct::s",
    );
    assert_eq!(
        with_walls, without_walls,
        "the walls contribute nothing, so the roof seats where it would with no walls at all",
    );
    let out = lower_source(source);
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
