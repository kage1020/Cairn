use cairn_lang_core::Edition;
use indexmap::IndexMap;

use super::*;
use crate::registry::builtin_pack;

/// `id[pairs]` as `version` of the built-in Bedrock pack writes it.
fn translate(
    version: &str,
    id: &str,
    pairs: &[(&str, &str)],
) -> Result<StateTranslation, BedrockStateError> {
    let map = builtin_pack(Edition::Bedrock)
        .states_at(version)
        .expect("the built-in pack maps every targetable version");
    let properties: IndexMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    translate_states(
        &map,
        &BlockState {
            id: id.to_owned(),
            properties,
        },
        &[],
    )
}

/// The written states, by name.
fn states(translated: &StateTranslation) -> Vec<(&str, &Tag)> {
    translated
        .states
        .entries
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect()
}

fn dropped(key: &str, value: &str, version: &str, reason: &str) -> DroppedIntent {
    DroppedIntent {
        key: key.to_owned(),
        value: value.to_owned(),
        mc_versions: vec![version.to_owned()],
        reason: reason.to_owned(),
    }
}

const NO_SHAPE: &str = "this Bedrock version has no stair `shape` state; its stairs render \
                        straight, so corners show visual gaps";

#[test]
fn a_block_without_properties_writes_none() {
    let t = translate("1.26.60", "minecraft:oak_planks", &[]).expect("planks");
    assert_eq!(t.id, "minecraft:oak_planks");
    assert_eq!(states(&t), []);
    assert!(!t.waterlogged);
    assert_eq!(t.degraded, []);
}

#[test]
fn a_log_axis_becomes_its_pillar_axis() {
    for (axis, pillar) in [("x", "x"), ("y", "y"), ("z", "z")] {
        let t = translate("1.21.60", "minecraft:oak_log", &[("axis", axis)]).expect("log");
        assert_eq!(
            states(&t),
            [("pillar_axis", &Tag::String(pillar.to_owned()))],
            "axis={axis}",
        );
    }
}

#[test]
fn a_bare_entry_is_the_java_blocks_default_state() {
    let t = translate("1.21.60", "minecraft:oak_stairs", &[]).expect("stairs");
    assert_eq!(
        states(&t),
        [
            ("upside_down_bit", &Tag::Byte(0)),
            ("weirdo_direction", &Tag::Int(3))
        ],
    );
    assert_eq!(t.degraded, []);
}

#[test]
fn each_stair_facing_has_its_own_weirdo_direction() {
    for (facing, weirdo) in [("east", 0), ("west", 1), ("south", 2), ("north", 3)] {
        let t = translate(
            "1.21.60",
            "minecraft:dark_oak_stairs",
            &[("facing", facing), ("half", "top"), ("shape", "straight")],
        )
        .expect("stairs");
        assert_eq!(
            states(&t),
            [
                ("upside_down_bit", &Tag::Byte(1)),
                ("weirdo_direction", &Tag::Int(weirdo))
            ],
            "facing={facing}",
        );
    }
}

#[test]
fn a_stair_shape_is_lost_where_the_version_has_no_corner_state() {
    let corner = [
        ("facing", "south"),
        ("half", "top"),
        ("shape", "outer_left"),
    ];
    let t = translate("1.21.60", "minecraft:oak_stairs", &corner).expect("stairs");
    assert_eq!(
        states(&t),
        [
            ("upside_down_bit", &Tag::Byte(1)),
            ("weirdo_direction", &Tag::Int(2))
        ],
    );
    assert_eq!(
        t.degraded,
        [dropped("shape", "outer_left", "1.21.60", NO_SHAPE)]
    );

    let t = translate("1.26.60", "minecraft:oak_stairs", &corner).expect("stairs");
    assert_eq!(
        states(&t),
        [
            ("minecraft:corner", &Tag::String("outer_left".to_owned())),
            ("upside_down_bit", &Tag::Byte(1)),
            ("weirdo_direction", &Tag::Int(2)),
        ],
    );
    assert_eq!(t.degraded, []);
}

#[test]
fn a_dropped_state_at_its_default_is_no_loss() {
    let t = translate(
        "1.21.60",
        "minecraft:oak_stairs",
        &[("facing", "south"), ("shape", "straight")],
    )
    .expect("stairs");
    assert_eq!(t.degraded, []);
}

#[test]
fn a_state_bedrock_works_out_itself_is_lost_only_where_a_member_wrote_it() {
    let connected = [("east", "true"), ("north", "true")];
    let t = translate("1.21.60", "minecraft:oak_fence", &connected).expect("fence");
    assert_eq!(t.id, "minecraft:oak_fence");
    assert_eq!(states(&t), []);
    let neighbours = "Bedrock works the connection out from the neighbouring blocks";
    assert_eq!(
        t.degraded,
        ["east", "north"].map(|key| dropped(
            key,
            "true",
            "1.21.60",
            &format!("the written `{key}=true` is not kept: {neighbours}"),
        )),
    );
    let map = builtin_pack(Edition::Bedrock)
        .states_at("1.21.60")
        .expect("the built-in pack maps every targetable version");
    let fence = BlockState {
        id: "minecraft:oak_fence".to_owned(),
        properties: connected
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    };
    let east = ["east".to_owned()];
    let t = translate_states(&map, &fence, &east).expect("fence");
    assert_eq!(
        t.degraded,
        [dropped(
            "north",
            "true",
            "1.21.60",
            &format!("the written `north=true` is not kept: {neighbours}"),
        )],
    );
    let both = ["east".to_owned(), "north".to_owned()];
    assert_eq!(
        translate_states(&map, &fence, &both)
            .expect("fence")
            .degraded,
        []
    );

    let t = translate("1.26.60", "minecraft:oak_fence", &connected).expect("fence");
    assert_eq!(
        states(&t),
        [
            ("minecraft:connection_east", &Tag::Byte(1)),
            ("minecraft:connection_north", &Tag::Byte(1)),
            ("minecraft:connection_south", &Tag::Byte(0)),
            ("minecraft:connection_west", &Tag::Byte(0)),
        ],
    );
    assert_eq!(t.degraded, []);
}

#[test]
fn a_state_bedrock_spells_as_another_block_changes_the_id() {
    let t = translate("1.26.60", "minecraft:oak_slab", &[("type", "double")]).expect("slab");
    assert_eq!(t.id, "minecraft:oak_double_slab");
    assert_eq!(
        states(&t),
        [("minecraft:vertical_half", &Tag::String("bottom".to_owned()))],
    );

    let t = translate("1.26.60", "minecraft:oak_slab", &[("type", "top")]).expect("slab");
    assert_eq!(t.id, "minecraft:oak_slab");
    assert_eq!(
        states(&t),
        [("minecraft:vertical_half", &Tag::String("top".to_owned()))],
    );
}

#[test]
fn a_waterlogged_entry_holds_water_beside_its_states() {
    let t = translate(
        "1.26.60",
        "minecraft:oak_stairs",
        &[("facing", "east"), ("waterlogged", "true")],
    )
    .expect("stairs");
    assert!(t.waterlogged);
    assert_eq!(
        states(&t),
        [
            ("minecraft:corner", &Tag::String("none".to_owned())),
            ("upside_down_bit", &Tag::Byte(0)),
            ("weirdo_direction", &Tag::Int(0)),
        ],
    );
}

#[test]
fn a_bedrock_spelling_reads_the_states_of_the_java_block_mapped_onto_it() {
    let t = translate("1.26.60", "minecraft:standing_sign", &[("rotation", "4")]).expect("sign");
    assert_eq!(t.id, "minecraft:standing_sign");
    assert_eq!(states(&t), [("ground_sign_direction", &Tag::Int(4))]);
}

#[test]
fn an_id_both_editions_spell_reads_as_its_own_block_unless_another_takes_its_states() {
    let standing = translate("1.26.60", "minecraft:redstone_torch", &[]).expect("standing");
    assert_eq!(
        (standing.id.as_str(), states(&standing)),
        (
            "minecraft:redstone_torch",
            vec![("torch_facing_direction", &Tag::String("top".to_owned()))],
        ),
    );
    let wall = translate("1.26.60", "minecraft:redstone_torch", &[("facing", "east")])
        .expect("the wall torch takes `facing`");
    assert_eq!(
        (wall.id.as_str(), states(&wall)),
        (
            "minecraft:redstone_torch",
            vec![("torch_facing_direction", &Tag::String("west".to_owned()))],
        ),
    );
    let err = translate("1.26.60", "minecraft:redstone_torch", &[("facing", "up")])
        .expect_err("no torch faces up");
    assert_eq!(
        err,
        BedrockStateError::UnknownValue {
            id: "minecraft:redstone_torch".to_owned(),
            key: "facing".to_owned(),
            value: "up".to_owned(),
            valid: "north, south, west, east".to_owned(),
        },
    );
}

#[test]
fn a_bedrock_spelling_of_several_java_blocks_is_written_bare_or_refused() {
    let t = translate("1.21.0", "minecraft:stonebrick", &[]).expect("bare");
    assert_eq!(
        (t.id.as_str(), states(&t)),
        ("minecraft:stonebrick", vec![])
    );

    let err = translate("1.21.0", "minecraft:stonebrick", &[("variant", "mossy")])
        .expect_err("no Java stone brick has a variant");
    assert_eq!(
        err,
        BedrockStateError::UnknownProperty {
            id: "minecraft:stonebrick".to_owned(),
            key: "variant".to_owned(),
            valid: String::new(),
        },
    );
}

#[test]
fn a_block_no_java_block_maps_onto_takes_no_canonical_state() {
    let t = translate("1.26.60", "minecraft:frame", &[]).expect("bare");
    assert_eq!(states(&t), []);
    let err =
        translate("1.26.60", "minecraft:frame", &[("facing", "north")]).expect_err("unmapped");
    assert_eq!(
        err.to_string(),
        "block `minecraft:frame[facing=north]` has no form at bedrock 1.26.60: no Java block maps \
         onto `minecraft:frame` there, so the canonical vocabulary names none of its states. Fix: \
         pass a `--target` whose state map carries these states, bind the member's material to \
         another block, or compile with `--edition java`",
    );
}

#[test]
fn a_state_the_versions_palette_refuses_is_unmapped_there_alone() {
    let err = translate("1.21.40", "minecraft:cherry_wood", &[("axis", "x")]).expect_err("refused");
    assert_eq!(
        err,
        BedrockStateError::Unmapped {
            id: "minecraft:cherry_wood".to_owned(),
            properties: "axis=x".to_owned(),
            mc_version: "1.21.40".to_owned(),
            reason: "its palette does not declare the states GeyserMC maps \
                     `minecraft:cherry_wood` to, so the pack carries no form for them"
                .to_owned(),
        },
    );
    let t = translate("1.21.60", "minecraft:cherry_wood", &[("axis", "x")]).expect("mapped");
    assert_eq!(t.id, "minecraft:cherry_wood");
}

#[test]
fn a_property_the_java_block_lacks_is_a_leak() {
    let err = translate("1.21.60", "minecraft:oak_stairs", &[("axis", "x")]).expect_err("leak");
    assert_eq!(
        err,
        BedrockStateError::UnknownProperty {
            id: "minecraft:oak_stairs".to_owned(),
            key: "axis".to_owned(),
            valid: "facing, half, shape, waterlogged".to_owned(),
        },
    );
    assert_eq!(
        err.to_string(),
        "block `minecraft:oak_stairs` carries blockstate `axis`, which the Java block does not \
         have. Valid: facing, half, shape, waterlogged. Fix: remove it from the source blockstate; \
         `--edition java` would write it unchanged, since Java has no such state either",
    );
}

#[test]
fn a_value_only_a_later_versions_map_holds_is_unmapped_before_it() {
    let err = translate(
        "1.21.60",
        "minecraft:noteblock",
        &[("instrument", "trumpet")],
    )
    .expect_err("unmapped");
    assert_eq!(
        err,
        BedrockStateError::Unmapped {
            id: "minecraft:noteblock".to_owned(),
            properties: "instrument=trumpet".to_owned(),
            mc_version: "1.21.60".to_owned(),
            reason: "its state map there is read from a Java release whose \
                     `minecraft:note_block` has no `instrument=trumpet`, which the map of 1.26.60 \
                     holds"
                .to_owned(),
        },
    );
    let t = translate(
        "1.26.60",
        "minecraft:noteblock",
        &[("instrument", "trumpet")],
    )
    .expect("mapped");
    assert_eq!(t.id, "minecraft:noteblock");
}

#[test]
fn a_value_outside_the_java_domain_is_a_leak() {
    let err = translate("1.21.60", "minecraft:oak_stairs", &[("facing", "up")]).expect_err("leak");
    assert_eq!(
        err.to_string(),
        "block `minecraft:oak_stairs` has `facing=up`, which is not a valid Java `facing`. Valid \
         `facing`: north, south, west, east. Fix: correct the source blockstate to one of those; \
         `--edition java` would write the same value unchanged, since Java has no such `facing` \
         either",
    );
}
