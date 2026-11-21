//! `cairn compile` of a canonical token the target spells another way, per
//! `spec/materials-themes` "Mappings across version and edition".

use std::fs;

use crate::common::{
    Fixture, assert_stream, compile_as, int_tags, nbt_body, stderr_of, string_tags,
};

/// A struct with one painted floor whose `slot floor` binds `@{token}`.
fn source_binding(token: &str) -> String {
    format!("theme t:\n  slot floor -> @{token}\nstruct s size=2x2\n  floor mat_slot=floor\n")
}

/// The block names a Java build of `@{token}` at `target` writes.
fn java_palette(token: &str, target: &str) -> Vec<String> {
    let fixture = Fixture::new(&source_binding(token));
    let out = compile_as(&fixture, "java", target);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
    assert_stream(&stderr_of(&out), &[]);
    // Java 26.3's palette names a block with `id`, an older one's with `Name`.
    let tag = if target == "26.3" { "id" } else { "Name" };
    string_tags(&nbt_body(&fixture.out().join("s.nbt")), tag, false)
}

/// The `.mcstructure` a Bedrock build of `@{token}` at `target` writes.
fn bedrock_structure(token: &str, target: &str) -> Vec<u8> {
    let fixture = Fixture::new(&source_binding(token));
    let out = compile_as(&fixture, "bedrock", target);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
    assert_stream(&stderr_of(&out), &[]);
    fs::read(fixture.out().join("s.mcstructure")).expect("read artifact")
}

/// The block names a Bedrock build of `@{token}` at `target` writes.
fn bedrock_palette(token: &str, target: &str) -> Vec<String> {
    string_tags(&bedrock_structure(token, target), "name", true)
}

#[test]
fn a_renamed_token_is_written_as_each_version_spells_it() {
    assert_eq!(
        java_palette("chain", "1.21.4"),
        ["minecraft:air", "minecraft:chain"]
    );
    assert_eq!(
        java_palette("chain", "26.3"),
        ["minecraft:air", "minecraft:iron_chain"]
    );
    assert_eq!(
        java_palette("iron_chain", "1.21.4"),
        ["minecraft:air", "minecraft:chain"]
    );
    assert_eq!(
        bedrock_palette("chain", "1.26.60"),
        ["minecraft:air", "minecraft:iron_chain"]
    );
}

#[test]
fn a_java_spelling_is_written_as_the_bedrock_block_its_state_map_names() {
    assert_eq!(
        bedrock_palette("oak_sign", "1.21.60"),
        ["minecraft:air", "minecraft:standing_sign"]
    );
    assert_eq!(
        bedrock_palette("stone_bricks", "1.21.0"),
        ["minecraft:air", "minecraft:stonebrick"]
    );
    assert_eq!(
        bedrock_palette("stone_bricks", "1.21.60"),
        ["minecraft:air", "minecraft:stone_bricks"]
    );
}

/// The writer maps the Java block the palette keeps, so its states reach the Bedrock block.
#[test]
fn a_token_written_through_the_state_map_keeps_its_states() {
    let bricks = bedrock_structure("mossy_stone_bricks", "1.21.0");
    assert_eq!(
        string_tags(&bricks, "name", true),
        ["minecraft:air", "minecraft:stonebrick"]
    );
    assert_eq!(string_tags(&bricks, "stone_brick_type", true), ["mossy"]);
    let sign = bedrock_structure("oak_sign[rotation=4]", "1.21.60");
    assert_eq!(
        string_tags(&sign, "name", true),
        ["minecraft:air", "minecraft:standing_sign"]
    );
    assert_eq!(int_tags(&sign, "ground_sign_direction", true), [4]);
    let cauldron = bedrock_structure("cauldron[fluid=water,level=3]", "1.21.60");
    assert_eq!(
        string_tags(&cauldron, "name", true),
        ["minecraft:air", "minecraft:cauldron"]
    );
    assert_eq!(string_tags(&cauldron, "cauldron_liquid", true), ["water"]);
    assert_eq!(int_tags(&cauldron, "fill_level", true), [6]);
}

#[test]
fn a_meaning_token_is_written_as_the_block_it_picks() {
    assert_eq!(
        java_palette("cauldron[fluid=water]", "1.21.4"),
        ["minecraft:air", "minecraft:water_cauldron"]
    );
    assert_eq!(
        bedrock_palette("cauldron[fluid=water]", "1.21.60"),
        ["minecraft:air", "minecraft:cauldron"]
    );
}

#[test]
fn a_raw_block_is_written_as_given_and_not_respelled() {
    let fixture = Fixture::new(
        "theme t:\n  slot floor -> @oak_planks\nstruct s size=3x3\n  floor mat_slot=floor\n  \
         raw_block mat=minecraft:chain at=1,0,1\n",
    );
    let out = compile_as(&fixture, "java", "26.3");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert_stream(&stderr_of(&out), &[(5, 17, "error", "E_UNKNOWN_ID")]);
    assert_eq!(fixture.artifacts(), Vec::<String>::new());
}
