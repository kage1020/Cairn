//! Every id a built-in pack can produce exists in its target, on every supported version.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BUILTIN_BLOCK_IDS;
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::common::{examples, lower_source, supported_versions};

fn edition(pack: &RegistryPack) -> &'static str {
    pack.manifest.edition.as_str()
}

#[test]
fn every_material_mapping_resolves_to_an_id_its_target_declares() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
        for version in supported_versions(pack) {
            let ids = pack
                .blocks
                .ids_for(version)
                .unwrap_or_else(|| panic!("{} {version} has no block table", edition(pack)));
            for (token, pinned, id) in pack.materials.mappings() {
                // A pinned mapping is checked only on its own version.
                if pinned.is_some_and(|v| v != version) {
                    continue;
                }
                // A default is checked only where no override replaces it.
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
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
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

/// Without the token, lowering falls back to a Java id that Bedrock does not have.
#[test]
fn both_packs_declare_the_token_that_redirects_the_plate() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
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

#[test]
fn the_bedrock_pack_uses_bedrock_spellings_for_the_blocks_that_differ() {
    let bedrock = builtin_pack(Edition::Bedrock);
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

    let java = builtin_pack(Edition::Java);
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

#[test]
fn the_bedrock_pack_respells_stone_bricks_for_the_version_that_predates_it() {
    let bedrock = builtin_pack(Edition::Bedrock);
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

    assert_eq!(
        builtin_pack(Edition::Java).materials.overrides().count(),
        0,
        "an override on the Java pack would mean a rename nobody has recorded a reason for",
    );
}

#[test]
fn every_id_the_examples_intern_exists_in_its_target() {
    let examples = examples();
    for (name, source) in &examples {
        for (pack, pinned_edition) in [
            (builtin_pack(Edition::Java), Edition::Java),
            (builtin_pack(Edition::Bedrock), Edition::Bedrock),
        ] {
            for version in supported_versions(pack) {
                let out = lower_source(source, pinned_edition, Some(version));
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

/// Guards the sweeps above, which would pass on a pack supporting no version.
#[test]
fn both_packs_declare_the_versions_these_tests_expect_to_walk() {
    assert_eq!(
        supported_versions(builtin_pack(Edition::Java)),
        vec!["1.20.4", "1.21", "1.21.4"],
    );
    assert_eq!(
        supported_versions(builtin_pack(Edition::Bedrock)),
        vec!["1.21.0", "1.21.40", "1.21.60"],
    );
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
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
