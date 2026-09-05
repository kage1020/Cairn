//! What the built-in packs' alias groups promise about the ids in them.
//!
//! The `blocks` tables say which ids a version has; the `aliases` groups
//! say which of them are the same block under another name. Nothing inside
//! the loader can check the second half against reality — a group is
//! refused only when *no* member is an id its own edition declares, which
//! leaves a mistyped foreign spelling riding along silently, doing nothing
//! and looking like data.
//!
//! So the properties are held here, against both packs at once: every
//! spelling has to be an id some supported version of some edition
//! declares, and the renames the component exists for have to come out of
//! it.

use cairn_lang_core::block_array::TargetRegistry;
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

/// Every version the pack can build for, as the `--target` strings a user
/// types. The `targetable` rows only — an ordering row has no block table
/// and so declares nothing.
fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.data_versions
        .versions
        .iter()
        .filter(|e| e.targetable)
        .map(|e| e.mc_version.as_str())
        .collect()
}

/// Whether either built-in pack declares `id` in any version it supports.
fn known_to_some_edition(id: &str) -> bool {
    [builtin_java(), builtin_bedrock()]
        .into_iter()
        .any(|pack| pack.blocks.declared_by_some_version(id) == Some(true))
}

/// The ids `edition version` spells the same block as `id`.
fn aliases_at(pack: &RegistryPack, version: &str, id: &str) -> Vec<String> {
    pack.view(Some(version)).aliases_for(id)
}

/// A spelling no edition has ever declared is a typo in the pack, not a
/// rename.
///
/// The loader cannot catch this: it refuses a group only when *nothing* in
/// it is an id this pack's own tables have, and a cross-edition row is
/// supposed to carry a spelling this edition does not use. `standing_sgn`
/// beside `oak_sign` would load, answer nothing, and read as data.
#[test]
fn every_spelling_is_an_id_some_supported_version_declares() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for group in pack.aliases.groups() {
            for spelling in group {
                assert!(
                    known_to_some_edition(spelling),
                    "the {} pack groups `{spelling}`, which no supported version of either \
                     edition declares",
                    pack.manifest.edition.label(),
                );
            }
        }
    }
}

/// Every group answers on at least one version the pack can build for.
///
/// The loader's rule is weaker — some version declares some member — and
/// that admits a group whose members are all declared by every version,
/// which can never be reached: an alias is asked for only about an id the
/// target refused.
#[test]
fn every_group_answers_somewhere_in_its_own_range() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for group in pack.aliases.groups() {
            let answers = supported_versions(pack).into_iter().any(|version| {
                group
                    .iter()
                    .any(|spelling| !aliases_at(pack, version, spelling).is_empty())
            });
            assert!(
                answers,
                "the {} pack's group [{}] answers on no version it supports, so nothing can \
                 ever read it",
                pack.manifest.edition.label(),
                group.join(", "),
            );
        }
    }
}

/// The renames from the report this component was written for, each asked
/// of the target that refuses the id.
///
/// These are the cases a distance search cannot reach — seven and eight
/// edits — and the reason a table exists at all rather than a wider
/// threshold.
#[test]
fn the_java_spellings_a_bedrock_build_refuses_are_answered() {
    let bedrock = builtin_bedrock();
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

/// A rename that splits one id into a family answers with the family.
///
/// Bedrock 1.21.40 replaced `light_block` with sixteen ids, one per light
/// level. Which one an author wants is theirs to pick, so all sixteen are
/// returned rather than a guess.
#[test]
fn a_split_answers_with_every_id_the_version_has() {
    let bedrock = builtin_bedrock();
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

/// The same table answers a rename inside one edition's own range, which
/// is the half a Java-as-the-base alias map would not have covered.
#[test]
fn a_rename_inside_bedrocks_own_range_is_answered_both_ways() {
    let bedrock = builtin_bedrock();
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

/// A group carries no direction, so the Java pack answers the mirror of
/// what the Bedrock pack does.
#[test]
fn the_java_pack_answers_a_bedrock_spelling() {
    let java = builtin_java();
    for version in supported_versions(java) {
        assert_eq!(
            aliases_at(java, version, "minecraft:standing_sign"),
            ["minecraft:oak_sign".to_owned()],
            "java {version} spells the same block `oak_sign`",
        );
    }
}

/// An id that is not a block anywhere gets no alias, so the refusal keeps
/// saying it has no candidate rather than reaching for the nearest group.
#[test]
fn an_id_no_group_names_is_answered_with_nothing() {
    for pack in [builtin_java(), builtin_bedrock()] {
        for version in supported_versions(pack) {
            assert!(
                aliases_at(pack, version, "minecraft:totally_not_a_block").is_empty(),
                "{} {version} named an alias for an id no group carries",
                pack.manifest.edition.label(),
            );
        }
    }
}
