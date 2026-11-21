//! What the entities a build places look like in each writer's file: Java's `entities` list, in
//! the shape each `DataVersion` reads, and Bedrock's, with an item frame built as a frame block
//! (`spec/entities` "Two tiers").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, EntityData};
use cairn_lang_formats::bedrock_structure::{BedrockStructureError, build_mcstructure_tag};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{JavaStructureError, build_structure_tag};
use cairn_lang_nbt::tag::{Compound, Tag};

use crate::common::{
    lower_source, position_data, snbt, states_for, structure_compound, written_palette,
};

/// A 7x5 room, its front wall `z=4`: a librarian who sells a book of mending, an armor stand, a
/// painting inside the front wall and a frame holding a diamond inside the back one.
const ROOM: &str = "theme t:\n  slot wall -> @cobblestone\n\n\
                    struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                    villager id=lib at=2,1,2 profession=librarian level=apprentice\n  \
                    trade villager=lib buy=emerald count=24 sell=enchanted_book enchant=mending\n  \
                    armor_stand at=4,1,2 facing=east\n  \
                    painting at=inside.front offset=2 y=1 variant=kebab\n  \
                    item_frame at=inside.back offset=3 y=2 item=diamond\n";

/// [`ROOM`] with a text display and a block display, which only Java has.
fn java_room() -> String {
    format!(
        "{ROOM}  text_display at=3,2,2 text=[\"Inn\", \"rooms\"]\n  \
         block_display at=5,1,3 mat_slot=wall scale=0.5\n"
    )
}

/// `source`'s one struct, lowered for `edition` at `mc_version` without a finding.
fn lowered(source: &str, edition: Edition, mc_version: &str) -> BlockArray {
    let mut ir = lower_source(source, edition, Some(mc_version));
    assert_eq!(ir.diagnostics, []);
    ir.structures.swap_remove("struct::s").expect("struct s")
}

/// The `entities` of `array`'s Java structure at `mc_version`, each in SNBT.
fn java_entities(array: &BlockArray, mc_version: &str) -> Vec<String> {
    let target = resolve_java_target(mc_version).expect("a shipped target");
    let root = build_structure_tag(array, &target).expect("build");
    listed(&root)
}

/// Each entry of `compound`'s `entities` list, in SNBT.
fn listed(compound: &Compound) -> Vec<String> {
    match compound.entries.get("entities") {
        Some(Tag::List(list)) => list.items.iter().map(snbt).collect(),
        other => panic!("entities is not a List: {other:?}"),
    }
}

/// The Java `nbt` of one entity of `array` at `mc_version`, the `n`th the list holds.
fn java_nbt(array: &BlockArray, mc_version: &str, n: usize) -> String {
    let target = resolve_java_target(mc_version).expect("a shipped target");
    let root = build_structure_tag(array, &target).expect("build");
    let Some(Tag::List(list)) = root.entries.get("entities") else {
        panic!("no entities list");
    };
    let Tag::Compound(entry) = &list.items[n] else {
        panic!("entity entry: {:?}", list.items[n]);
    };
    snbt(entry.entries.get("nbt").expect("an nbt"))
}

const TRANSFORMATION: &str = "{left_rotation:[0f,0f,0f,1f],right_rotation:[0f,0f,0f,1f],\
                              translation:[0f,0f,0f],scale:[1f,1f,1f]}";

#[test]
fn a_java_structure_lists_each_entity_with_its_point_its_cell_and_its_nbt() {
    let array = lowered(&java_room(), Edition::Java, "26.3");
    assert_eq!(
        java_entities(&array, "26.3"),
        [
            "{pos:[2.5d,1d,2.5d],blockPos:[2,1,2],nbt:{id:\"minecraft:villager\",\
             Rotation:[0f,0f],VillagerData:{type:\"minecraft:plains\",\
             profession:\"minecraft:librarian\",level:2},Xp:10,Offers:{Recipes:[{buy:{id:\
             \"minecraft:emerald\",count:24},sell:{id:\"minecraft:enchanted_book\",count:1,\
             components:{minecraft:stored_enchantments:{minecraft:mending:1}}},uses:0,maxUses:12,\
             rewardExp:1b,xp:1,priceMultiplier:0.05f,specialPrice:0,demand:0}]}}}"
                .to_owned(),
            "{pos:[4.5d,1d,2.5d],blockPos:[4,1,2],nbt:{id:\"minecraft:armor_stand\",\
             Rotation:[270f,0f]}}"
                .to_owned(),
            "{pos:[2.5d,1.5d,3.96875d],blockPos:[2,1,3],nbt:{id:\"minecraft:painting\",\
             variant:\"minecraft:kebab\",facing:2b}}"
                .to_owned(),
            "{pos:[5.5d,1d,3.5d],blockPos:[5,1,3],nbt:{id:\"minecraft:block_display\",\
             Rotation:[0f,0f],billboard:\"fixed\",transformation:{left_rotation:[0f,0f,0f,1f],\
             right_rotation:[0f,0f,0f,1f],translation:[0f,0f,0f],scale:[0.5f,0.5f,0.5f]},\
             block_state:{id:\"minecraft:cobblestone\"}}}"
                .to_owned(),
            "{pos:[3.5d,2.5d,1.03125d],blockPos:[3,2,1],nbt:{id:\"minecraft:item_frame\",\
             Facing:3b,Item:{id:\"minecraft:diamond\",count:1}}}"
                .to_owned(),
            format!(
                "{{pos:[3.5d,2d,2.5d],blockPos:[3,2,2],nbt:{{id:\"minecraft:text_display\",\
                 Rotation:[0f,0f],billboard:\"fixed\",transformation:{TRANSFORMATION},\
                 text:\"Inn\\nrooms\"}}}}"
            ),
        ],
    );
}

#[test]
fn a_java_item_and_text_take_the_shape_each_data_version_reads() {
    let stacks = |mc_version: &str| {
        let array = lowered(&java_room(), Edition::Java, mc_version);
        [0, 3, 4, 5].map(|n| java_nbt(&array, mc_version, n))
    };
    let villager = |stack: &str, book: &str| {
        format!(
            "{{id:\"minecraft:villager\",Rotation:[0f,0f],VillagerData:{{type:\"minecraft:plains\",\
             profession:\"minecraft:librarian\",level:2}},Xp:10,Offers:{{Recipes:[{{buy:{stack},\
             sell:{book},uses:0,maxUses:12,rewardExp:1b,xp:1,priceMultiplier:0.05f,\
             specialPrice:0,demand:0}}]}}}}"
        )
    };
    let block_display = |state: &str| {
        format!(
            "{{id:\"minecraft:block_display\",Rotation:[0f,0f],billboard:\"fixed\",\
             transformation:{{left_rotation:[0f,0f,0f,1f],right_rotation:[0f,0f,0f,1f],\
             translation:[0f,0f,0f],scale:[0.5f,0.5f,0.5f]}},block_state:{state}}}"
        )
    };
    let frame = |stack: &str| format!("{{id:\"minecraft:item_frame\",Facing:3b,Item:{stack}}}");
    let text = |text: &str| {
        format!(
            "{{id:\"minecraft:text_display\",Rotation:[0f,0f],billboard:\"fixed\",\
             transformation:{TRANSFORMATION},text:{text}}}"
        )
    };
    assert_eq!(
        stacks("1.20.4"),
        [
            villager(
                "{id:\"minecraft:emerald\",Count:24b}",
                "{id:\"minecraft:enchanted_book\",Count:1b,tag:{StoredEnchantments:[{id:\
                 \"minecraft:mending\",lvl:1s}]}}",
            ),
            block_display("{Name:\"minecraft:cobblestone\"}"),
            frame("{id:\"minecraft:diamond\",Count:1b}"),
            text("\"\\\"Inn\\\\u000arooms\\\"\""),
        ],
    );
    assert_eq!(
        stacks("1.21.4"),
        [
            villager(
                "{id:\"minecraft:emerald\",count:24}",
                "{id:\"minecraft:enchanted_book\",count:1,components:{minecraft:\
                 stored_enchantments:{levels:{minecraft:mending:1}}}}",
            ),
            block_display("{Name:\"minecraft:cobblestone\"}"),
            frame("{id:\"minecraft:diamond\",count:1}"),
            text("\"\\\"Inn\\\\u000arooms\\\"\""),
        ],
    );
}

#[test]
fn a_novice_is_saved_with_one_experience_to_keep_its_profession() {
    let source = java_room().replace("level=apprentice", "level=novice");
    let array = lowered(&source, Edition::Java, "26.3");
    let nbt = java_nbt(&array, "26.3", 0);
    assert!(
        nbt.starts_with(
            "{id:\"minecraft:villager\",Rotation:[0f,0f],VillagerData:{type:\"minecraft:plains\",\
             profession:\"minecraft:librarian\",level:1},Xp:1,"
        ),
        "{nbt}"
    );
}

#[test]
fn an_nbt_compound_is_put_over_what_the_writer_builds_key_by_key() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  @edition java\n    \
                  armor_stand at=2,1,2 nbt={Rotation:[90f,0f],ShowArms:1b}\n    \
                  spawn type=cat at=4,1,2 nbt={variant:\"minecraft:black\",Tags:[\"a\",\"b\"]}\n    \
                  raw_block mat=minecraft:chest at=3,1,3 nbt={Lock:\"key\"}\n";
    let array = lowered(source, Edition::Java, "26.3");
    assert_eq!(
        [0, 1].map(|n| java_nbt(&array, "26.3", n)),
        [
            "{id:\"minecraft:armor_stand\",Rotation:[90f,0f],ShowArms:1b}",
            "{id:\"minecraft:cat\",Rotation:[0f,0f],variant:\"minecraft:black\",\
             Tags:[\"a\",\"b\"]}",
        ],
    );
    let target = resolve_java_target("26.3").expect("a shipped target");
    let root = build_structure_tag(&array, &target).expect("build");
    let Some(Tag::List(blocks)) = root.entries.get("blocks") else {
        panic!("no blocks");
    };
    let kept: Vec<String> = blocks
        .items
        .iter()
        .filter_map(|block| match block {
            Tag::Compound(block) => block.entries.get("nbt").map(snbt),
            _ => None,
        })
        .collect();
    assert_eq!(kept, ["{Lock:\"key\"}"]);
}

#[test]
fn an_entity_string_the_nbt_encoder_cannot_write_is_refused() {
    let source = java_room().replace("text=[\"Inn\", \"rooms\"]", "text=\"Café\"");
    let array = lowered(&source, Edition::Java, "26.3");
    let target = resolve_java_target("26.3").expect("a shipped target");
    match build_structure_tag(&array, &target) {
        Err(JavaStructureError::UnencodableEntity { id, pos, text, .. }) => assert_eq!(
            (id.as_str(), pos, text.as_str()),
            ("minecraft:text_display", (3, 2, 2), "Café"),
        ),
        other => panic!("expected UnencodableEntity, got {other:?}"),
    }
}

/// `array`'s Bedrock structure at `mc_version`.
fn bedrock_root(array: &BlockArray, mc_version: &str) -> Compound {
    let target = resolve_bedrock_target(mc_version).expect("a shipped target");
    let (root, notes) = build_mcstructure_tag(array, &target, &states_for(&target)).expect("build");
    assert_eq!(notes, []);
    root
}

#[test]
fn a_bedrock_structure_lists_each_entity_and_builds_an_item_frame_as_a_frame_block() {
    let array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    let root = bedrock_root(&array, "1.21.60");
    assert_eq!(
        listed(structure_compound(&root)),
        [
            "{identifier:\"minecraft:villager_v2\",Pos:[2.5f,1f,2.5f],Rotation:[0f,0f],\
             Motion:[0f,0f,0f],UniqueID:1L,Variant:5,PreferredProfession:\"librarian\",\
             TradeTier:1,TradeExperience:10,Offers:{Recipes:[{buyA:{Name:\"minecraft:emerald\",\
             Count:24b,Damage:0s,WasPickedUp:0b},buyCountA:24,buyCountB:0,sell:{Name:\
             \"minecraft:enchanted_book\",Count:1b,Damage:0s,WasPickedUp:0b,tag:{ench:[{id:26s,\
             lvl:1s}]}},uses:0,maxUses:12,rewardExp:1b,tier:1,traderExp:1,\
             priceMultiplierA:0.05f,priceMultiplierB:0f,demand:0}]},\
             definitions:[\"+minecraft:villager_v2\",\"+librarian\"]}",
            "{identifier:\"minecraft:armor_stand\",Pos:[4.5f,1f,2.5f],Rotation:[270f,0f],\
             Motion:[0f,0f,0f],UniqueID:2L,definitions:[\"+minecraft:armor_stand\"]}",
            "{identifier:\"minecraft:painting\",Pos:[2.5f,1.5f,3.96875f],Rotation:[180f,0f],\
             Motion:[0f,0f,0f],UniqueID:3L,Motive:\"Kebab\",Direction:2b,\
             definitions:[\"+minecraft:painting\"]}",
        ],
    );
    assert!(
        written_palette(&root).contains(
            &"minecraft:frame[facing_direction=3,item_frame_map_bit=0,item_frame_photo_bit=0]"
                .to_owned()
        ),
        "{:?}",
        written_palette(&root)
    );
    // A voxel's place in `block_indices` is `(x * 4 + y) * 5 + z` in this 7x4x5 array.
    assert_eq!(
        position_data(&root),
        [(
            "71".to_owned(),
            "{block_entity_data:{id:\"ItemFrame\",Item:{Name:\"minecraft:diamond\",Count:1b,\
             Damage:0s,WasPickedUp:0b},ItemRotation:0f,ItemDropChance:1f,x:3,y:2,z:1}}"
                .to_owned()
        )],
    );
}

/// A painting inside each wall of a 7x5 room, so one faces each compass direction.
const GALLERY: &str = "theme t:\n  slot wall -> @cobblestone\n\n\
                       struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                       painting at=inside.front offset=2 y=1 variant=kebab\n  \
                       painting at=inside.back offset=2 y=1 variant=kebab\n  \
                       painting at=inside.left offset=2 y=1 variant=kebab\n  \
                       painting at=inside.right offset=2 y=1 variant=kebab\n";

/// Each entity of `array` by the way it faces, beside the byte `key` holds in `entries`, the
/// written list of its entities, inside each entry's `nbt` when `in_nbt`.
fn saved_facings(
    array: &BlockArray,
    entries: &Compound,
    key: &str,
    in_nbt: bool,
) -> Vec<(&'static str, i8)> {
    let Some(Tag::List(list)) = entries.entries.get("entities") else {
        panic!("no entities list");
    };
    let mut saved: Vec<(&'static str, i8)> = array
        .entities
        .iter()
        .zip(&list.items)
        .map(|(entity, entry)| {
            let Tag::Compound(written) = entry else {
                panic!("entity entry: {entry:?}");
            };
            let fields = match written.entries.get("nbt") {
                Some(Tag::Compound(nbt)) if in_nbt => nbt,
                _ if !in_nbt => written,
                _ => panic!("no nbt: {entry:?}"),
            };
            let Some(Tag::Byte(number)) = fields.entries.get(key) else {
                panic!("no byte `{key}`: {entry:?}");
            };
            (entity.facing.as_str(), *number)
        })
        .collect();
    saved.sort_unstable_by_key(|(_, number)| *number);
    saved
}

#[test]
fn each_writer_numbers_a_painting_s_facing_south_west_north_east() {
    let expected = [("south", 0), ("west", 1), ("north", 2), ("east", 3)];
    let java = lowered(GALLERY, Edition::Java, "26.3");
    let target = resolve_java_target("26.3").expect("a shipped target");
    let root = build_structure_tag(&java, &target).expect("build");
    assert_eq!(saved_facings(&java, &root, "facing", true), expected);
    let bedrock = lowered(GALLERY, Edition::Bedrock, "1.21.60");
    let root = bedrock_root(&bedrock, "1.21.60");
    assert_eq!(
        saved_facings(&bedrock, structure_compound(&root), "Direction", false),
        expected
    );
}

#[test]
fn a_bedrock_frame_in_a_cell_a_block_holds_is_refused() {
    let mut array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    for entity in &mut array.entities {
        if matches!(entity.data, EntityData::ItemFrame { .. }) {
            entity.block_pos = (3, 2, 0);
        }
    }
    let target = resolve_bedrock_target("1.21.60").expect("a shipped target");
    match build_mcstructure_tag(&array, &target, &states_for(&target)) {
        Err(BedrockStructureError::FrameCellTaken { id, pos, block }) => assert_eq!(
            (id.as_str(), pos, block.as_str()),
            ("minecraft:frame", (3, 2, 0), "minecraft:cobblestone"),
        ),
        other => panic!("expected FrameCellTaken, got {other:?}"),
    }
}

#[test]
fn a_bedrock_writer_refuses_a_display_it_has_no_entity_for() {
    let java = lowered(&java_room(), Edition::Java, "26.3");
    let mut array = lowered(ROOM, Edition::Bedrock, "1.21.60");
    array.entities.extend(
        java.entities
            .into_iter()
            .filter(|entity| entity.id == "minecraft:text_display"),
    );
    let target = resolve_bedrock_target("1.21.60").expect("a shipped target");
    match build_mcstructure_tag(&array, &target, &states_for(&target)) {
        Err(BedrockStructureError::UnmappedEntity { id, pos }) => {
            assert_eq!((id.as_str(), pos), ("minecraft:text_display", (3, 2, 2)));
        }
        other => panic!("expected UnmappedEntity, got {other:?}"),
    }
}
