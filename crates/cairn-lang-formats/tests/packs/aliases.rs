//! The built-in packs' alias groups, checked against both editions' tables at once.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::TargetRegistry;
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::common::supported_versions;

/// Whether either built-in pack declares `id` in any version it supports.
fn known_to_some_edition(id: &str) -> bool {
    [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)]
        .into_iter()
        .any(|pack| pack.blocks.declared_by_some_version(id) == Some(true))
}

/// The ids `edition version` spells the same block as `id`.
fn aliases_at(pack: &RegistryPack, version: &str, id: &str) -> Vec<String> {
    pack.view(Some(version)).aliases_for(id)
}

/// The loader accepts a mistyped foreign spelling, such as `standing_sgn` beside `oak_sign`.
#[test]
fn every_spelling_is_an_id_some_supported_version_declares() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
        for group in pack.aliases.groups() {
            for spelling in group {
                assert!(
                    known_to_some_edition(spelling),
                    "the {} pack groups `{spelling}`, which no supported version of either \
                     edition declares",
                    pack.manifest.edition.as_str(),
                );
            }
        }
    }
}

/// A group whose members every version declares loads, but no refused id ever reaches it.
#[test]
fn every_group_answers_somewhere_in_its_own_range() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
        for group in pack.aliases.groups() {
            let answers = supported_versions(pack).into_iter().any(|version| {
                let declared = pack.blocks.ids_for(version).unwrap_or(&[]);
                group.iter().any(|spelling| {
                    !declared.contains(spelling) && !aliases_at(pack, version, spelling).is_empty()
                })
            });
            assert!(
                answers,
                "the {} pack's group [{}] answers no version it supports about a spelling that \
                 version lacks, so nothing can ever read it",
                pack.manifest.edition.as_str(),
                group.join(", "),
            );
        }
    }
}

/// Renames seven and eight edits apart, out of the typo search's reach.
#[test]
fn the_java_spellings_a_bedrock_build_refuses_are_answered() {
    let bedrock = builtin_pack(Edition::Bedrock);
    for (wrote, expected) in [
        ("minecraft:oak_sign", "minecraft:standing_sign"),
        (
            "minecraft:oak_pressure_plate",
            "minecraft:wooden_pressure_plate",
        ),
    ] {
        for version in supported_versions(bedrock) {
            assert_eq!(
                aliases_at(bedrock, version, wrote),
                [expected.to_owned()],
                "bedrock {version} spells `{wrote}` `{expected}`",
            );
        }
    }
}

#[test]
fn a_split_answers_with_every_id_the_version_has() {
    let bedrock = builtin_pack(Edition::Bedrock);
    assert_eq!(
        aliases_at(bedrock, "1.21.0", "minecraft:light"),
        ["minecraft:light_block".to_owned()],
        "1.21.0 predates the split and has the one id",
    );
    let after = aliases_at(bedrock, "1.21.60", "minecraft:light");
    assert_eq!(after.len(), 16, "one id per light level, got {after:?}");
    assert!(after.contains(&"minecraft:light_block_0".to_owned()));
    assert!(!after.contains(&"minecraft:light_block".to_owned()));
}

#[test]
fn a_rename_inside_bedrocks_own_range_is_answered_both_ways() {
    let bedrock = builtin_pack(Edition::Bedrock);
    assert_eq!(
        aliases_at(bedrock, "1.21.0", "minecraft:stone_bricks"),
        ["minecraft:stonebrick".to_owned()],
        "1.21.0 spells stone bricks the old way",
    );
    assert!(
        aliases_at(bedrock, "1.21.60", "minecraft:stonebrick")
            .contains(&"minecraft:stone_bricks".to_owned()),
        "and 1.21.60 spells the old id's block the new way",
    );
}

#[test]
fn the_java_pack_answers_a_bedrock_spelling() {
    let java = builtin_pack(Edition::Java);
    for version in supported_versions(java) {
        assert_eq!(
            aliases_at(java, version, "minecraft:standing_sign"),
            ["minecraft:oak_sign".to_owned()],
            "java {version} spells the same block `oak_sign`",
        );
    }
}

/// Not equality: the Bedrock pack also groups blocks Java has never had, which a Java pack refuses.
#[test]
fn the_java_groups_are_a_subset_of_the_bedrock_ones() {
    let bedrock: Vec<Vec<String>> = builtin_pack(Edition::Bedrock)
        .aliases
        .groups()
        .map(<[String]>::to_vec)
        .collect();
    for group in builtin_pack(Edition::Java).aliases.groups() {
        assert!(
            bedrock.iter().any(|other| other == group),
            "the Java pack's group [{}] is in no Bedrock group, so the two packs disagree \
             about a rename that has no direction",
            group.join(", "),
        );
    }
}

#[test]
fn an_id_no_group_names_is_answered_with_nothing() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
        for version in supported_versions(pack) {
            assert!(
                aliases_at(pack, version, "minecraft:totally_not_a_block").is_empty(),
                "{} {version} named an alias for an id no group carries",
                pack.manifest.edition.as_str(),
            );
        }
    }
}
