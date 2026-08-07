//! What an indented body actually costs, measured on the build rather
//! than on the diagnostic.
//!
//! `check_nesting.rs` pins the wording and the anchoring of
//! `E_UNSUPPORTED_NESTING`; every assertion there filters to that code
//! and never lowers, so none of them would notice if the premise —
//! "these members produce no blocks" — stopped being true. These do the
//! lowering and compare against the same source with the indentation
//! removed.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

fn build(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// Cells that are not air, which is the only thing a `door` changes:
/// it cuts an opening out of a wall that is otherwise solid.
fn solid_cells(array: &BlockArray) -> usize {
    array.voxels.iter().filter(|c| c.0 != 0).count()
}

const THEME: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n";

/// A `door` indented under `walls` cuts no opening. Same source, same
/// def, one level of indentation apart — so the difference in solid
/// cells is the door and nothing else.
#[test]
fn nl_1_a_nested_door_cuts_no_opening() {
    let flat = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n"
    );
    let nested = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let flat_solid = solid_cells(&build(&flat).structures["struct::s"]);
    let nested_solid = solid_cells(&build(&nested).structures["struct::s"]);
    assert!(
        nested_solid > flat_solid,
        "the nested door should cut nothing, so the wall stays solid: \
         flat={flat_solid} nested={nested_solid}",
    );
}

/// A `door` under a `level y=0` does cut its opening — the negative
/// space for the test above, and the reason `level` is exempt from the
/// nesting check at all.
#[test]
fn nl_2_a_door_under_a_level_still_cuts_its_opening() {
    let flat = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n"
    );
    let levelled = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         level y=0\n    door id=d side=front at=center\n"
    );
    assert_eq!(
        solid_cells(&build(&levelled).structures["struct::s"]),
        solid_cells(&build(&flat).structures["struct::s"]),
        "a `level` body reaches the build, so the opening is cut either way",
    );
}

const SITE_PRELUDE: &str = "def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// A `place` indented under another `place` produces no placement, and
/// a `connect` indented under a `place` lays no walkway. Both come out
/// of `resolve_site_placements` iterating the rows without descending.
#[test]
fn nl_3_a_nested_site_row_produces_neither_a_placement_nor_a_walkway() {
    let flat = format!(
        "{THEME}{SITE_PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let built = build(&flat);
    assert_eq!(built.placements.len(), 2, "baseline: both places build");
    assert_eq!(built.walkways.len(), 1, "baseline: the walkway is laid");

    let nested_place = format!(
        "{THEME}{SITE_PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n    \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n"
    );
    assert_eq!(
        build(&nested_place).placements.len(),
        1,
        "the nested place must not reach the build",
    );

    let nested_connect = format!(
        "{THEME}{SITE_PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n    \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    assert_eq!(
        build(&nested_connect).walkways.len(),
        0,
        "the nested connect must lay no walkway",
    );
}

/// The other half of the message's claim: a nested member in a geometry
/// body is not gone from resolution — it still takes a theme binding.
/// The note says so, so something has to hold it.
#[test]
fn nl_4_a_nested_geometry_member_still_takes_a_theme_binding() {
    let src = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center mat_slot=wall\n"
    );
    let module = parse(&src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let scope = &resolution.scopes["struct::s"];
    let nested_start = src
        .find("door id=d")
        .expect("the nested door is in the source");
    let binding = scope
        .members
        .get(&nested_start)
        .expect("the nested member is resolved");
    assert!(
        binding.slot_value.is_some(),
        "the nested member's `mat_slot=` should still resolve through the theme",
    );
}
