//! Panes, fences, walls, fence gates and chests take their connection states from the blocks
//! beside them once every member is painted (`spec/blockstate` "Connections").

use cairn_lang_core::block_array::{
    BlockArray, BlockIdSet, BlockState, Face, Faces, JoinKind, Joins, TargetRegistry,
};
use serde_json::json;

use crate::common::{chests, lowered_with, only_structure, state_at};

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

    fn joins(&self, state: &BlockState) -> Option<Joins> {
        let value = |key: &str, default: &'static str| {
            state.properties.get(key).map_or(default, String::as_str)
        };
        let kind = |kind| Joins {
            kind: Some(kind),
            ..Joins::default()
        };
        Some(match state.id.strip_prefix("minecraft:")? {
            "glass_pane" => kind(JoinKind::Pane),
            "cobblestone_wall" => kind(JoinKind::Wall),
            "oak_fence" => kind(JoinKind::Fence { wooden: true }),
            "nether_brick_fence" => kind(JoinKind::Fence { wooden: false }),
            "oak_fence_gate" => kind(JoinKind::FenceGate {
                facing: Face::from_name(value("facing", "north"))?,
                open: value("open", "false") == "true",
            }),
            "chest" | "trapped_chest" => kind(JoinKind::Chest {
                facing: Face::from_name(value("facing", "north"))?,
            }),
            "cobblestone" => Joins {
                sturdy: Faces::ALL,
                ..Joins::default()
            },
            "oak_leaves" => Joins {
                sturdy: Faces::ALL,
                exception: true,
                ..Joins::default()
            },
            "oak_stairs" if value("half", "bottom") == "bottom" => Joins {
                sturdy: Faces::NONE.with(Face::Down),
                ..Joins::default()
            },
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
            "cells": [[1, 1, 2], [2, 1, 2]],
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
    let wall = |east: &str, north: &str, south: &str, up: bool, west: &str| {
        format!(
            "minecraft:cobblestone_wall[east={east},north={north},south={south},up={up},west={west}]"
        )
    };
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
    assert_eq!(
        at(ba, 2, 1, 2),
        "minecraft:cobblestone_wall[east=tall,north=none,south=none,up=false,west=tall]",
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

/// A 7x5 struct with 3-row walls under a flat roof one voxel past them, `chests` after the roof;
/// the overhang puts its back wall at `z=1`, where `offset=` grows toward the west.
fn storeroom(chests: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot trap -> @trapped_chest\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  {chests}\n"
    )
}

/// The chests `lines` sets in [`storeroom`], each with its cell, paired as [`Rules`] pairs them.
fn paired(lines: &[&str]) -> Vec<((u32, u32, u32), String)> {
    let out = lowered_with(&storeroom(&lines.join("\n  ")), &Rules);
    assert_eq!(out.diagnostics, [], "{lines:?}");
    chests(only_structure(&out))
}

#[test]
fn chests_side_by_side_pair_two_by_two_from_their_own_left() {
    // Facing into the struct, the chests' own left is the east end of the back wall, `offset=1`.
    let inside = paired(&[
        "chest at=inside.back offset=1 y=1",
        "chest at=inside.back offset=2 y=1",
        "chest at=inside.back offset=3 y=1",
    ]);
    let south = |kind| format!("minecraft:chest[facing=south,type={kind}]");
    assert_eq!(
        inside,
        [
            ((4, 1, 2), south("single")),
            ((5, 1, 2), south("right")),
            ((6, 1, 2), south("left")),
        ],
    );
    // Facing out of it, their own left is the west end.
    let outside = paired(&[
        "chest at=back.outside offset=0 y=1",
        "chest at=back.outside offset=1 y=1",
    ]);
    let north = |kind| format!("minecraft:chest[facing=north,type={kind}]");
    assert_eq!(
        outside,
        [((6, 1, 0), north("left")), ((7, 1, 0), north("right"))],
    );
}

#[test]
fn a_chest_that_writes_its_type_faces_another_way_or_is_another_block_pairs_with_none() {
    let south = |kind| format!("minecraft:chest[facing=south,type={kind}]");
    assert_eq!(
        paired(&[
            "chest at=inside.back offset=1 y=1 size=single",
            "chest at=inside.back offset=2 y=1",
            "chest at=inside.back offset=3 y=1",
        ]),
        [
            ((4, 1, 2), south("right")),
            ((5, 1, 2), south("left")),
            ((6, 1, 2), south("single")),
        ],
    );
    assert_eq!(
        paired(&[
            "chest at=inside.back offset=1 y=1 facing=out",
            "chest at=inside.back offset=2 y=1",
        ]),
        [
            ((5, 1, 2), south("single")),
            (
                (6, 1, 2),
                "minecraft:chest[facing=north,type=single]".to_owned()
            ),
        ],
    );
    assert_eq!(
        paired(&[
            "chest at=inside.back offset=1 y=1 mat_slot=trap",
            "chest at=inside.back offset=2 y=1",
        ]),
        [
            ((5, 1, 2), south("single")),
            (
                (6, 1, 2),
                "minecraft:trapped_chest[facing=south,type=single]".to_owned()
            ),
        ],
    );
}
