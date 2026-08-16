//! `level y=N` grouping must not change what the members inside it mean.
//!
//! Dimension derivation and painting used to read two different member
//! lists. The dims walked the source members, where a `level` block is
//! opaque, while the paint pass walked the flattened ones. A `roof` under
//! `level y=0` was therefore invisible to the volume that had to hold it:
//! every voxel it produced landed past the end of that volume and was
//! dropped without a word, leaving a palette full of stairs and a structure
//! with none.
//!
//! These tests pin the two lists to each other from both directions. A
//! member the pass paints is one the dims saw — so it lowers to exactly the
//! volume it would have produced written directly in the body. A member the
//! pass defers is one the dims did not see — so it lowers to exactly the
//! volume of a source that never mentioned it. Anything in between is the
//! shape of this bug.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// The single structure the sources below declare.
fn only_structure(ir: &BlockArrayIr) -> &BlockArray {
    assert_eq!(
        ir.structures.len(),
        1,
        "these sources declare exactly one struct",
    );
    ir.structures.values().next().expect("one structure")
}

fn defer_reasons(ir: &BlockArrayIr) -> Vec<String> {
    ir.diagnostics.iter().map(|d| d.primary.clone()).collect()
}

/// Body shared by every source here: a theme with the two slots the members
/// bind, and a struct header wide enough for a roof to have a ridge.
const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n  \
                       slot roof -> @spruce_stairs\n  \
                       slot deck -> @oak_planks\n\n\
                       struct t size=9x7\n  \
                       walls mat_slot=wall height=3\n";

fn source(body: &str) -> String {
    format!("{PRELUDE}{body}")
}

#[test]
fn a_roof_under_level_zero_builds_what_the_same_roof_in_the_body_builds() {
    let nested = lowered(&source(
        "\n  level id=l0 y=0\n    roof kind=gable mat_slot=roof overhang=1\n",
    ));
    let direct = lowered(&source("  roof kind=gable mat_slot=roof overhang=1\n"));

    // Dims, palette, and every voxel — not just the extents. A volume of
    // the right size with the roof still missing from it is the failure
    // this whole file is about.
    assert_eq!(only_structure(&nested), only_structure(&direct));
    assert_eq!(defer_reasons(&nested), Vec::<String>::new());
    // Guard against the assertion above passing because neither side built
    // a roof at all.
    assert!(
        only_structure(&direct).dims.y > 4,
        "the direct roof must raise the volume above the wall top for this comparison to mean anything",
    );
}

#[test]
fn a_floor_under_level_zero_builds_what_the_same_floor_in_the_body_builds() {
    let nested = lowered(&source("\n  level id=l0 y=0\n    floor mat_slot=deck\n"));
    let direct = lowered(&source("  floor mat_slot=deck\n"));

    assert_eq!(only_structure(&nested), only_structure(&direct));
    assert_eq!(defer_reasons(&nested), Vec::<String>::new());
    assert!(
        only_structure(&direct)
            .palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:oak_planks"),
        "the direct floor must reach the palette for this comparison to mean anything",
    );
}

#[test]
fn a_deferred_roof_under_a_raised_level_does_not_inflate_the_volume() {
    let with_roof = lowered(&source(
        "\n  level id=upper y=5\n    roof kind=gable mat_slot=roof overhang=1\n",
    ));
    let without = lowered(&source(""));

    // A member that paints nothing must cost nothing. Reading `overhang=1`
    // off a roof that is about to be dropped would widen the footprint by a
    // ring of air on every side and shift the walls inward to match.
    assert_eq!(only_structure(&with_roof), only_structure(&without));
    assert_eq!(
        defer_reasons(&with_roof),
        vec!["level-scoped `roof` is not yet supported".to_owned()],
    );
}

#[test]
fn a_deferred_floor_under_a_raised_level_does_not_inflate_the_volume() {
    let with_floor = lowered(&source("\n  level id=upper y=5\n    floor mat_slot=deck\n"));
    let without = lowered(&source(""));

    assert_eq!(only_structure(&with_floor), only_structure(&without));
    assert_eq!(
        defer_reasons(&with_floor),
        vec!["level-scoped `floor` is not yet supported".to_owned()],
    );
}

#[test]
fn a_raised_level_still_paints_the_roles_that_have_a_lowering_at_an_offset() {
    // `themed-tower.crn` puts walls, a window, and an eave stair under
    // `level y=5`. Deferring by offset rather than by role would silently
    // empty the upper floor of the one example that demonstrates levels.
    let ir = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n\n  \
         level id=upper y=4\n    \
         walls mat_slot=wall height=2\n    \
         window side=front y=1 offset=2 size=2x2 mat_slot=deck\n    \
         stair kind=stairs mat_slot=roof side=back half=top facing=out\n",
    ));
    let ba = only_structure(&ir);

    assert_eq!(defer_reasons(&ir), Vec::<String>::new());
    // The upper walls raise the wall top from 3 to 4+2=6, which the roof
    // then sits on top of.
    assert!(
        ba.dims.y > 7,
        "level-scoped walls must reach the dim math, got dims.y={}",
        ba.dims.y,
    );
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"minecraft:oak_planks"),
        "the level-scoped window must paint, palette was {ids:?}",
    );
}

#[test]
fn dropping_one_member_of_a_raised_level_keeps_its_siblings() {
    // The drop happens while the level's body is being walked, so getting
    // out of the loop instead of past the one member takes everything
    // declared after it with no second diagnostic to show for it.
    let mixed = lowered(&source(
        "\n  level id=upper y=4\n    \
         roof kind=gable mat_slot=roof overhang=1\n    \
         walls id=upper_shell mat_slot=wall height=2\n",
    ));
    let siblings_only = lowered(&source(
        "\n  level id=upper y=4\n    walls id=upper_shell mat_slot=wall height=2\n",
    ));

    assert_eq!(only_structure(&mixed), only_structure(&siblings_only));
    assert_eq!(
        defer_reasons(&mixed),
        vec!["level-scoped `roof` is not yet supported".to_owned()],
    );
    // 4 + 2 = 6 wall top, plus the base row.
    assert_eq!(only_structure(&mixed).dims.y, 7);
}

#[test]
fn a_level_scoped_roofs_overhang_is_validated_exactly_once() {
    // The dim pass is the only reader of `overhang=`, so a roof it cannot
    // see is a roof whose `overhang=` nothing validates — that is what this
    // pins. Reading it twice is the other way to get it wrong.
    //
    // The message is the one the shared non-negative-integer reader emits,
    // recorded here as it stands. It overstates the outcome: the roof is
    // still drawn, flush with the wall line, rather than deferred.
    let ir = lowered(&source(
        "\n  level id=l0 y=0\n    roof kind=gable mat_slot=roof overhang=nope\n",
    ));

    assert_eq!(
        defer_reasons(&ir),
        vec!["`overhang=` must be a non-negative integer that fits in u32".to_owned()],
    );
}

#[test]
fn a_malformed_overhang_on_a_dropped_roof_is_not_read_at_all() {
    // A member the pass drops costs nothing, and validating an argument
    // nothing will use is a cost: it reports a mistake in a line that has
    // no effect either way.
    let ir = lowered(&source(
        "\n  level id=upper y=5\n    roof kind=gable mat_slot=roof overhang=nope\n",
    ));

    assert_eq!(
        defer_reasons(&ir),
        vec!["level-scoped `roof` is not yet supported".to_owned()],
    );
}

#[test]
fn a_pressure_plate_under_a_raised_level_lands_at_that_level() {
    // `pressure_plate` is on the side of the rule that keeps its lowering
    // at an offset, and nothing else in this file proves it: a mutant that
    // moved it to the dropped side, or one that stopped adding the offset,
    // would be caught by `walls` alone in every other test here.
    let raised = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n\n  \
         level id=upper y=3\n    pressure_plate at=inside.front mat_slot=deck\n",
    ));
    let ground = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n  \
         pressure_plate at=inside.front y=3 mat_slot=deck\n",
    ));

    assert_eq!(defer_reasons(&raised), Vec::<String>::new());
    assert_eq!(only_structure(&raised), only_structure(&ground));
    assert!(
        only_structure(&raised)
            .palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:oak_planks"),
        "the plate must paint for this comparison to mean anything",
    );
}

#[test]
fn findings_are_reported_in_source_order_not_pass_order() {
    // Deciding participation during the flatten step means a level-scoped
    // member is judged before anything the phases look at, including
    // members written above it. The pass reports in span order so a reader
    // can work down the file instead of jumping around it.
    let ir = lowered(&source(
        "  roof kind=bogus mat_slot=roof\n\n  \
         level id=upper y=5\n    roof kind=gable mat_slot=roof\n",
    ));

    assert_eq!(
        defer_reasons(&ir),
        vec![
            "unknown roof `kind=bogus` (expected one of gable, shed, hip, flat)".to_owned(),
            "level-scoped `roof` is not yet supported".to_owned(),
        ],
    );
}

#[test]
fn a_placed_def_flattens_its_levels_the_way_a_struct_does() {
    // The flatten happens once, inside the body-lowering helper, so a `def`
    // instantiated by a `place` goes through it too — and its dims feed the
    // placement record that walkway routing collides against, not just the
    // voxels.
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "  slot roof -> @spruce_stairs\n",
        "\n",
        "def hut size=9x7:\n",
        "  walls mat_slot=wall height=3\n",
        "\n",
        "  level id=l0 y=0\n",
        "    roof kind=gable mat_slot=roof overhang=1\n",
        "\n",
        "site s:\n",
        "  place id=home use=hut theme=t at=origin\n",
    );
    let ir = lowered(src);
    let ba = ir
        .structures
        .get("site::s::home")
        .expect("place lowered under site::s::home");
    let placement = ir.placements.get("site::s::home").expect("placement");

    assert_eq!(ba.dims.x, 11);
    assert_eq!(ba.dims.z, 9);
    assert!(ba.dims.y > 4, "the roof must raise the volume");
    assert_eq!(
        placement.dims, ba.dims,
        "the placement record must carry the dims the voxels were built into",
    );
}
