//! Every palette entry an artifact carries must be one its voxels use.
//!
//! The palette is not a scratch table the pass keeps to itself. The Java
//! writer emits it verbatim into the `.nbt`, `cairn info` reports one
//! portability row per entry, and `resolved_ir_hash` covers it. An entry no
//! voxel references is therefore a block the tooling counts and the artifact
//! ships for a block that is not in the build.
//!
//! Interning a state before knowing whether the generator will emit it is
//! how one gets there. A gable roof has four stair faces but a 3x3 roof has
//! no room for a high apex, and a roof whose voxels all landed outside the
//! volume interned a full set of stairs and painted none of them.

use std::collections::HashSet;
use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

fn lower_source(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

/// Palette slots no voxel names, rendered for the failure message.
///
/// Slot 0 is exempt. `Palette::new_with_air` puts air there before any
/// member is lowered, so a fully paved volume — every walkway — has an
/// unreferenced air entry by construction, and the two consumers that read
/// the palette skip that slot for the same reason.
fn unreferenced(ba: &BlockArray) -> Vec<String> {
    assert_eq!(
        ba.palette.entries.first().map(|s| s.id.as_str()),
        Some(BlockState::AIR_ID),
        "the exemption below is only sound while slot 0 is the air slot",
    );
    let used: HashSet<u16> = ba.voxels.iter().map(|i| i.0).collect();
    ba.palette
        .entries
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(slot, _)| u16::try_from(*slot).is_ok_and(|slot| !used.contains(&slot)))
        .map(|(slot, state)| format!("[{slot}] {state:?}"))
        .collect()
}

fn assert_every_entry_is_used(label: &str, ir: &BlockArrayIr) {
    for (key, ba) in &ir.structures {
        let dangling = unreferenced(ba);
        assert!(
            dangling.is_empty(),
            "{label}: `{key}` ships palette entries no voxel references: {dangling:#?}",
        );
    }
}

fn examples() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "crn"))
        .collect();
    found.sort();
    // A directory walk that finds nothing turns every assertion below into
    // a claim about the empty set.
    assert!(
        found.len() >= 10,
        "expected the shipped examples, found {found:?}",
    );
    found
}

#[test]
fn no_shipped_example_ships_a_palette_entry_it_does_not_use() {
    for path in examples() {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path.file_name().expect("file name").to_string_lossy();
        assert_every_entry_is_used(&name, &lower_source(&source));
    }
}

#[test]
fn a_gable_too_small_for_a_high_apex_does_not_intern_one() {
    // 3x3 leaves one ridge voxel, so the generator emits three of the four
    // gable faces. Interning the table up front put the fourth in the
    // palette anyway.
    let ir = lower_source(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot roof -> @spruce_stairs\n\n\
         struct t size=3x3\n  \
         walls mat_slot=wall height=2\n  \
         roof kind=gable mat_slot=roof\n",
    );
    assert_every_entry_is_used("3x3 gable", &ir);

    let ba = ir.structures.values().next().expect("one structure");
    assert!(
        ba.palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:spruce_stairs"),
        "the roof must reach the palette for this test to mean anything",
    );
}

#[test]
fn a_shed_shallow_enough_to_be_all_apex_does_not_intern_a_slope() {
    // A one-deep slope span is a single layer, and that layer is the apex.
    // The slope face the other half of the table describes never appears.
    let ir = lower_source(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot roof -> @spruce_stairs\n\n\
         struct t size=3x1\n  \
         walls mat_slot=wall height=2\n  \
         roof kind=shed slope_to=front mat_slot=roof\n",
    );
    assert_every_entry_is_used("3x1 shed", &ir);

    let ba = ir.structures.values().next().expect("one structure");
    assert!(
        ba.palette
            .entries
            .iter()
            .any(|s| s.id == "minecraft:spruce_stairs"),
        "the roof must reach the palette for this test to mean anything",
    );
}

#[test]
fn a_pressure_plate_with_nowhere_to_sit_adds_no_palette_entry() {
    // Above the floor row a `<side>.outside` anchor on a struct with no
    // overhang has no exterior cell and no foundation to fall back on, so
    // the plate defers. A deferred member must leave nothing behind: an
    // entry in the palette is a block `cairn info` counts and the `.nbt`
    // ships for a fixture that was never placed.
    let ir = lower_source(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot plate -> @oak_pressure_plate\n\n\
         struct t size=3x3\n  \
         walls mat_slot=wall height=2\n  \
         pressure_plate at=front.outside y=1 mat_slot=plate\n",
    );

    assert_eq!(
        ir.diagnostics.len(),
        1,
        "the plate must say it was dropped: {:#?}",
        ir.diagnostics,
    );
    let ba = ir.structures.values().next().expect("one structure");
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        !ids.contains(&"minecraft:oak_pressure_plate"),
        "a plate with nowhere to go must not reach the palette, got {ids:?}",
    );
    assert_every_entry_is_used("dropped plate", &ir);
}
