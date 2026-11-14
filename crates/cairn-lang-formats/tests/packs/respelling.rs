//! How a pinned version writes a canonical token its table does not declare, and the block a
//! meaning token picks, per `spec/materials-themes` "Mappings across version and edition".

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, DomainFault, StateDomain, TargetRegistry};
use cairn_lang_core::resolve::SpellingTable;
use cairn_lang_formats::registry::{Respelling, builtin_pack};

use crate::common::supported_versions;

fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    let mut state = BlockState::bare(id);
    for (key, value) in properties {
        state
            .properties
            .insert((*key).to_owned(), (*value).to_owned());
    }
    state.properties.sort_keys();
    state
}

/// The palette id, the written id and the table, when `version` respells the bare `id`.
fn respelled(edition: Edition, version: &str, id: &str) -> Option<(String, String, SpellingTable)> {
    let Respelling {
        state,
        written,
        through,
    } = builtin_pack(edition)
        .view(Some(version))
        .respelling(&BlockState::bare(id))?;
    Some((state.id, written, through))
}

fn spelled(
    palette: &str,
    written: &str,
    through: SpellingTable,
) -> (String, String, SpellingTable) {
    (palette.to_owned(), written.to_owned(), through)
}

#[test]
fn a_rename_is_written_as_the_pinned_version_spells_it() {
    let chain = "minecraft:chain";
    let iron_chain = "minecraft:iron_chain";
    assert_eq!(
        respelled(Edition::Java, "26.3", chain),
        Some(spelled(iron_chain, iron_chain, SpellingTable::Aliases))
    );
    assert_eq!(
        respelled(Edition::Java, "1.21.4", iron_chain),
        Some(spelled(chain, chain, SpellingTable::Aliases))
    );
    assert_eq!(respelled(Edition::Java, "1.21.4", chain), None);
    assert_eq!(
        respelled(Edition::Bedrock, "1.26.60", chain),
        Some(spelled(iron_chain, iron_chain, SpellingTable::Aliases))
    );
}

#[test]
fn a_spelling_no_version_of_the_edition_declares_is_not_respelled() {
    for id in ["minecraft:standing_sign", "minecraft:grass_path"] {
        assert_eq!(respelled(Edition::Java, "1.21.4", id), None, "{id}");
    }
}

/// 1.21.40 spells the light block sixteen ways, so no one spelling succeeds `light_block`.
#[test]
fn a_group_one_version_declares_several_of_is_not_followed() {
    assert_eq!(
        respelled(Edition::Bedrock, "1.21.0", "minecraft:light_block_5"),
        None
    );
}

/// The palette keeps the Java id, so the writer reads its states as that block's.
#[test]
fn a_java_block_bedrock_writes_apart_keeps_its_java_id_in_the_palette() {
    for (version, id, written) in [
        ("1.21.60", "minecraft:oak_sign", "minecraft:standing_sign"),
        ("1.21.0", "minecraft:stone_bricks", "minecraft:stonebrick"),
        (
            "1.21.0",
            "minecraft:mossy_stone_bricks",
            "minecraft:stonebrick",
        ),
        ("1.21.60", "minecraft:water_cauldron", "minecraft:cauldron"),
    ] {
        assert_eq!(
            respelled(Edition::Bedrock, version, id),
            Some(spelled(id, written, SpellingTable::StateMap)),
            "bedrock {version} writes `{id}` as `{written}`",
        );
    }
    assert_eq!(
        respelled(Edition::Bedrock, "1.21.60", "minecraft:stone_bricks"),
        None,
        "1.21.60 declares the id",
    );
}

/// Bedrock keeps a bed's colour and a pot's plant in the block entity, so every bed is one block.
#[test]
fn a_java_block_whose_bedrock_form_another_takes_is_not_respelled() {
    let bedrock = builtin_pack(Edition::Bedrock);
    for version in supported_versions(bedrock) {
        for id in ["minecraft:black_bed", "minecraft:potted_poppy"] {
            assert_eq!(
                respelled(Edition::Bedrock, version, id),
                None,
                "{id} at {version}"
            );
        }
    }
}

#[test]
fn a_split_id_no_one_spelling_succeeds_is_not_respelled() {
    assert_eq!(
        respelled(Edition::Bedrock, "1.21.60", "minecraft:stonebrick"),
        None,
        "1.21.40 split `stonebrick` four ways",
    );
}

#[test]
fn unpinned_a_token_some_version_respells_passes_as_itself() {
    let view = builtin_pack(Edition::Java).view(None);
    let chain = BlockState::bare("minecraft:chain");
    assert_eq!(view.respelled(&chain), Some(chain));
    assert_eq!(
        view.respelled(&BlockState::bare("minecraft:no_such_block")),
        None
    );
    assert_eq!(
        view.respelling(&BlockState::bare("minecraft:iron_chain")),
        None
    );
}

#[test]
fn a_meaning_token_names_the_block_its_value_picks() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let view = builtin_pack(edition).view(None);
        for (fluid, block) in [
            ("water", "minecraft:water_cauldron"),
            ("lava", "minecraft:lava_cauldron"),
            ("powder_snow", "minecraft:powder_snow_cauldron"),
        ] {
            assert_eq!(
                view.meant(&state(
                    "minecraft:cauldron",
                    &[("fluid", fluid), ("level", "2")]
                )),
                Some(state(block, &[("level", "2")])),
                "{edition:?}: `fluid={fluid}`",
            );
        }
        assert_eq!(view.meant(&BlockState::bare("minecraft:cauldron")), None);
        assert_eq!(
            view.meant(&state("minecraft:cauldron", &[("fluid", "milk")])),
            None
        );
    }
}

#[test]
fn a_pinned_target_lists_the_meaning_values_beside_the_blocks_states() {
    let view = builtin_pack(Edition::Java).view(Some("1.21.4"));
    let StateDomain::Checked(outside) =
        view.state_domain(&state("minecraft:cauldron", &[("fluid", "milk")]))
    else {
        panic!("1.21.4 records the cauldron's states");
    };
    let faults: Vec<(&str, &DomainFault)> = outside
        .iter()
        .map(|out| (out.key.as_str(), &out.fault))
        .collect();
    assert_eq!(
        faults,
        [(
            "fluid",
            &DomainFault::ValueOutside {
                values: vec![
                    "water".to_owned(),
                    "lava".to_owned(),
                    "powder_snow".to_owned()
                ],
            }
        )],
    );
}
