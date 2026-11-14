//! What a sign and a bed keep beside their blocks, as each writer saves it (`spec/entities` "Two
//! tiers").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockEntityData};
use cairn_lang_formats::bedrock_structure::{BedrockStructureError, build_mcstructure_tag};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{JavaStructureError, build_structure_tag};
use cairn_lang_nbt::tag::Tag;

use crate::common::{lower_source, position_data, snbt, states_for};

/// A 5x5 room with a sign on the inside of its front wall at (2, 1, 3) and a blue bed against its
/// back wall, its head at (2, 1, 1) and its foot at (2, 1, 2).
const ROOM: &str = "theme t:\n  slot wall -> @cobblestone\n  slot sheet -> @bed.blue\n\n\
                    struct s size=5x5\n  walls mat_slot=wall height=3\n  \
                    sign at=inside.front offset=2 y=1 text=[\"The\", \"Inn\"] back=\"Closed\" \
                    glowing=true\n  bed at=inside.back offset=2 y=1 mat_slot=sheet\n";

/// `source`'s one struct, lowered for `edition` at `mc_version` without a finding.
fn lowered(source: &str, edition: Edition, mc_version: &str) -> BlockArray {
    let mut ir = lower_source(source, edition, Some(mc_version));
    assert_eq!(ir.diagnostics, []);
    ir.structures.swap_remove("struct::s").expect("struct s")
}

/// Each `blocks` entry of `array`'s Java structure at `mc_version` that carries `nbt`, as its
/// position and that `nbt` in SNBT.
fn java_nbt(array: &BlockArray, mc_version: &str) -> Vec<(String, String)> {
    let target = resolve_java_target(mc_version).expect("a shipped target");
    let root = build_structure_tag(array, &target).expect("build");
    let Some(Tag::List(blocks)) = root.entries.get("blocks") else {
        panic!("blocks is not a List");
    };
    blocks
        .items
        .iter()
        .filter_map(|block| match block {
            Tag::Compound(block) => Some((
                snbt(block.entries.get("pos")?),
                snbt(block.entries.get("nbt")?),
            )),
            other => panic!("block entry: {other:?}"),
        })
        .collect()
}

/// `array`'s `block_position_data` at Bedrock `mc_version`, each entry's key and its SNBT.
fn bedrock_data(array: &BlockArray, mc_version: &str) -> Vec<(String, String)> {
    let target = resolve_bedrock_target(mc_version).expect("a shipped target");
    let (root, notes) = build_mcstructure_tag(array, &target, &states_for(&target)).expect("build");
    assert_eq!(notes, []);
    position_data(&root)
}

/// A Java sign's block entity whose faces hold `front` and `back`, each line as written there.
fn java_sign(front: [&str; 4], back: [&str; 4]) -> String {
    let face = |lines: [&str; 4]| {
        let messages: Vec<String> = lines.iter().map(|line| format!("{line:?}")).collect();
        format!(
            "{{messages:[{}],color:\"black\",has_glowing_text:1b}}",
            messages.join(",")
        )
    };
    format!(
        "{{id:\"minecraft:sign\",front_text:{},back_text:{},is_waxed:0b}}",
        face(front),
        face(back)
    )
}

#[test]
fn a_java_sign_writes_its_text_as_json_before_1_21_5_and_as_text_from_it() {
    let array = lowered(ROOM, Edition::Java, "1.21.4");
    assert_eq!(
        java_nbt(&array, "1.21.4"),
        [(
            "[2,1,3]".to_owned(),
            java_sign(
                [r#""The""#, r#""Inn""#, r#""""#, r#""""#],
                [r#""Closed""#, r#""""#, r#""""#, r#""""#],
            ),
        )],
    );
    let array = lowered(ROOM, Edition::Java, "26.3");
    assert_eq!(
        java_nbt(&array, "26.3"),
        [(
            "[2,1,3]".to_owned(),
            java_sign(["The", "Inn", "", ""], ["Closed", "", "", ""]),
        )],
    );
}

#[test]
fn a_java_sign_line_is_escaped_as_a_json_string() {
    let source = ROOM.replace(r#"back="Closed""#, r#"back="C:\inn""#);
    let array = lowered(&source, Edition::Java, "1.21.4");
    let nbt = java_nbt(&array, "1.21.4");
    assert_eq!(
        nbt[0].1,
        java_sign(
            [r#""The""#, r#""Inn""#, r#""""#, r#""""#],
            [r#""C:\\inn""#, r#""""#, r#""""#, r#""""#],
        ),
    );
}

#[test]
fn a_bedrock_sign_and_bed_keep_their_text_and_colour_in_their_block_entities() {
    let array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    let face = |text: &str| {
        format!(
            "{{Text:{text:?},TextOwner:\"\",SignTextColor:-16777216,IgnoreLighting:1b,\
             HideGlowOutline:0b,PersistFormatting:1b}}"
        )
    };
    let bed = |(x, y, z): (u32, u32, u32)| {
        format!("{{block_entity_data:{{id:\"Bed\",color:11b,x:{x},y:{y},z:{z}}}}}")
    };
    // A voxel's place in `block_indices` is `x * 20 + y * 5 + z` in this 5x4x5 array.
    assert_eq!(
        bedrock_data(&array, "1.21.60"),
        [
            (
                "48".to_owned(),
                format!(
                    "{{block_entity_data:{{id:\"Sign\",FrontText:{},BackText:{},IsWaxed:0b,x:2,\
                     y:1,z:3}}}}",
                    face("The\nInn"),
                    face("Closed")
                ),
            ),
            ("46".to_owned(), bed((2, 1, 1))),
            ("47".to_owned(), bed((2, 1, 2))),
        ],
    );
}

#[test]
fn a_bedrock_bed_of_no_colour_is_saved_white_and_one_of_no_dye_is_refused() {
    let mut array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    for entity in &mut array.block_entities {
        if let BlockEntityData::Bed { color } = &mut entity.data {
            *color = None;
        }
    }
    let colors: Vec<String> = bedrock_data(&array, "1.21.60")
        .into_iter()
        .skip(1)
        .map(|(_, data)| data)
        .collect();
    assert_eq!(
        colors,
        [
            "{block_entity_data:{id:\"Bed\",color:0b,x:2,y:1,z:1}}",
            "{block_entity_data:{id:\"Bed\",color:0b,x:2,y:1,z:2}}",
        ],
    );
    array.block_entities[1].data = BlockEntityData::Bed {
        color: Some("teal".to_owned()),
    };
    let target = resolve_bedrock_target("1.21.60").expect("a shipped target");
    match build_mcstructure_tag(&array, &target, &states_for(&target)) {
        Err(BedrockStructureError::UnknownBedColor { pos, color, valid }) => assert_eq!(
            (pos, color.as_str(), valid.as_str()),
            (
                (2, 1, 1),
                "teal",
                "white, orange, magenta, light_blue, yellow, lime, pink, gray, light_gray, cyan, \
                 purple, blue, brown, green, red, black"
            ),
        ),
        other => panic!("expected UnknownBedColor, got {other:?}"),
    }
}

#[test]
fn a_bed_keeps_nothing_beside_its_block_on_java() {
    let source = ROOM.replace(
        "sign at=inside.front offset=2 y=1 text=[\"The\", \"Inn\"] back=\"Closed\" glowing=true\n  ",
        "",
    );
    let array = lowered(&source, Edition::Java, "1.21.4");
    assert_eq!(array.block_entities.len(), 2);
    assert_eq!(java_nbt(&array, "1.21.4"), []);
}

#[test]
fn sign_text_the_nbt_encoder_cannot_write_is_refused_by_both_writers() {
    let source = ROOM.replace(r#"back="Closed""#, r#"back="Café""#);
    let java = lowered(&source, Edition::Java, "26.3");
    let target = resolve_java_target("26.3").expect("a shipped target");
    match build_structure_tag(&java, &target) {
        Err(JavaStructureError::UnencodableSignText { pos, text, .. }) => {
            assert_eq!((pos, text.as_str()), ((2, 1, 3), "Café"));
        }
        other => panic!("expected UnencodableSignText, got {other:?}"),
    }
    let bedrock = lowered(&source, Edition::Bedrock, "1.21.60");
    let target = resolve_bedrock_target("1.21.60").expect("a shipped target");
    match build_mcstructure_tag(&bedrock, &target, &states_for(&target)) {
        Err(BedrockStructureError::UnencodableSignText { pos, text, .. }) => {
            assert_eq!((pos, text.as_str()), ((2, 1, 3), "Café"));
        }
        other => panic!("expected UnencodableSignText, got {other:?}"),
    }
}
