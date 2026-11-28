//! What a sign and a bed keep beside their blocks, as each writer saves it (`spec/entities` "Block
//! entities").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockEntity, BlockEntityData, DyeColor};
use cairn_lang_formats::bedrock_structure::{
    BedrockStructureError, build_mcstructure_tag, write_mcstructure,
};
use cairn_lang_formats::data_version::{JavaTarget, resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{JavaStructureError, build_structure_tag};
use cairn_lang_formats::litematic::{litematic_files, prepare_litematic};
use cairn_lang_formats::schem::{prepare_schem, schem_files};
use cairn_lang_nbt::read_java_gzip;
use cairn_lang_nbt::tag::{Compound, Tag};

use crate::common::{lower_source, position_data, snbt, states_for, written_palette};

/// A 5x5 room with a sign on the inside of its front wall at (2, 1, 3) and a blue bed against its
/// back wall, its head at (2, 1, 1) and its foot at (2, 1, 2).
const ROOM: &str = "theme t:\n  slot wall -> @cobblestone\n  slot sheet -> @bed.blue\n\n\
                    struct s size=5x5\n  walls mat_slot=wall height=3\n  \
                    sign at=inside.front offset=2 y=1 text=[\"The\", \"Inn\"] back=\"Closed\" \
                    glowing=true\n  bed at=inside.back offset=2 y=1 mat_slot=sheet\n";

/// [`ROOM`]'s sign line, as a test rewrites it.
const ROOM_SIGN: &str =
    "sign at=inside.front offset=2 y=1 text=[\"The\", \"Inn\"] back=\"Closed\" glowing=true";

/// `source`'s one struct, lowered for `edition` at `mc_version` without a finding.
fn lowered(source: &str, edition: Edition, mc_version: &str) -> BlockArray {
    let mut ir = lower_source(source, edition, Some(mc_version));
    assert_eq!(ir.diagnostics, []);
    ir.structures.swap_remove("struct::s").expect("struct s")
}

/// Each `blocks` entry of `array`'s Java structure for `target` that carries `nbt`, as its
/// position and that `nbt` in SNBT.
fn java_nbt_for(array: &BlockArray, target: &JavaTarget) -> Vec<(String, String)> {
    let root = build_structure_tag(array, target).expect("build");
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

/// [`java_nbt_for`] at the shipped Java `mc_version`.
fn java_nbt(array: &BlockArray, mc_version: &str) -> Vec<(String, String)> {
    java_nbt_for(
        array,
        &resolve_java_target(mc_version).expect("a shipped target"),
    )
}

/// `root`'s compound `name`.
fn child<'c>(root: &'c Compound, name: &str) -> &'c Compound {
    match root.entries.get(name) {
        Some(Tag::Compound(child)) => child,
        other => panic!("`{name}` is not a compound: {other:?}"),
    }
}

/// Each `TileEntities` entry of the one region of `source`'s `.litematic` at Java `mc_version`, in
/// SNBT.
fn litematic_tile_entities(source: &str, mc_version: &str) -> Vec<String> {
    let ir = lower_source(source, Edition::Java, Some(mc_version));
    assert_eq!(ir.diagnostics, []);
    let target = resolve_java_target(mc_version).expect("a shipped target");
    let mut files = litematic_files(&ir);
    assert_eq!(files.len(), 1, "one file");
    let mut bytes = Vec::new();
    prepare_litematic(files.remove(0), &target)
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let (_, root) = read_java_gzip(bytes.as_slice()).expect("the bytes read back");
    let [Tag::Compound(region)] = child(&root, "Regions").entries.values().collect::<Vec<_>>()[..]
    else {
        panic!("not one region");
    };
    let Some(Tag::List(tile_entities)) = region.entries.get("TileEntities") else {
        panic!("TileEntities is not a List");
    };
    tile_entities.items.iter().map(snbt).collect()
}

/// Each `BlockEntities` entry of `source`'s one `.schem` at Java `mc_version`, in SNBT.
fn schem_block_entities(source: &str, mc_version: &str) -> Vec<String> {
    let ir = lower_source(source, Edition::Java, Some(mc_version));
    assert_eq!(ir.diagnostics, []);
    let target = resolve_java_target(mc_version).expect("a shipped target");
    let mut files = schem_files(&ir);
    assert_eq!(files.len(), 1, "one file");
    let mut bytes = Vec::new();
    prepare_schem(files.remove(0), &target)
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let (_, root) = read_java_gzip(bytes.as_slice()).expect("the bytes read back");
    let blocks = child(child(&root, "Schematic"), "Blocks");
    let Some(Tag::List(block_entities)) = blocks.entries.get("BlockEntities") else {
        panic!("BlockEntities is not a List");
    };
    block_entities.items.iter().map(snbt).collect()
}

/// `array`'s `.mcstructure` root at Bedrock `mc_version`, which it writes without a note.
fn bedrock_root(array: &BlockArray, mc_version: &str) -> Compound {
    let target = resolve_bedrock_target(mc_version).expect("a shipped target");
    let (root, notes) = build_mcstructure_tag(array, &target, &states_for(&target)).expect("build");
    assert_eq!(notes, []);
    root
}

/// `array`'s `block_position_data` at Bedrock `mc_version`, each entry's key and its SNBT.
fn bedrock_data(array: &BlockArray, mc_version: &str) -> Vec<(String, String)> {
    position_data(&bedrock_root(array, mc_version))
}

/// A Java sign's block entity whose faces hold `front` and `back`, each line as written there.
fn java_sign(front: [&str; 4], back: [&str; 4], glowing: bool) -> String {
    let glowing = i8::from(glowing);
    let face = |lines: [&str; 4]| {
        let messages: Vec<String> = lines.iter().map(|line| format!("{line:?}")).collect();
        format!(
            "{{messages:[{}],color:\"black\",has_glowing_text:{glowing}b}}",
            messages.join(",")
        )
    };
    format!(
        "{{id:\"minecraft:sign\",front_text:{},back_text:{},is_waxed:0b}}",
        face(front),
        face(back)
    )
}

/// A Bedrock sign face holding `text`, glowing or not.
fn bedrock_face(text: &str, glowing: bool) -> String {
    format!(
        "{{Text:{text:?},TextOwner:\"\",SignTextColor:-16777216,IgnoreLighting:{}b,\
         HideGlowOutline:0b,PersistFormatting:1b}}",
        i8::from(glowing)
    )
}

/// [`ROOM`]'s sign as Bedrock keeps it at its key, 48, glowing or not.
fn bedrock_sign(glowing: bool) -> (String, String) {
    (
        "48".to_owned(),
        format!(
            "{{block_entity_data:{{id:\"Sign\",FrontText:{},BackText:{},IsWaxed:0b,x:2,y:1,z:3}}}}",
            bedrock_face("The\nInn", glowing),
            bedrock_face("Closed", glowing)
        ),
    )
}

/// A Bedrock bed half's block entity at `pos`, keeping the dye numbered `color`.
fn bedrock_bed(key: &str, (x, y, z): (u32, u32, u32), color: u8) -> (String, String) {
    (
        key.to_owned(),
        format!("{{block_entity_data:{{id:\"Bed\",color:{color}b,x:{x},y:{y},z:{z}}}}}"),
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
                true,
            ),
        )],
    );
    let array = lowered(ROOM, Edition::Java, "26.3");
    assert_eq!(
        java_nbt(&array, "26.3"),
        [(
            "[2,1,3]".to_owned(),
            java_sign(["The", "Inn", "", ""], ["Closed", "", "", ""], true),
        )],
    );
}

#[test]
fn a_java_sign_saves_its_text_as_text_from_data_version_4325() {
    let array = lowered(ROOM, Edition::Java, "26.3");
    let at = |data_version| {
        let target = JavaTarget {
            mc_version: "26.3".to_owned(),
            data_version,
        };
        java_nbt_for(&array, &target)
    };
    assert_eq!(
        (at(4324), at(4325)),
        (
            vec![(
                "[2,1,3]".to_owned(),
                java_sign(
                    [r#""The""#, r#""Inn""#, r#""""#, r#""""#],
                    [r#""Closed""#, r#""""#, r#""""#, r#""""#],
                    true,
                ),
            )],
            vec![(
                "[2,1,3]".to_owned(),
                java_sign(["The", "Inn", "", ""], ["Closed", "", "", ""], true),
            )],
        ),
    );
}

#[test]
fn a_sign_that_does_not_glow_is_saved_unlit_by_both_writers() {
    let source = ROOM.replace(" glowing=true", "");
    let java = lowered(&source, Edition::Java, "26.3");
    assert_eq!(
        java_nbt(&java, "26.3"),
        [(
            "[2,1,3]".to_owned(),
            java_sign(["The", "Inn", "", ""], ["Closed", "", "", ""], false),
        )],
    );
    let bedrock = lowered(&source, Edition::Bedrock, "1.21.60");
    assert_eq!(
        bedrock_data(&bedrock, "1.21.60"),
        [
            bedrock_bed("46", (2, 1, 1), 11),
            bedrock_bed("47", (2, 1, 2), 11),
            bedrock_sign(false),
        ],
    );
}

#[test]
fn a_litematic_region_keeps_the_sign_a_java_structure_does_and_no_bed() {
    let sign = java_sign(["The", "Inn", "", ""], ["Closed", "", "", ""], true);
    let at_its_voxel = format!("{},x:2,y:1,z:3}}", &sign[..sign.len() - 1]);
    assert_eq!(litematic_tile_entities(ROOM, "26.3"), [at_its_voxel]);
}

#[test]
fn a_schematic_keeps_the_sign_a_java_structure_does_and_no_bed() {
    let face = |lines: [&str; 4]| {
        let messages: Vec<String> = lines.iter().map(|line| format!("{line:?}")).collect();
        format!(
            "{{messages:[{}],color:\"black\",has_glowing_text:1b}}",
            messages.join(",")
        )
    };
    assert_eq!(
        schem_block_entities(ROOM, "26.3"),
        [format!(
            "{{Pos:[I;[2, 1, 3]],Id:\"minecraft:sign\",Data:{{front_text:{},back_text:{},\
             is_waxed:0b}}}}",
            face(["The", "Inn", "", ""]),
            face(["Closed", "", "", ""])
        )],
    );
}

#[test]
fn a_java_sign_line_is_escaped_as_a_json_string() {
    let source = ROOM.replace(r#"back="Closed""#, r#"back="C:\inn""#);
    let array = lowered(&source, Edition::Java, "1.21.4");
    assert_eq!(
        java_nbt(&array, "1.21.4"),
        [(
            "[2,1,3]".to_owned(),
            java_sign(
                [r#""The""#, r#""Inn""#, r#""""#, r#""""#],
                [r#""C:\\inn""#, r#""""#, r#""""#, r#""""#],
                true,
            ),
        )],
    );
}

#[test]
fn a_bedrock_sign_and_bed_keep_their_text_and_colour_in_their_block_entities() {
    let array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    // A voxel's place in `block_indices` is `x * 20 + y * 5 + z` in this 5x4x5 array.
    assert_eq!(
        bedrock_data(&array, "1.21.60"),
        [
            bedrock_bed("46", (2, 1, 1), 11),
            bedrock_bed("47", (2, 1, 2), 11),
            bedrock_sign(true),
        ],
    );
}

#[test]
fn a_bedrock_bed_and_sign_write_their_facing_in_their_palette_entries() {
    let source = ROOM.replace(
        ROOM_SIGN,
        "sign at=inside.front offset=2 y=1\n  sign at=1,1,2",
    );
    let array = lowered(&source, Edition::Bedrock, "1.21.60");
    assert_eq!(
        written_palette(&bedrock_root(&array, "1.21.60")),
        [
            "minecraft:air[]",
            "minecraft:bed[direction=2,head_piece_bit=0,occupied_bit=0]",
            "minecraft:bed[direction=2,head_piece_bit=1,occupied_bit=0]",
            "minecraft:cobblestone[]",
            "minecraft:standing_sign[ground_sign_direction=0]",
            "minecraft:wall_sign[facing_direction=2]",
        ],
    );
}

#[test]
fn a_bedrock_bed_of_no_colour_is_saved_white() {
    let mut array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    for entity in &mut array.block_entities {
        if let BlockEntityData::Bed { color } = &mut entity.data {
            *color = None;
        }
    }
    assert_eq!(
        bedrock_data(&array, "1.21.60"),
        [
            bedrock_bed("46", (2, 1, 1), 0),
            bedrock_bed("47", (2, 1, 2), 0),
            bedrock_sign(true),
        ],
    );
}

#[test]
fn a_bed_keeps_nothing_beside_its_block_on_java() {
    let source = ROOM.replace(&format!("{ROOM_SIGN}\n  "), "");
    let array = lowered(&source, Edition::Java, "1.21.4");
    let half = |pos| BlockEntity {
        pos,
        data: BlockEntityData::Bed {
            color: Some(DyeColor::Blue),
        },
    };
    assert_eq!(array.block_entities, [half((2, 1, 1)), half((2, 1, 2))]);
    assert_eq!(java_nbt(&array, "1.21.4"), []);
}

#[test]
fn sign_text_the_nbt_encoder_cannot_write_is_refused_by_both_writers_with_its_fix() {
    let long = "a".repeat(usize::from(u16::MAX) + 1);
    for (back, fix) in [
        (
            "Café",
            "nbt: string contains byte 0xc3 at index 3, not encodable here. Fix: write the sign's \
             text in printable ASCII"
                .to_owned(),
        ),
        (
            long.as_str(),
            "nbt: string length 65536 exceeds wire limit 65535. Fix: shorten the sign's text, \
             which NBT holds to 65535 bytes as written"
                .to_owned(),
        ),
    ] {
        let source = ROOM.replace(r#"back="Closed""#, &format!("back={back:?}"));
        let java = lowered(&source, Edition::Java, "26.3");
        let target = resolve_java_target("26.3").expect("a shipped target");
        match build_structure_tag(&java, &target) {
            Err(err @ JavaStructureError::UnencodableSignText { .. }) => assert_eq!(
                err.to_string(),
                format!(
                    "the sign at (2, 1, 3) has the line `{back}`, which cannot be written as NBT: \
                     {fix}"
                ),
            ),
            other => panic!("expected UnencodableSignText, got {other:?}"),
        }
        let bedrock = lowered(&source, Edition::Bedrock, "1.21.60");
        let target = resolve_bedrock_target("1.21.60").expect("a shipped target");
        match build_mcstructure_tag(&bedrock, &target, &states_for(&target)) {
            Err(err @ BedrockStructureError::UnencodableSignText { .. }) => assert_eq!(
                err.to_string(),
                format!(
                    "the sign at (2, 1, 3) has the text `{back}`, which cannot be written as NBT: \
                     {fix}"
                ),
            ),
            other => panic!("expected UnencodableSignText, got {other:?}"),
        }
    }
}

/// `spec/compilation` "Within-phase conflicts and the palette": two signs that share no voxel
/// write the same `.mcstructure` bytes whichever line comes first.
#[test]
fn swapping_two_signs_leaves_the_bedrock_bytes_equal() {
    let walled = |signs: &str| {
        format!(
            "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x5\n  \
             walls mat_slot=wall height=3\n{signs}"
        )
    };
    let a = "  sign at=inside.back offset=1 y=1 text=\"A\"\n";
    let b = "  sign at=inside.back offset=3 y=1 text=\"B\"\n";
    let root =
        |source: &str| bedrock_root(&lowered(source, Edition::Bedrock, "1.21.60"), "1.21.60");
    let bytes = |source: &str| {
        let mut out = Vec::new();
        write_mcstructure(&mut out, &root(source)).expect("writes");
        out
    };
    assert_eq!(
        bytes(&walled(&format!("{a}{b}"))),
        bytes(&walled(&format!("{b}{a}")))
    );
    let keys: Vec<String> = position_data(&root(&walled(&format!("{a}{b}"))))
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(keys, ["26", "66"]);
}
