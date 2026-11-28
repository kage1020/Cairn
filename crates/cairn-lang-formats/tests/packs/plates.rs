//! What the built-in packs make of the block a `pressure_plate`'s `mat_slot=` binds: a pressure
//! plate as the target spells it, and any other block refused for the default plate
//! (`spec/redstone` "Signal binding").

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::{DiagnosticCode, Edition};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_pack;

use crate::common::{lower_source, states_for, supported_versions, written_palette};

/// A room whose theme binds `plate` to `@<token>`, with a plate on it.
fn plated(token: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot plate -> @{token}\n\n\
         struct s size=5x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         pressure_plate at=inside.front offset=1 y=1 mat_slot=plate\n"
    )
}

/// The block at the plate's cell.
fn plate(out: &BlockArrayIr) -> &str {
    let array = &out.structures["struct::s"];
    let at = array.dims.index(1, 1, 3).expect("inside the room");
    &array.palette.entries[usize::from(array.voxels[at].0)].id
}

#[test]
fn a_plate_is_built_from_the_pressure_plate_its_slot_binds_as_each_target_spells_it() {
    let bound = [
        ("oak_pressure_plate", "wooden_pressure_plate"),
        ("stone_pressure_plate", "stone_pressure_plate"),
        (
            "heavy_weighted_pressure_plate",
            "heavy_weighted_pressure_plate",
        ),
    ];
    for edition in [Edition::Java, Edition::Bedrock] {
        for version in supported_versions(builtin_pack(edition)) {
            for (token, bedrock) in bound {
                let out = lower_source(&plated(token), edition, Some(version));
                assert_eq!(out.diagnostics, [], "{token} at {edition:?} {version}");
                assert_eq!(
                    plate(&out),
                    format!("minecraft:{token}"),
                    "{token} at {version}"
                );
                if edition == Edition::Bedrock {
                    assert_eq!(
                        written_plates(&out, version),
                        [format!("minecraft:{bedrock}[redstone_signal=0]")],
                        "{token} at {version}",
                    );
                }
            }
        }
    }
}

#[test]
fn a_plate_bound_to_a_full_block_is_refused_and_built_from_the_default_plate_at_every_target() {
    let source = plated("cobblestone");
    for (edition, default) in [
        (Edition::Java, "minecraft:oak_pressure_plate"),
        (Edition::Bedrock, "minecraft:wooden_pressure_plate"),
    ] {
        for version in supported_versions(builtin_pack(edition)) {
            let out = lower_source(&source, edition, Some(version));
            let found: Vec<(DiagnosticCode, &str)> = out
                .diagnostics
                .iter()
                .map(|d| (d.code, &source[d.span.clone()]))
                .collect();
            assert_eq!(
                found,
                [(DiagnosticCode::IncompatibleMaterial, "@cobblestone")],
                "at {edition:?} {version}",
            );
            assert_eq!(plate(&out), default, "at {edition:?} {version}");
        }
    }
}

/// The pressure plates in the palette of the `.mcstructure` `out` writes at Bedrock `version`.
fn written_plates(out: &BlockArrayIr, version: &str) -> Vec<String> {
    let target = resolve_bedrock_target(version).expect("a shipped target");
    let (root, _) =
        build_mcstructure_tag(&out.structures["struct::s"], &target, &states_for(&target))
            .unwrap_or_else(|err| panic!("not written at {version}: {err}"));
    written_palette(&root)
        .into_iter()
        .filter(|entry| entry.contains("pressure_plate"))
        .collect()
}
