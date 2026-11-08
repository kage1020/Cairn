//! What the built-in packs' connection tables say a block is to the rules that join panes, fences
//! and walls (`spec/blockstate` "Connections").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockState, Covers, Face, Faces, JoinKind, Joins, TargetRegistry,
};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_nbt::tag::Tag;

use crate::common::{block_palette, lower_source, states_for};

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
        Some(Joins::default()),
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
    let mut written: Vec<String> = block_palette(&root)
        .iter()
        .map(|entry| {
            let Tag::Compound(entry) = entry else {
                panic!("palette entry: {entry:?}");
            };
            let (Some(Tag::String(name)), Some(Tag::Compound(states))) =
                (entry.entries.get("name"), entry.entries.get("states"))
            else {
                panic!("palette entry: {entry:?}");
            };
            let mut states: Vec<String> = states
                .entries
                .iter()
                .map(|(key, value)| match value {
                    Tag::Byte(v) => format!("{key}={v}"),
                    Tag::Int(v) => format!("{key}={v}"),
                    Tag::String(v) => format!("{key}={v}"),
                    other => panic!("{key}: {other:?}"),
                })
                .collect();
            states.sort();
            format!("{name}[{}]", states.join(","))
        })
        .collect();
    written.sort();
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
