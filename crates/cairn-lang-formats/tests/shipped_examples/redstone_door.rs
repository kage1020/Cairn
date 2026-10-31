//! `redstone-door.crn` lowered: its gatehouse, pressure plates, circuit marker and door actuator.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_formats::registry::builtin_pack;

use crate::common::{
    example_source, id_at, latest, lower_example, lower_source, supported_versions,
};

fn lower_redstone_door() -> BlockArrayIr {
    lower_example(
        "redstone-door.crn",
        Edition::Java,
        Some(latest(Edition::Java)),
    )
}

#[test]
fn redstone_door_lowers_to_a_gatehouse_under_an_overhanging_roof() {
    let out = lower_redstone_door();
    let ba = out
        .structures
        .get("struct::gatehouse")
        .expect("gatehouse struct present");
    assert_eq!((ba.dims.x, ba.dims.y, ba.dims.z), (9, 5, 7));
}

#[test]
fn redstone_door_palette_contains_oak_pressure_plate() {
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"minecraft:oak_pressure_plate"),
        "palette missing oak_pressure_plate; got {ids:?}",
    );
}

/// Bedrock has no `oak_pressure_plate`, so lowering must take the plate id from the pack.
#[test]
fn the_bedrock_pack_plates_with_the_bedrock_id_on_every_supported_target() {
    for version in supported_versions(builtin_pack(Edition::Bedrock)) {
        let out = lower_example("redstone-door.crn", Edition::Bedrock, Some(version));
        let ba = out.structures.get("struct::gatehouse").unwrap();
        let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"minecraft:wooden_pressure_plate"),
            "bedrock {version} palette missing wooden_pressure_plate; got {ids:?}",
        );
        assert!(
            !ids.contains(&"minecraft:oak_pressure_plate"),
            "bedrock {version} palette carries the Java-only oak_pressure_plate; got {ids:?}",
        );
    }
}

#[test]
fn redstone_door_plates_sit_on_either_side_of_the_door() {
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    // The door's opening is (4, 1..=2, 5), in the front wall at z=5.
    for (cell, id) in [
        ((4, 0, 6), "minecraft:oak_pressure_plate"),
        ((4, 1, 5), "minecraft:air"),
        ((4, 1, 4), "minecraft:oak_pressure_plate"),
        ((4, 0, 4), "minecraft:oak_planks"),
    ] {
        let (x, y, z) = cell;
        assert_eq!(id_at(ba, x, y, z), Some(id), "{cell:?}");
    }
    let plates = ba
        .voxels
        .iter()
        .filter(|v| ba.palette.entries[usize::from(v.0)].id == "minecraft:oak_pressure_plate")
        .count();
    assert_eq!(plates, 2);
}

#[test]
fn redstone_door_pressure_plate_lines_emit_no_deferred_warnings() {
    let out = lower_redstone_door();
    let plate_deferred = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::DeferredMember))
        .filter(|d| d.primary.contains("pressure_plate"))
        .collect::<Vec<_>>();
    assert!(
        plate_deferred.is_empty(),
        "pressure_plate must lower without deferred members; got {} — first: {}",
        plate_deferred.len(),
        plate_deferred
            .first()
            .map_or("<none>", |d| d.primary.as_str()),
    );
}

#[test]
fn redstone_door_lowers_without_deferred_warnings() {
    let out = lower_redstone_door();
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::DeferredMember))
        .collect::<Vec<_>>();
    assert!(
        deferred.is_empty(),
        "redstone-door should have zero deferred members; got {} — primaries: {:?}",
        deferred.len(),
        deferred
            .iter()
            .map(|d| d.primary.as_str())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn redstone_door_actuator_patch_bad_id_emits_actuator_shaped_primary() {
    // Keeps the test above from passing on a recognizer that accepts every patch.
    let mut source = example_source("redstone-door.crn");
    source.push_str("  door[id=nonexistent] opened_by=sig.open\n");
    let out = lower_source(&source, Edition::Java, Some(latest(Edition::Java)));

    let actuator_refusals: Vec<&cairn_lang_core::check::Diagnostic> = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::InvalidMember))
        .filter(|d| d.primary.contains("actuator patch"))
        .collect();
    assert_eq!(
        actuator_refusals.len(),
        1,
        "canary expected exactly one actuator-shaped refusal, got {} — primaries: {:?}",
        actuator_refusals.len(),
        actuator_refusals
            .iter()
            .map(|d| d.primary.as_str())
            .collect::<Vec<_>>(),
    );
    assert!(
        actuator_refusals[0].primary.contains("id=nonexistent"),
        "canary primary must render the offending id verbatim, got {}",
        actuator_refusals[0].primary,
    );
    assert!(
        actuator_refusals[0].primary.contains("front"),
        "canary primary must list `front` as a known door id (the fixture's only physical door), got {}",
        actuator_refusals[0].primary,
    );
}
