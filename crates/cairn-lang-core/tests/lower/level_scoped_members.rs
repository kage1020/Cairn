//! `level y=N` grouping does not change what its members mean: a painted member lowers as if
//! written in the body, and a deferred one as if never written.

use cairn_lang_core::block_array::BlockArrayIr;

use crate::common::{lowered, only_structure};

fn defer_reasons(ir: &BlockArrayIr) -> Vec<String> {
    ir.diagnostics.iter().map(|d| d.primary.clone()).collect()
}

/// A theme binding the members' two slots, and a struct header wide enough for a roof ridge.
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

    // Dims, palette and every voxel: a volume of the right size can still be missing the roof.
    assert_eq!(only_structure(&nested), only_structure(&direct));
    assert_eq!(defer_reasons(&nested), Vec::<String>::new());
    // Guard: neither side may pass by building no roof at all.
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
    // `themed-tower.crn` puts walls, a window and an eave stair under `level y=5`.
    let ir = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n\n  \
         level id=upper y=4\n    \
         walls mat_slot=wall height=2\n    \
         window side=front y=1 offset=2 size=2x2 mat_slot=deck\n    \
         stair kind=stairs mat_slot=roof side=back half=top facing=out\n",
    ));
    let ba = only_structure(&ir);

    assert_eq!(defer_reasons(&ir), Vec::<String>::new());
    // The upper walls raise the wall top from 3 to 4+2=6, and the roof sits on that.
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
    assert_eq!(only_structure(&mixed).dims.y, 7);
}

#[test]
fn a_level_scoped_roofs_overhang_is_validated_exactly_once() {
    // The roof is drawn flush with the wall line, so the finding is for the ignored argument.
    let ir = lowered(&source(
        "\n  level id=l0 y=0\n    roof kind=gable mat_slot=roof overhang=nope\n",
    ));

    assert_eq!(
        ir.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.primary.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "W_IGNORED_ARGUMENT",
            "`overhang=` must be a non-negative integer that fits in u32, not identifier `nope`; \
             the value was ignored",
        )],
    );
}

#[test]
fn a_malformed_overhang_on_a_dropped_roof_is_not_read_at_all() {
    // `spec/lint` "Error vs warning" names this exception to reporting unreadable values.
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
    let raised = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n\n  \
         level id=upper y=3\n    pressure_plate at=inside.front offset=1 mat_slot=deck\n",
    ));
    let ground = lowered(&source(
        "  roof kind=gable mat_slot=roof overhang=1\n  \
         pressure_plate at=inside.front offset=1 y=3 mat_slot=deck\n",
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
