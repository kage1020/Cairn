//! Panes, fences, walls and fence gates take their connection states from the blocks beside them
//! once every member is painted (`spec/blockstate` "Connections").

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockIdSet, BlockState, Covers, Face, Faces, JoinKind, Joins,
    StateDomain, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::{lower, parse, resolve};
use serde_json::json;

use crate::common::{only_structure, state_at};

/// A target whose connection table holds the blocks these tests build with, read as the game's
/// rules read them.
struct Rules;

impl TargetRegistry for Rules {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Checked(Vec::new())
    }

    fn joins(&self, state: &BlockState) -> Option<Joins> {
        let value = |key: &str, default: &'static str| {
            state.properties.get(key).map_or(default, String::as_str)
        };
        let kind = |kind| Joins {
            kind: Some(kind),
            ..Joins::default()
        };
        let covers = |post, sides: &[Face]| Covers {
            post,
            sides: sides.iter().copied().collect(),
        };
        let whole = covers(true, &Face::SIDES);
        Some(match state.id.strip_prefix("minecraft:")? {
            "glass_pane" => kind(JoinKind::Pane),
            "cobblestone_wall" => kind(JoinKind::Wall),
            "oak_fence" => kind(JoinKind::Fence { wooden: true }),
            "nether_brick_fence" => kind(JoinKind::Fence { wooden: false }),
            "oak_fence_gate" => kind(JoinKind::FenceGate {
                facing: Face::from_name(value("facing", "north"))?,
                open: value("open", "false") == "true",
            }),
            "cobblestone" => Joins {
                sturdy: Faces::ALL,
                covers: whole,
                ..Joins::default()
            },
            "oak_leaves" => Joins {
                sturdy: Faces::ALL,
                covers: whole,
                never_joined_by_face: true,
                ..Joins::default()
            },
            "oak_stairs" if value("half", "bottom") == "bottom" => Joins {
                sturdy: Faces::NONE.with(Face::Down),
                covers: whole,
                ..Joins::default()
            },
            "lantern" => Joins {
                covers: covers(true, &[]),
                ..Joins::default()
            },
            // Its logs lie across the way it faces, so it covers the post and the sides they cross.
            "campfire" => Joins {
                covers: match value("facing", "north") {
                    "east" | "west" => covers(true, &[Face::East, Face::West]),
                    _ => covers(true, &[Face::North, Face::South]),
                },
                ..Joins::default()
            },
            "oak_sign" => Joins {
                raises_post: true,
                ..Joins::default()
            },
            // Bedrock's, which no Java block becomes, so the table cannot say what it is.
            "border_block" => return None,
            _ => Joins::default(),
        })
    }
}

/// A target that cannot say what any block is to the rules.
struct Silent;

impl TargetRegistry for Silent {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

fn lowered_with(source: &str, registry: &dyn TargetRegistry) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, None), Some(registry))
}

/// A 5x3 struct of `walls` built from `wall`, `height` rows tall, with `members` after them; its
/// front wall runs along x at `z=2`.
fn yard(wall: &str, height: u32, members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> {wall}\n  slot glass -> @glass_pane\n  \
         slot black -> @nether_brick_fence\n  slot gate -> @oak_fence_gate\n  \
         slot open_gate -> @oak_fence_gate[facing=east]\n  slot north -> @glass_pane[north=true]\n  \
         slot roof -> @oak_stairs\n\n\
         struct s size=5x3\n  walls mat_slot=wall height={height}\n  {members}\n"
    )
}

/// The block at `(x, y, z)` as `id[properties]`.
fn at(array: &BlockArray, x: u32, y: u32, z: u32) -> String {
    let state = state_at(array, x, y, z);
    format!("{}[{}]", state.id, state.properties_text())
}

/// A 5x3 struct of `walls` built from `wall`, `height` rows tall, with `material` set into its
/// front wall at `(2, y, 2)`; the block that ends up there and the one under it, top first.
fn set_into(wall: &str, height: u32, material: &str, y: u32) -> [String; 2] {
    let src = format!(
        "theme t:\n  slot wall -> {wall}\n  slot set -> {material}\n\n\
         struct s size=5x3\n  walls mat_slot=wall height={height}\n  \
         window side=front offset=2 y={y} size=1x1 mat_slot=set\n"
    );
    let out = lowered_with(&src, &Rules);
    assert_eq!(out.diagnostics, [], "{material}");
    let ba = only_structure(&out);
    [at(ba, 2, y, 2), at(ba, 2, y - 1, 2)]
}

/// The wall in the middle of a straight run, under `above`.
fn wall_under(above: &str) -> String {
    let [_, under] = set_into("@cobblestone_wall", 2, above, 2);
    under
}

/// A `cobblestone_wall` with these side heights and post.
fn wall(east: &str, north: &str, south: &str, up: bool, west: &str) -> String {
    format!(
        "minecraft:cobblestone_wall[east={east},north={north},south={south},up={up},west={west}]"
    )
}

#[test]
fn a_pane_joins_the_whole_faces_and_the_panes_beside_it() {
    let out = lowered_with(
        &yard(
            "@cobblestone",
            1,
            "window side=front offset=1 y=1 size=2x1 mat_slot=glass",
        ),
        &Rules,
    );
    assert_eq!(out.diagnostics, []);
    let ba = only_structure(&out);
    for x in 1..=2 {
        assert_eq!(
            at(ba, x, 1, 2),
            "minecraft:glass_pane[east=true,north=false,south=false,west=true]",
            "x={x}",
        );
    }
    let window = out.members["struct::s"]
        .iter()
        .find(|member| member.role == "window")
        .expect("the window is recorded");
    assert_eq!(
        serde_json::to_value(&window.resolved_state).expect("serialises"),
        json!([{
            "state": {
                "id": "minecraft:glass_pane",
                "properties": {"east": "true", "north": "false", "south": "false", "west": "true"},
            },
            "runs": [{ "from": [1, 1, 2], "len": 2 }],
        }]),
    );
}

#[test]
fn a_pane_never_joins_leaves_by_their_faces() {
    let out = lowered_with(
        &yard(
            "@oak_leaves",
            1,
            "window side=front offset=1 y=1 size=1x1 mat_slot=glass",
        ),
        &Rules,
    );
    assert_eq!(
        at(only_structure(&out), 1, 1, 2),
        "minecraft:glass_pane[east=false,north=false,south=false,west=false]",
    );
}

#[test]
fn a_fence_joins_the_fences_of_its_own_group() {
    let out = lowered_with(
        &yard(
            "@oak_fence",
            1,
            "window side=front offset=2 y=1 size=1x1 mat_slot=black",
        ),
        &Rules,
    );
    let ba = only_structure(&out);
    assert_eq!(
        at(ba, 0, 1, 2),
        "minecraft:oak_fence[east=true,north=true,south=false,west=false]",
    );
    assert_eq!(
        at(ba, 1, 1, 2),
        "minecraft:oak_fence[east=false,north=false,south=false,west=true]",
    );
    assert_eq!(
        at(ba, 2, 1, 2),
        "minecraft:nether_brick_fence[east=false,north=false,south=false,west=false]",
    );
}

#[test]
fn a_wall_stands_tall_under_a_wall_and_raises_its_post_where_it_turns() {
    let out = lowered_with(&yard("@cobblestone_wall", 2, ""), &Rules);
    assert_eq!(out.diagnostics, []);
    let ba = only_structure(&out);
    // The front-left corner turns north and east; the cells between run straight east-west.
    assert_eq!(at(ba, 0, 1, 2), wall("tall", "tall", "none", true, "none"));
    assert_eq!(at(ba, 2, 1, 2), wall("tall", "none", "none", false, "tall"));
    assert_eq!(at(ba, 0, 2, 2), wall("low", "low", "none", true, "none"));
    assert_eq!(at(ba, 2, 2, 2), wall("low", "none", "none", false, "low"));
}

#[test]
fn a_wall_stands_tall_under_a_block_whose_bottom_is_whole() {
    let out = lowered_with(
        &yard("@cobblestone_wall", 1, "roof kind=flat mat_slot=roof"),
        &Rules,
    );
    let ba = only_structure(&out);
    assert_eq!(at(ba, 2, 1, 2), wall("tall", "none", "none", false, "tall"));
}

#[test]
fn a_wall_raises_its_post_under_a_block_whose_bottom_covers_only_the_post() {
    assert_eq!(
        wall_under("@lantern"),
        wall("low", "none", "none", true, "low")
    );
}

#[test]
fn a_wall_stands_tall_on_the_sides_the_block_above_covers() {
    // Covered on both sides, the post stays down; covered across them, only the post rises.
    assert_eq!(
        wall_under("@campfire[facing=east]"),
        wall("tall", "none", "none", false, "tall"),
    );
    assert_eq!(
        wall_under("@campfire[facing=north]"),
        wall("low", "none", "none", true, "low"),
    );
}

#[test]
fn a_wall_raises_its_post_under_a_block_the_post_override_tag_lists() {
    assert_eq!(
        wall_under("@oak_sign"),
        wall("low", "none", "none", true, "low")
    );
}

#[test]
fn a_wall_reads_a_connecting_block_above_by_the_sides_it_joins() {
    // A gate covers the sides it spans and the post, and an open one covers nothing.
    assert_eq!(
        wall_under("@oak_fence_gate[facing=north]"),
        wall("tall", "none", "none", false, "tall"),
    );
    assert_eq!(
        wall_under("@oak_fence_gate[facing=east]"),
        wall("low", "none", "none", true, "low"),
    );
    assert_eq!(
        wall_under("@oak_fence_gate[facing=east,open=true]"),
        wall("low", "none", "none", false, "low"),
    );
    // A pane, fence or wall above covers its post, and each side it joins.
    for above in [
        "@glass_pane[east=false,west=false]",
        "@oak_fence[east=false,west=false]",
    ] {
        assert_eq!(
            wall_under(above),
            wall("low", "none", "none", true, "low"),
            "{above}"
        );
    }
    assert_eq!(
        wall_under("@glass_pane"),
        wall("tall", "none", "none", false, "tall")
    );
    assert_eq!(
        wall_under("@cobblestone_wall[east=none,west=none,up=false]"),
        wall("low", "none", "none", false, "low"),
    );
}

#[test]
fn a_wall_under_a_wall_whose_post_is_up_raises_its_own() {
    // The sign raises the post of the wall it stands on, and that post the one under it.
    let src = "theme t:\n  slot wall -> @cobblestone_wall\n  slot sign -> @oak_sign\n\n\
               struct s size=5x3\n  walls mat_slot=wall height=3\n  \
               window side=front offset=2 y=3 size=1x1 mat_slot=sign\n";
    let out = lowered_with(src, &Rules);
    let ba = only_structure(&out);
    assert_eq!(at(ba, 2, 2, 2), wall("low", "none", "none", true, "low"));
    assert_eq!(at(ba, 2, 1, 2), wall("tall", "none", "none", true, "tall"));
}

#[test]
fn a_wall_and_a_fence_join_a_sturdy_face_and_a_pane_joins_a_wall() {
    assert_eq!(
        set_into("@cobblestone", 1, "@cobblestone_wall", 1)[0],
        wall("low", "none", "none", false, "low"),
    );
    assert_eq!(
        set_into("@cobblestone", 1, "@oak_fence", 1)[0],
        "minecraft:oak_fence[east=true,north=false,south=false,west=true]",
    );
    let src = yard(
        "@cobblestone_wall",
        1,
        "window side=front offset=2 y=1 size=1x1 mat_slot=glass",
    );
    let ba = lowered_with(&src, &Rules);
    let ba = only_structure(&ba);
    assert_eq!(
        at(ba, 2, 1, 2),
        "minecraft:glass_pane[east=true,north=false,south=false,west=true]",
    );
    assert_eq!(at(ba, 1, 1, 2), wall("low", "none", "none", false, "low"));
}

#[test]
fn a_fence_joins_a_gate_across_it_and_a_gate_between_fences_is_not_in_a_wall() {
    let src = yard(
        "@oak_fence",
        1,
        "window side=front offset=2 y=1 size=1x1 mat_slot=gate",
    );
    let out = lowered_with(&src, &Rules);
    let ba = only_structure(&out);
    assert_eq!(at(ba, 2, 1, 2), "minecraft:oak_fence_gate[in_wall=false]");
    assert_eq!(
        at(ba, 1, 1, 2),
        "minecraft:oak_fence[east=true,north=false,south=false,west=true]",
    );
}

#[test]
fn a_gate_joins_across_the_way_it_faces_and_sits_lower_between_walls() {
    let src = yard(
        "@cobblestone_wall",
        1,
        "window side=front offset=1 y=1 size=1x1 mat_slot=gate\n  \
         window side=front offset=3 y=1 size=1x1 mat_slot=open_gate",
    );
    let out = lowered_with(&src, &Rules);
    let ba = only_structure(&out);
    assert_eq!(at(ba, 1, 1, 2), "minecraft:oak_fence_gate[in_wall=true]",);
    assert_eq!(
        at(ba, 3, 1, 2),
        "minecraft:oak_fence_gate[facing=east,in_wall=false]",
    );
    // The wall between the two joins the gate facing north and not the one facing east.
    assert_eq!(
        at(ba, 2, 1, 2),
        "minecraft:cobblestone_wall[east=none,north=none,south=none,up=true,west=low]",
    );
}

#[test]
fn a_side_or_a_gate_state_the_material_writes_is_kept_however_the_rules_would_set_it() {
    // Under a whole bottom both sides would stand tall, and the post rises over two low sides.
    assert_eq!(
        set_into("@cobblestone", 2, "@cobblestone_wall[east=low,west=low]", 1)[0],
        wall("low", "none", "none", true, "low"),
    );
    assert_eq!(
        set_into("@cobblestone_wall", 1, "@oak_fence_gate[in_wall=false]", 1)[0],
        "minecraft:oak_fence_gate[in_wall=false]",
    );
}

#[test]
fn a_connection_state_the_material_writes_is_kept() {
    let out = lowered_with(
        &yard(
            "@cobblestone",
            1,
            "window side=front offset=1 y=1 size=1x1 mat_slot=north",
        ),
        &Rules,
    );
    assert_eq!(
        at(only_structure(&out), 1, 1, 2),
        "minecraft:glass_pane[east=true,north=true,south=false,west=true]",
    );
}

#[test]
fn a_block_beside_that_the_target_cannot_say_anything_of_is_joined_as_nothing() {
    let out = lowered_with(
        &yard(
            "@border_block",
            1,
            "window side=front offset=1 y=1 size=1x1 mat_slot=glass",
        ),
        &Rules,
    );
    assert_eq!(
        at(only_structure(&out), 1, 1, 2),
        "minecraft:glass_pane[east=false,north=false,south=false,west=false]",
    );
}

#[test]
fn a_target_that_cannot_say_leaves_each_block_as_painted() {
    let out = lowered_with(
        &yard(
            "@cobblestone",
            1,
            "window side=front offset=1 y=1 size=1x1 mat_slot=glass",
        ),
        &Silent,
    );
    assert_eq!(at(only_structure(&out), 1, 1, 2), "minecraft:glass_pane[]");
}
