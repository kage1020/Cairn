//! How a pinned version writes a canonical token its table does not declare, and the block a
//! meaning token picks, per `spec/materials-themes` "Mappings across version and edition".

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockState, DomainFault, IdStanding, StateDomain, TargetRegistry,
};
use cairn_lang_core::resolve::SpellingTable;
use cairn_lang_formats::registry::{Respelling, builtin_pack};

use crate::common::{lower_source, supported_versions};

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
    let view = builtin_pack(edition).view(Some(version));
    let token = BlockState::bare(id);
    match (view.respelled(&token), view.respelling(&token)) {
        (Some(palette), Some(Respelling { written, through })) => {
            Some((palette.id, written, through))
        }
        (None, None) => None,
        (palette, respelling) => panic!(
            "{edition:?} {version} `{id}`: the palette holds {palette:?} but writes {respelling:?}"
        ),
    }
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

/// Bedrock 1.21.0 spells the dandelion `yellow_flower`, which its alias group and its state map
/// both answer; the alias table is asked first.
#[test]
fn a_rename_is_followed_before_the_state_map() {
    let yellow_flower = "minecraft:yellow_flower";
    assert_eq!(
        respelled(Edition::Bedrock, "1.21.0", "minecraft:dandelion"),
        Some(spelled(
            yellow_flower,
            yellow_flower,
            SpellingTable::Aliases
        ))
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
fn a_version_that_refuses_the_states_writes_no_other_id() {
    let view = builtin_pack(Edition::Bedrock).view(Some("1.21.0"));
    let sign = state("minecraft:oak_sign", &[("rotation", "99")]);
    assert_eq!(
        view.respelled(&sign),
        Some(sign.clone()),
        "the palette keeps it for `E_STATE_DOMAIN` to refuse"
    );
    assert_eq!(view.respelling(&sign), None);
}

#[test]
fn a_meaning_token_a_version_declares_the_block_of_is_written_through_the_meaning_table() {
    let view = builtin_pack(Edition::Java).view(Some("1.21.4"));
    assert_eq!(
        view.respelling(&state("minecraft:cauldron", &[("fluid", "water")])),
        Some(Respelling {
            written: "minecraft:water_cauldron".to_owned(),
            through: SpellingTable::Meanings,
        }),
    );
    assert_eq!(
        view.respelling(&state("minecraft:cauldron", &[("fluid", "milk")])),
        None
    );
    assert_eq!(
        view.respelling(&state(
            "minecraft:cauldron",
            &[("fluid", "water"), ("level", "9")]
        )),
        None,
        "1.21.4 refuses `level=9`"
    );
}

#[test]
fn a_token_some_version_writes_under_another_id_stands_as_respelled() {
    let bedrock = builtin_pack(Edition::Bedrock);
    for (id, standing) in [
        ("minecraft:standing_sign", IdStanding::Declared),
        ("minecraft:chain", IdStanding::Declared),
        ("minecraft:oak_sign", IdStanding::Respelled),
        ("minecraft:water_cauldron", IdStanding::Respelled),
        ("minecraft:black_bed", IdStanding::Unknown),
        ("minecraft:no_such_block", IdStanding::Unknown),
    ] {
        assert_eq!(bedrock.id_standing(id), standing, "{id}");
    }
    let java = builtin_pack(Edition::Java);
    assert_eq!(java.id_standing("minecraft:chain"), IdStanding::Declared);
    assert_eq!(
        java.id_standing("minecraft:standing_sign"),
        IdStanding::Unknown
    );
}

#[test]
fn unpinned_no_token_is_respelled() {
    let view = builtin_pack(Edition::Bedrock).view(None);
    let sign = BlockState::bare("minecraft:oak_sign");
    assert_eq!(view.respelled(&sign), None);
    assert_eq!(view.respelling(&sign), None);
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

/// Each pair of `domain` outside it, as its key and fault.
fn faults(domain: StateDomain) -> Vec<(String, DomainFault)> {
    let StateDomain::Checked(outside) = domain else {
        panic!("the pinned target records the block's states, got {domain:?}");
    };
    outside
        .into_iter()
        .map(|out| (out.key, out.fault))
        .collect()
}

#[test]
fn a_pinned_target_lists_the_meaning_values_beside_a_tokens_states() {
    for (edition, version) in [(Edition::Java, "1.21.4"), (Edition::Bedrock, "1.21.60")] {
        let view = builtin_pack(edition).view(Some(version));
        let milk = state("minecraft:cauldron", &[("fluid", "milk")]);
        assert_eq!(
            faults(view.token_state_domain(&milk)),
            [(
                "fluid".to_owned(),
                DomainFault::ValueOutside {
                    values: vec![
                        "water".to_owned(),
                        "lava".to_owned(),
                        "powder_snow".to_owned()
                    ],
                }
            )],
            "{edition:?}",
        );
    }
}

/// A raw block is not read through the meaning table, so its block has no such state.
#[test]
fn a_block_id_is_checked_without_the_meaning_token() {
    let view = builtin_pack(Edition::Java).view(Some("1.21.4"));
    assert_eq!(
        faults(view.state_domain(&state("minecraft:cauldron", &[("fluid", "water")]))),
        [(
            "fluid".to_owned(),
            DomainFault::UnknownProperty {
                properties: Vec::new()
            }
        )],
    );
}

/// Each finding of `source` lowered at `edition` `version`, as its code, the text it points at
/// and its first line.
fn findings<'s>(
    source: &'s str,
    edition: Edition,
    version: &str,
) -> Vec<(&'static str, &'s str, String)> {
    lower_source(source, edition, Some(version))
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), &source[d.span.clone()], d.primary.clone()))
        .collect()
}

#[test]
fn a_raw_block_carrying_a_meaning_token_is_refused_as_a_state_its_block_lacks() {
    for member in [
        "raw_block mat=minecraft:cauldron[fluid=water] at=1,0,1",
        "raw_fill mat=minecraft:cauldron[fluid=water] from=0,0,0 to=1,0,1",
    ] {
        let source = format!("struct s size=3x3\n  {member}\n");
        assert_eq!(
            findings(&source, Edition::Java, "1.21.4"),
            [(
                "E_STATE_DOMAIN",
                "minecraft:cauldron[fluid=water]",
                "`minecraft:cauldron` has no state `fluid` at java 1.21.4; it takes no state"
                    .to_owned()
            )],
            "{member}",
        );
        assert_eq!(
            findings(&source, Edition::Bedrock, "1.21.60"),
            [(
                "E_STATE_DOMAIN",
                "minecraft:cauldron[fluid=water]",
                "`minecraft:cauldron` has no state `fluid` at bedrock 1.21.60; its states are level"
                    .to_owned()
            )],
            "{member}",
        );
    }
}

#[test]
fn a_meaning_value_the_row_does_not_list_is_refused_with_the_values_it_does() {
    let source = "theme t:\n  slot floor -> @cauldron[fluid=milk]\nstruct s size=2x2\n  floor mat_slot=floor\n";
    for (edition, version) in [(Edition::Java, "1.21.4"), (Edition::Bedrock, "1.21.60")] {
        assert_eq!(
            findings(source, edition, version),
            [(
                "E_STATE_DOMAIN",
                "@cauldron[fluid=milk]",
                format!(
                    "`minecraft:cauldron` takes no `fluid=milk` at {} {version}; `fluid` takes \
                     water, lava, powder_snow",
                    edition.as_str()
                )
            )],
            "{edition:?}",
        );
    }
}
