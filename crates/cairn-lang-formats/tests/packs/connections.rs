//! What the built-in packs' connection tables say a block is to the rules that join panes, fences,
//! walls and chests (`spec/blockstate` "Connections").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, Face, Faces, JoinKind, Joins, TargetRegistry};
use cairn_lang_formats::registry::builtin_pack;

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
            exception: true,
            ..Joins::default()
        }),
    );
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
        chest(Face::East),
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
