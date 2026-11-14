//! What the built-in packs' connection tables say a block is to the rules that join panes, fences,
//! walls, chests and redstone dust (`spec/blockstate` "Connections").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockState, Covers, Face, Faces, JoinKind, Joins, TargetRegistry,
};
use cairn_lang_formats::bedrock_structure::{ParityNote, build_mcstructure_tag};
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_nbt::tag::Compound;

use crate::common::{lower_source, position_data, states_for, written_palette};

/// `minecraft:{id}` with `properties`.
fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: format!("minecraft:{id}"),
        properties: properties
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn joins(edition: Edition, version: Option<&str>, state: &BlockState) -> Option<Joins> {
    builtin_pack(edition).view(version).joins(state)
}

/// What a connecting block of `kind` is to the rules, with no whole face.
fn kind(kind: JoinKind) -> Joins {
    Joins {
        kind: Some(kind),
        ..Joins::default()
    }
}

/// The four sideways faces.
const SIDES: Faces = Faces::NONE
    .with(Face::North)
    .with(Face::East)
    .with(Face::South)
    .with(Face::West);

#[test]
fn a_java_target_reads_the_connecting_blocks_its_version_declares() {
    let at = |version, id| joins(Edition::Java, Some(version), &state(id, &[]));
    assert_eq!(at("26.3", "glass_pane"), Some(kind(JoinKind::Pane)));
    assert_eq!(at("26.3", "copper_bars"), Some(kind(JoinKind::Pane)));
    assert_eq!(at("1.21", "copper_bars"), None);
    assert_eq!(
        at("1.21", "oak_fence"),
        Some(kind(JoinKind::Fence { wooden: true }))
    );
    assert_eq!(
        at("1.21", "nether_brick_fence"),
        Some(kind(JoinKind::Fence { wooden: false }))
    );
    assert_eq!(
        at("1.21", "oak_fence_gate"),
        Some(kind(JoinKind::FenceGate {
            facing: Face::North,
            open: false
        })),
    );
    assert_eq!(at("1.21", "cobblestone_wall"), Some(kind(JoinKind::Wall)));
    assert_eq!(at("1.21", "air"), Some(Joins::default()));
}

#[test]
fn the_tuff_walls_behind_1_20_4s_experiment_are_walls_there_too() {
    for version in ["1.20.4", "1.21", "1.21.4", "26.3"] {
        assert_eq!(
            joins(Edition::Java, Some(version), &state("tuff_wall", &[])),
            Some(kind(JoinKind::Wall)),
            "{version}",
        );
    }
}

#[test]
fn a_face_is_sturdy_where_the_states_model_covers_it_whole() {
    let faces = |id, properties: &[(&str, &str)]| {
        joins(Edition::Java, Some("1.21"), &state(id, properties)).map(|joins| joins.sturdy)
    };
    assert_eq!(faces("cobblestone", &[]), Some(Faces::ALL));
    assert_eq!(faces("oak_slab", &[]), Some(Faces::NONE.with(Face::Down)));
    assert_eq!(
        faces("oak_slab", &[("type", "top")]),
        Some(Faces::NONE.with(Face::Up))
    );
    assert_eq!(faces("oak_slab", &[("type", "double")]), Some(Faces::ALL));
    // A stair facing east is whole on its back, the east face, and on its bottom.
    assert_eq!(
        faces("oak_stairs", &[("facing", "east")]),
        Some(Faces::NONE.with(Face::Down).with(Face::East)),
    );
    assert_eq!(faces("glass_pane", &[]), Some(Faces::NONE));
    let leaves = joins(Edition::Java, Some("1.21"), &state("oak_leaves", &[]));
    assert_eq!(
        leaves,
        Some(Joins {
            sturdy: Faces::ALL,
            covers: every_part(),
            never_joined_by_face: true,
            ..Joins::default()
        }),
    );
}

/// The post and every side of a wall.
fn every_part() -> Covers {
    Covers {
        post: true,
        sides: Face::SIDES.into_iter().collect(),
    }
}

#[test]
fn what_a_bottom_covers_of_a_wall_is_read_off_the_parts_of_the_model_resting_on_it() {
    let covers = |id, properties: &[(&str, &str)]| {
        joins(Edition::Java, Some("1.21"), &state(id, properties)).map(|joins| joins.covers)
    };
    let post = Covers {
        post: true,
        sides: Faces::NONE,
    };
    assert_eq!(covers("cobblestone", &[]), Some(every_part()));
    assert_eq!(covers("oak_slab", &[]), Some(every_part()));
    assert_eq!(
        covers("oak_slab", &[("type", "top")]),
        Some(Covers::default())
    );
    assert_eq!(covers("lantern", &[]), Some(post));
    assert_eq!(
        covers("lantern", &[("hanging", "true")]),
        Some(Covers::default())
    );
    assert_eq!(covers("flower_pot", &[]), Some(post));
    // A connecting block is read by its connection states instead.
    assert_eq!(covers("cobblestone_wall", &[]), Some(Covers::default()));
}

#[test]
fn an_unpinned_lowering_reads_what_every_version_declaring_the_block_agrees_on() {
    assert_eq!(
        joins(Edition::Java, None, &state("copper_bars", &[])),
        Some(kind(JoinKind::Pane))
    );
    assert_eq!(
        joins(Edition::Java, None, &state("tuff_wall", &[])),
        Some(kind(JoinKind::Wall))
    );
    assert_eq!(
        joins(Edition::Java, None, &state("no_such_block", &[])),
        None
    );
    // Its bottom covers the post alone from 1.21.4 on; the dust it joins never changes.
    assert_eq!(
        joins(Edition::Java, None, &state("redstone_torch", &[])),
        Some(Joins {
            raises_post: true,
            dust: SIDES,
            ..Joins::default()
        }),
    );
}

#[test]
fn a_bedrock_id_is_read_as_the_canonical_blocks_its_state_map_names() {
    let at = |version, id| joins(Edition::Bedrock, Some(version), &state(id, &[]));
    assert_eq!(at("1.21.0", "cobblestone_wall"), Some(kind(JoinKind::Wall)));
    assert_eq!(at("1.26.60", "glass_pane"), Some(kind(JoinKind::Pane)));
    assert_eq!(
        at("1.21.0", "fence_gate"),
        Some(kind(JoinKind::FenceGate {
            facing: Face::North,
            open: false
        })),
    );
    // Java's standing and wall torch both become it, and only the standing one raises a post.
    assert_eq!(at("1.21.0", "unlit_redstone_torch"), None);
}

#[test]
fn a_bedrock_id_several_java_blocks_become_is_read_as_those_that_take_its_states() {
    let wall_torch = state("unlit_redstone_torch", &[("facing", "north")]);
    assert_eq!(
        joins(Edition::Bedrock, Some("1.21.0"), &wall_torch),
        Some(Joins {
            dust: SIDES,
            ..Joins::default()
        }),
    );
}

#[test]
fn a_chest_is_read_with_the_way_it_faces_and_an_ender_chest_pairs_with_none() {
    let at = |edition, version, id, properties: &[(&str, &str)]| {
        joins(edition, Some(version), &state(id, properties))
    };
    let chest = |facing| Some(kind(JoinKind::Chest { facing }));
    assert_eq!(at(Edition::Java, "1.21", "chest", &[]), chest(Face::North));
    assert_eq!(
        at(
            Edition::Java,
            "1.21",
            "trapped_chest",
            &[("facing", "east")]
        ),
        Some(Joins {
            dust: SIDES,
            ..kind(JoinKind::Chest { facing: Face::East })
        }),
    );
    assert_eq!(
        at(Edition::Java, "26.3", "copper_chest", &[]),
        chest(Face::North)
    );
    assert_eq!(at(Edition::Java, "1.21", "copper_chest", &[]), None);
    assert_eq!(
        at(Edition::Java, "1.21", "ender_chest", &[]),
        Some(Joins::default())
    );
    assert_eq!(
        at(Edition::Bedrock, "1.21.0", "chest", &[]),
        chest(Face::North)
    );
}

#[test]
fn dust_joins_a_signal_source_on_every_side_and_a_repeater_or_observer_only_where_it_reads_or_drives()
 {
    let at = |id, properties: &[(&str, &str)]| {
        joins(Edition::Java, Some("1.21"), &state(id, properties)).map(|joins| joins.dust)
    };
    assert_eq!(
        joins(Edition::Java, Some("1.21"), &state("redstone_wire", &[])),
        Some(kind(JoinKind::Dust)),
    );
    assert_eq!(at("lever", &[]), Some(SIDES));
    assert_eq!(at("oak_button", &[]), Some(SIDES));
    assert_eq!(at("redstone_torch", &[]), Some(SIDES));
    assert_eq!(at("redstone_block", &[]), Some(SIDES));
    assert_eq!(
        at("repeater", &[("facing", "east")]),
        Some(Faces::NONE.with(Face::East).with(Face::West)),
    );
    assert_eq!(
        at("observer", &[("facing", "east")]),
        Some(Faces::NONE.with(Face::West)),
    );
    assert_eq!(at("observer", &[]), Some(Faces::NONE.with(Face::North)));
    assert_eq!(at("comparator", &[("facing", "east")]), Some(SIDES));
    assert_eq!(
        at("redstone_wall_torch", &[("facing", "east")]),
        Some(SIDES)
    );
    assert_eq!(at("stone_pressure_plate", &[]), Some(SIDES));
    assert_eq!(at("lightning_rod", &[("facing", "up")]), Some(SIDES));
    assert_eq!(at("stone", &[]), Some(Faces::NONE));
}

#[test]
fn a_pinned_java_build_joins_dust_to_the_dust_and_signal_sources_beside_it() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  walls mat_slot=wall height=3\n  \
                  redstone_dust at=1,1,1\n  redstone_dust at=2,1,1\n  \
                  repeater at=3,1,1 facing=east\n  raw_block mat=minecraft:lever at=1,1,2\n";
    let out = lower_source(
        source,
        Edition::Java,
        Some(crate::common::latest(Edition::Java)),
    );
    assert_eq!(out.diagnostics, []);
    let ba = &out.structures["struct::s"];
    let dust: Vec<((u32, u32, u32), String)> = ba
        .voxels
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            let state = &ba.palette.entries[usize::from(slot.0)];
            (state.id == "minecraft:redstone_wire").then(|| {
                let cell = ba.dims.position(i).expect("a voxel of the array");
                (cell, state.properties_text())
            })
        })
        .collect();
    assert_eq!(
        dust,
        [
            (
                (1, 1, 1),
                "east=side,north=none,south=side,west=none".to_owned()
            ),
            (
                (2, 1, 1),
                "east=side,north=none,south=none,west=side".to_owned()
            ),
        ],
    );
}

#[test]
fn a_bedrock_target_writes_the_connections_a_wall_and_its_gate_derive() {
    let source = "theme t:\n  slot wall -> @cobblestone_wall\n  slot gate -> @fence_gate\n\n\
                  struct s size=5x3\n  walls mat_slot=wall height=1\n  \
                  window side=front offset=1 y=1 size=1x1 mat_slot=gate\n";
    let version = "1.21.0";
    let out = lower_source(source, Edition::Bedrock, Some(version));
    assert_eq!(out.diagnostics, []);
    let target = resolve_bedrock_target(version).expect("known target");
    let (root, notes) =
        build_mcstructure_tag(&out.structures["struct::s"], &target, &states_for(&target))
            .expect("build");
    assert_eq!(notes, []);
    let written = written_palette(&root);
    let wall = |east, north, south, west, post| {
        format!(
            "minecraft:cobblestone_wall[wall_block_type=cobblestone,\
             wall_connection_type_east={east},wall_connection_type_north={north},\
             wall_connection_type_south={south},wall_connection_type_west={west},\
             wall_post_bit={post}]"
        )
    };
    // Four corners, a straight run each way with its post down, and the gate set into the front.
    assert_eq!(
        written,
        [
            "minecraft:air[]".to_owned(),
            wall("none", "none", "short", "short", 1),
            wall("none", "short", "none", "short", 1),
            wall("none", "short", "short", "none", 0),
            wall("short", "none", "none", "short", 0),
            wall("short", "none", "short", "none", 1),
            wall("short", "short", "none", "none", 1),
            "minecraft:fence_gate[direction=2,in_wall_bit=1,open_bit=0]".to_owned(),
        ],
    );
}

/// `source`'s `struct::s` written at Bedrock `version`, with the notes the writer raised.
fn bedrock_root(source: &str, version: &str) -> (Compound, Vec<ParityNote>) {
    let out = lower_source(source, Edition::Bedrock, Some(version));
    assert_eq!(out.diagnostics, []);
    let target = resolve_bedrock_target(version).expect("known target");
    build_mcstructure_tag(&out.structures["struct::s"], &target, &states_for(&target))
        .expect("build")
}

#[test]
fn a_bedrock_target_pairs_the_halves_of_a_chest_pair_through_their_block_entities() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=5x3\n  walls mat_slot=wall height=3\n  \
                  chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1\n";
    let (root, notes) = bedrock_root(source, "1.21.0");
    assert_eq!(notes, []);
    assert_eq!(
        written_palette(&root),
        [
            "minecraft:air[]",
            "minecraft:chest[minecraft:cardinal_direction=south]",
            "minecraft:cobblestone[]",
        ],
    );
    // The left half, facing south, has its other half to the west, and leads the pair.
    assert_eq!(
        position_data(&root),
        [
            (
                "28".to_owned(),
                "{block_entity_data:{id:\"Chest\",pairlead:0b,pairx:3,pairz:1,x:2,y:1,z:1}}"
                    .to_owned()
            ),
            (
                "40".to_owned(),
                "{block_entity_data:{id:\"Chest\",pairlead:1b,pairx:2,pairz:1,x:3,y:1,z:1}}"
                    .to_owned()
            ),
        ],
    );
}

#[test]
fn a_chest_half_without_its_other_half_beside_it_stands_single_on_bedrock() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=5x3\n  walls mat_slot=wall height=3\n  \
                  raw_block mat=minecraft:chest[facing=south,type=left] at=3,1,1\n  \
                  raw_block mat=minecraft:trapped_chest[facing=south,type=right] at=2,1,1\n  \
                  raw_block mat=minecraft:chest[facing=south,type=left] at=1,2,1\n";
    let (root, notes) = bedrock_root(source, "1.21.0");
    assert_eq!(notes, []);
    assert_eq!(position_data(&root), []);
}

#[test]
fn a_chest_pair_facing_east_pairs_along_z_on_bedrock() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=3x5\n  walls mat_slot=wall height=3\n  \
                  chest at=inside.left offset=1 y=1\n  chest at=inside.left offset=2 y=1\n";
    let (root, notes) = bedrock_root(source, "1.21.0");
    assert_eq!(notes, []);
    assert_eq!(
        written_palette(&root),
        [
            "minecraft:air[]",
            "minecraft:chest[minecraft:cardinal_direction=east]",
            "minecraft:cobblestone[]",
        ],
    );
    // The left half, facing east, has its other half to the south.
    assert_eq!(
        position_data(&root),
        [
            (
                "26".to_owned(),
                "{block_entity_data:{id:\"Chest\",pairlead:1b,pairx:1,pairz:2,x:1,y:1,z:1}}"
                    .to_owned()
            ),
            (
                "27".to_owned(),
                "{block_entity_data:{id:\"Chest\",pairlead:0b,pairx:1,pairz:1,x:1,y:1,z:2}}"
                    .to_owned()
            ),
        ],
    );
}
