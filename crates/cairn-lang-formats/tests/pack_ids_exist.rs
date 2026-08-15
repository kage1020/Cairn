//! Every id a built-in pack can produce exists in the target it produces it for.
//!
//! The Bedrock pack shipped `oak_sign`, `oak_wall_sign`, and (through core)
//! `oak_pressure_plate` — three Java spellings that Bedrock has never had.
//! Nothing caught them, because nothing compared a pack's own mappings
//! against a list of ids that edition declares. These tests are that
//! comparison, and they run against every supported version rather than
//! one, because a rename inside an edition's own range (`stonebrick` →
//! `stone_bricks` at Bedrock 1.21.40) is invisible to a spot check.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BUILTIN_BLOCK_IDS, lower_to_block_array};
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

/// Every version the pack supports, as the `--target` strings a user types.
fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.data_versions
        .versions
        .iter()
        .map(|e| e.mc_version.as_str())
        .collect()
}

fn edition(pack: &RegistryPack) -> &'static str {
    pack.manifest.edition.label()
}

#[test]
fn every_material_mapping_resolves_to_an_id_its_target_declares() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for version in supported_versions(pack) {
            let ids = pack
                .blocks
                .ids_for(version)
                .unwrap_or_else(|| panic!("{} {version} has no block table", edition(pack)));
            for (token, pinned, id) in pack.materials.mappings() {
                // A mapping pinned to another version says nothing about
                // this one; the default it falls back from is checked on
                // its own row.
                if pinned.is_some_and(|v| v != version) {
                    continue;
                }
                // The default is only reachable on versions with no
                // override, so a default that is wrong *only* where an
                // override covers it is not a defect.
                if pinned.is_none()
                    && pack.materials.lookup_id_for(token, Some(version)) != Some(id)
                {
                    continue;
                }
                assert!(
                    ids.binary_search(&id.to_owned()).is_ok(),
                    "the {edition} pack maps `@{token}` to `{id}`, which {edition} {version} \
                     does not declare",
                    edition = edition(pack),
                );
            }
        }
    }
}

#[test]
fn every_id_core_hardcodes_exists_in_every_supported_target() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for version in supported_versions(pack) {
            let ids = pack
                .blocks
                .ids_for(version)
                .unwrap_or_else(|| panic!("{} {version} has no block table", edition(pack)));
            for id in BUILTIN_BLOCK_IDS {
                assert!(
                    ids.binary_search(&(*id).to_owned()).is_ok(),
                    "lowering can emit `{id}` with no catalog to correct it, but {} {version} \
                     does not declare it",
                    edition(pack),
                );
            }
        }
    }
}

/// The plate is the one hardcoded id a pack can redirect, so the promise
/// that makes it safe is that both packs actually declare the token.
///
/// Lowering falls back to a Java spelling when the token is missing, and
/// on Bedrock that fallback is a block the game does not have — so a pack
/// that quietly dropped the row would put it back into `.mcstructure`
/// output with nothing to say so.
#[test]
fn both_packs_declare_the_token_that_redirects_the_plate() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for version in supported_versions(pack) {
            let id = pack
                .materials
                .lookup_id_for("pressure_plate.default", Some(version))
                .unwrap_or_else(|| {
                    panic!(
                        "the {} pack declares no `pressure_plate.default`, so lowering falls \
                         back to a hardcoded Java id",
                        edition(pack),
                    )
                });
            let ids = pack
                .blocks
                .ids_for(version)
                .unwrap_or_else(|| panic!("{} {version} has no block table", edition(pack)));
            assert!(
                ids.binary_search(&id.to_owned()).is_ok(),
                "the {edition} pack plates with `{id}`, which {edition} {version} does not \
                 declare",
                edition = edition(pack),
            );
        }
    }
}

/// The three ids the audit named, pinned by spelling rather than by "the
/// mapping resolves to something".
///
/// The test above would keep passing if a future edit pointed `sign.oak` at
/// some other block that happens to exist. These are the specific
/// substitutions that were wrong, and naming them is what keeps the fix
/// from being undone by a plausible-looking edit.
#[test]
fn the_bedrock_pack_uses_bedrock_spellings_for_the_blocks_that_differ() {
    let bedrock = builtin_bedrock();
    for (token, expected) in [
        ("sign.oak", "minecraft:standing_sign"),
        ("sign.oak_wall", "minecraft:wall_sign"),
        ("pressure_plate.default", "minecraft:wooden_pressure_plate"),
    ] {
        assert_eq!(
            bedrock.materials.lookup_id(token),
            Some(expected),
            "the Bedrock pack must spell `@{token}` the Bedrock way",
        );
    }

    let java = builtin_java();
    for (token, expected) in [
        ("sign.oak", "minecraft:oak_sign"),
        ("sign.oak_wall", "minecraft:oak_wall_sign"),
        ("pressure_plate.default", "minecraft:oak_pressure_plate"),
    ] {
        assert_eq!(
            java.materials.lookup_id(token),
            Some(expected),
            "the Java pack must keep the Java spelling of `@{token}`",
        );
    }
}

/// The Bedrock range spans the flattening wave, so one token needs two
/// spellings inside one pack.
#[test]
fn the_bedrock_pack_respells_stone_bricks_for_the_version_that_predates_it() {
    let bedrock = builtin_bedrock();
    for token in ["floor.stone.smooth", "wall.stone.smooth"] {
        assert_eq!(
            bedrock.materials.lookup_id_for(token, Some("1.21.0")),
            Some("minecraft:stonebrick"),
            "bedrock 1.21.0 predates the `stone_bricks` rename",
        );
        for later in ["1.21.40", "1.21.60"] {
            assert_eq!(
                bedrock.materials.lookup_id_for(token, Some(later)),
                Some("minecraft:stone_bricks"),
                "bedrock {later} is past the rename",
            );
        }
    }

    // Java never had the old spelling, so it needs no override at all.
    assert_eq!(
        builtin_java().materials.overrides().count(),
        0,
        "an override on the Java pack would mean a rename nobody has recorded a reason for",
    );
}

/// Every id the shipped examples actually put in a palette exists in the
/// target they were lowered for.
///
/// The tests above walk the two *lists* — the pack's mappings and
/// `BUILTIN_BLOCK_IDS` — and a list can only be held to what is written in
/// it. This one reads the palettes instead, so an id hardcoded somewhere in
/// lowering and added to no list is caught by the same assertion as
/// everything else. That is the failure mode the whole component exists
/// for: an id nobody registered, written into a structure file the game
/// loads as air.
#[test]
fn every_id_the_examples_intern_exists_in_its_target() {
    let examples = examples();
    assert!(
        examples.len() >= 5,
        "found only {} examples, which is not the shipped set",
        examples.len(),
    );
    for (name, source) in &examples {
        for (pack, pinned_edition) in [
            (builtin_java(), Edition::Java),
            (builtin_bedrock(), Edition::Bedrock),
        ] {
            for version in supported_versions(pack) {
                let module = parse(source).unwrap_or_else(|err| panic!("parse {name}: {err}"));
                let ir = lower(&module);
                let resolution = resolve(&ir, Some(pinned_edition));
                let out = lower_to_block_array(&ir, &resolution, Some(&pack.view(Some(version))));
                let ids = pack
                    .blocks
                    .ids_for(version)
                    .unwrap_or_else(|| panic!("{} {version} has no block table", edition(pack)));
                for structure in out.structures.values() {
                    for entry in &structure.palette.entries {
                        assert!(
                            ids.binary_search(&entry.id).is_ok(),
                            "{name} lowered for {edition} {version} interns `{}`, which that \
                             target does not declare",
                            entry.id,
                            edition = edition(pack),
                        );
                    }
                }
            }
        }
    }
}

/// Every `.crn` under `examples/`, as `(file name, source)`.
fn examples() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
    let mut found: Vec<(String, String)> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("cannot read an entry: {err}"))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            (
                path.file_name().expect("named").to_string_lossy().into(),
                source,
            )
        })
        .collect();
    found.sort();
    found
}

/// A guard on the tests above rather than on the packs: both walk "every
/// supported version", and a pack that supported none would make them pass
/// by iterating nothing.
#[test]
fn both_packs_declare_the_versions_these_tests_expect_to_walk() {
    assert_eq!(
        supported_versions(builtin_java()),
        vec!["1.20.4", "1.21", "1.21.4"],
    );
    assert_eq!(
        supported_versions(builtin_bedrock()),
        vec!["1.21.0", "1.21.40", "1.21.60"],
    );
    for pack in [builtin_java(), builtin_bedrock()] {
        for version in supported_versions(pack) {
            let count = pack.blocks.ids_for(version).map_or(0, <[String]>::len);
            assert!(
                count > 500,
                "{} {version} declares only {count} ids, which is not a vanilla block table",
                edition(pack),
            );
        }
    }
}
