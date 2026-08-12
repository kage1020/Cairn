//! Integration coverage for `redstone-door.crn` under `pressure_plate`,
//! `circuit`, and door actuator-patch lowering. Pins the palette (air,
//! floor material, wall material, `oak_pressure_plate`), the two plate
//! voxels, and the invariant that no `W_DEFERRED_MEMBER` fires on any
//! of the three fixture roles: the `pressure_plate` lines, the
//! `circuit region=floor void=2` routing marker, or the
//! `door[id=front] opened_by=sig.open` actuator patch that binds the
//! physical `door id=front` declared earlier in the struct body to a
//! logic-graph signal. Line-number references are intentionally avoided
//! so the tests survive edits to the fixture.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_redstone_door() -> BlockArrayIr {
    lower_redstone_door_with(builtin_java(), &builtin_java().data_versions.latest)
}

fn lower_redstone_door_with(pack: &RegistryPack, mc_version: &str) -> BlockArrayIr {
    let source = std::fs::read_to_string(examples_dir().join("redstone-door.crn"))
        .expect("redstone-door.crn readable");
    let module = parse(&source).expect("parse redstone-door");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, Some(&pack.view(Some(mc_version))))
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
    let pack = builtin_bedrock();
    for version in pack.data_versions.versions.iter().map(|e| &e.mc_version) {
        let out = lower_redstone_door_with(pack, version);
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
    // `pressure_plate id=inner at=inside.front offset=0 y=0` — shift-inward
    // from (0, 0, 4) is (0, 0, 3), well within dims.
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
    let i = ba.dims.index(0, 0, 3).expect("interior row within bounds");
    assert_eq!(ba.voxels[i].0, plate_idx);
}

#[test]
fn redstone_door_pressure_plate_lines_emit_no_deferred_warnings() {
    // Two plates → zero `W_DEFERRED_MEMBER` diagnostics whose primary
    // message names the `pressure_plate` role. The remaining deferred
    // warning (the selector-form actuator patch on `door[id=front]`) is
    // still expected and will be covered by its own test once the
    // actuator wiring pass lands.
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
    // `circuit region=floor void=2` is a routing marker for the future
    // logic passes; `door[id=front] opened_by=sig.open` is an actuator
    // patch that binds the physical `door id=front` declared earlier
    // in the struct to a logic-graph signal. Block-array lowering
    // recognises both shapes as surface-guards for the future redstone
    // pipeline — neither emits voxels, neither fires a
    // `W_DEFERRED_MEMBER`. Combined with the plate paint tested above,
    // this pins the whole example at "zero deferred members" so a
    // regression on any of the three recognisers fails loud. Instead
    // of filtering primaries by a per-role substring (which would
    // silently pass a regression where the recogniser routes into
    // `nonneg_int_or_defer`'s primary — that primary does not name
    // the enclosing role), pin the total count.
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
    // Canary that guards against the recogniser silently degrading to
    // "always accept": if the "zero deferred" invariant above ever
    // starts holding because `recognize_actuator_patch` was accidentally
    // turned into a no-op, this test still catches the regression by
    // appending a known-bad patch to a copy of the fixture. A
    // `door[id=nonexistent] opened_by=sig.open` line must produce
    // exactly one deferred entry whose primary carries the recogniser's
    // fingerprint (both the offending id rendered verbatim and the
    // fixture's real door id `front`). Filtering on the "actuator patch"
    // substring keeps the assertion targeted at this recogniser rather
    // than any other deferral upstream.
    let mut source = std::fs::read_to_string(examples_dir().join("redstone-door.crn"))
        .expect("redstone-door.crn readable");
    source.push_str("  door[id=nonexistent] opened_by=sig.open\n");
    let module = cairn_lang_core::parse(&source).expect("parse redstone-door canary");
    let ir = cairn_lang_core::lower(&module);
    let resolution = cairn_lang_core::resolve(&ir, None);
    let pack = builtin_java();
    let out = lower_to_block_array(
        &ir,
        &resolution,
        Some(&pack.view(Some(&pack.data_versions.latest))),
    );

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
