//! `redstone-door.crn` lowers its pressure plates, `circuit` marker and door
//! actuator patch: the gatehouse dims, the plate id each edition's pack
//! names and the voxels the plates land on, and no `W_DEFERRED_MEMBER` for
//! any of the three.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{
    example_source, latest, lower_example, lower_source, pack_for, supported_versions,
};

fn lower_redstone_door() -> BlockArrayIr {
    lower_example(
        "redstone-door.crn",
        Edition::Java,
        Some(latest(Edition::Java)),
    )
}

#[test]
fn redstone_door_lowers_to_a_flat_gatehouse_volume() {
    // struct gatehouse size=7x5, walls height=3, no roof, no overhang →
    // dims.x = 7, dims.z = 5, dims.y = floor(1) + wall_top(3) = 4.
    let out = lower_redstone_door();
    let ba = out
        .structures
        .get("struct::gatehouse")
        .expect("gatehouse struct present");
    assert_eq!(ba.dims.x, 7);
    assert_eq!(ba.dims.y, 4);
    assert_eq!(ba.dims.z, 5);
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

/// The plate id is edition-specific, and lowering has no edition of its
/// own — it reads the pack's `pressure_plate.default`.
///
/// Bedrock has never had `oak_pressure_plate`, so a lowering that ignored
/// the pack and used its hardcoded Java default would put an id into the
/// `.mcstructure` palette that the game loads as air. Asserting the pack
/// *declares* the right id is not enough: this is the test that the
/// lowering pass actually reads it.
#[test]
fn the_bedrock_pack_plates_with_the_bedrock_id_on_every_supported_target() {
    // The buildable rows only: the table also carries the releases the
    // pack can order a floor against but has no block data for, and
    // pinning a lowering to one of those checks nothing.
    for version in supported_versions(pack_for(Edition::Bedrock)) {
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
fn redstone_door_outside_plate_falls_back_to_wall_column_without_overhang() {
    // `pressure_plate id=plate at=front.outside offset=0 y=0` — the
    // shift-outward step lands at z=dims.z (out of range) because the
    // struct has no overhang. The `y=0` foundation fallback then paints
    // on the wall's own column at (x=0, y=0, z=4). The main-line
    // "shift into overhang column" semantics is covered by a unit test
    // in `cairn-lang-core::block_array::lower::tests`.
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let plate_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:oak_pressure_plate")
            .expect("oak_pressure_plate palette entry present"),
    )
    .expect("palette index fits in u16");
    let i = ba
        .dims
        .index(0, 0, 4)
        .expect("front-wall corner within bounds");
    assert_eq!(ba.voxels[i].0, plate_idx);
}

#[test]
fn redstone_door_inside_plate_paints_one_voxel_inside_the_front_wall() {
    // `pressure_plate id=inner at=inside.front offset=1 y=0` — shift-inward
    // from (1, 0, 4) is (1, 0, 3), the first interior cell along the front
    // wall. `offset=0` would be the corner, whose inward cell sits under
    // the left wall.
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let plate_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:oak_pressure_plate")
            .expect("oak_pressure_plate palette entry present"),
    )
    .expect("palette index fits in u16");
    let i = ba.dims.index(1, 0, 3).expect("interior row within bounds");
    assert_eq!(ba.voxels[i].0, plate_idx);
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
    // `circuit region=floor void=2` is a routing marker and
    // `door[id=front] opened_by=sig.open` an actuator patch binding the
    // struct's `door id=front` to a logic signal; block-array lowering
    // recognises both and emits neither voxels nor `W_DEFERRED_MEMBER`.
    // The total is pinned rather than primaries filtered by role, because
    // `nonneg_int_or_defer`'s primary does not name the enclosing role.
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
    // Keeps the test above from passing on a `recognize_actuator_patch`
    // that accepts everything: a patch naming no door must defer once,
    // with a primary naming the offending id and the fixture's real door
    // id `front`.
    let mut source = example_source("redstone-door.crn");
    source.push_str("  door[id=nonexistent] opened_by=sig.open\n");
    let out = lower_source(&source, Edition::Java, Some(latest(Edition::Java)));

    let actuator_defers: Vec<&cairn_lang_core::check::Diagnostic> = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::DeferredMember))
        .filter(|d| d.primary.contains("actuator patch"))
        .collect();
    assert_eq!(
        actuator_defers.len(),
        1,
        "canary expected exactly one actuator-shaped deferred entry, got {} — primaries: {:?}",
        actuator_defers.len(),
        actuator_defers
            .iter()
            .map(|d| d.primary.as_str())
            .collect::<Vec<_>>(),
    );
    assert!(
        actuator_defers[0].primary.contains("id=nonexistent"),
        "canary primary must render the offending id verbatim, got {}",
        actuator_defers[0].primary,
    );
    assert!(
        actuator_defers[0].primary.contains("front"),
        "canary primary must list `front` as a known door id (the fixture's only physical door), got {}",
        actuator_defers[0].primary,
    );
}
