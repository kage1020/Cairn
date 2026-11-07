//! The states and versions the extracted block tables give an id, read from the game's own data.

use cairn_lang_core::Edition;
use cairn_lang_formats::registry::{StateValue, builtin_pack};

fn strings(values: &[&str]) -> Vec<StateValue> {
    values
        .iter()
        .map(|v| StateValue::Str((*v).to_owned()))
        .collect()
}

#[test]
fn a_java_block_carries_each_property_with_its_values_in_the_games_order() {
    let blocks = builtin_pack(Edition::Java).blocks();
    let stairs = blocks
        .states_of("1.21.4", "minecraft:oak_stairs")
        .expect("oak_stairs has states on Java 1.21.4");
    let properties: Vec<&str> = stairs.properties().map(|(name, _)| name).collect();
    assert_eq!(properties, ["facing", "half", "shape", "waterlogged"]);
    assert_eq!(
        stairs.domain("facing"),
        Some(strings(&["north", "south", "west", "east"]).as_slice()),
    );
    assert_eq!(
        stairs.domain("waterlogged"),
        Some(strings(&["true", "false"]).as_slice()),
    );
    assert_eq!(stairs.domain("axis"), None);
}

#[test]
fn a_bedrock_block_carries_typed_values() {
    let blocks = builtin_pack(Edition::Bedrock).blocks();
    let stairs = blocks
        .states_of("1.21.60", "minecraft:oak_stairs")
        .expect("oak_stairs has states on Bedrock 1.21.60");
    let properties: Vec<&str> = stairs.properties().map(|(name, _)| name).collect();
    assert_eq!(properties, ["upside_down_bit", "weirdo_direction"]);
    assert_eq!(
        stairs.domain("upside_down_bit"),
        Some([StateValue::Bool(false), StateValue::Bool(true)].as_slice()),
    );
    assert_eq!(
        stairs.domain("weirdo_direction"),
        Some((0..4).map(StateValue::Int).collect::<Vec<_>>().as_slice()),
    );
}

#[test]
fn a_block_without_properties_has_empty_states_rather_than_none() {
    for (pack, version) in [
        (builtin_pack(Edition::Java), "26.3"),
        (builtin_pack(Edition::Bedrock), "1.26.60"),
    ] {
        let planks = pack
            .blocks()
            .states_of(version, "minecraft:oak_planks")
            .expect("oak_planks is declared");
        assert!(planks.is_empty(), "{version}");
    }
}

#[test]
fn a_block_whose_states_change_between_versions_reads_each_versions_own() {
    let blocks = builtin_pack(Edition::Java).blocks();
    let before = blocks
        .states_of("1.21.4", "minecraft:creaking_heart")
        .expect("creaking_heart has states on 1.21.4");
    let after = blocks
        .states_of("26.3", "minecraft:creaking_heart")
        .expect("creaking_heart has states on 26.3");
    assert_ne!(before, after);
    assert!(before.domain("active").is_some());
    assert!(after.domain("active").is_none());
}

#[test]
fn since_and_until_name_the_first_and_last_carried_versions_that_declare_an_id() {
    let java = builtin_pack(Edition::Java).blocks();
    assert_eq!(java.since("minecraft:stone"), Some("1.20.4"));
    assert_eq!(java.until("minecraft:stone"), Some("26.3"));
    assert_eq!(java.since("minecraft:pale_oak_planks"), Some("1.21.4"));
    assert_eq!(java.since("minecraft:iron_chain"), Some("26.3"));
    assert_eq!(java.until("minecraft:chain"), Some("1.21.4"));
    assert_eq!(java.since("minecraft:no_such_block"), None);
    assert_eq!(java.until("minecraft:no_such_block"), None);

    let bedrock = builtin_pack(Edition::Bedrock).blocks();
    assert_eq!(bedrock.until("minecraft:stonebrick"), Some("1.21.0"));
    assert_eq!(bedrock.since("minecraft:stone_bricks"), Some("1.21.40"));
}
