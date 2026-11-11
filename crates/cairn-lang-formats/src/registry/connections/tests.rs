use cairn_lang_core::block_array::{Covers, Face, Faces, JoinKind, Joins};
use indexmap::IndexMap;

use super::*;

fn index(rows: &serde_json::Value) -> Result<ConnectionIndex, ConnectionsError> {
    let catalog: ConnectionsCatalog = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "rows": rows,
    }))
    .expect("the catalog deserialises");
    ConnectionIndex::from_catalog(catalog)
}

/// The problem [`ConnectionsError::MalformedRow`] gives for the one row `row`, whose first id is
/// `stone`.
fn malformed(row: &serde_json::Value) -> String {
    match index(&serde_json::json!([row])) {
        Err(ConnectionsError::MalformedRow { id, problem }) if id == "stone" => problem,
        other => panic!("expected a malformed `stone` row, got {other:?}"),
    }
}

fn properties(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn a_lookup_reads_the_written_values_and_the_defaults_for_the_rest() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"],
        "ids": ["oak_slab"],
        "defaults": {"type": "bottom"},
        "sturdy": {"reads": ["type"], "map": {"bottom": ["down"], "top": ["up"], "double": []}},
    }]))
    .expect("valid");
    let faces = |pairs: &[(&str, &str)]| {
        tables
            .joins("1.0", "minecraft:oak_slab", &properties(pairs))
            .map(|joins| joins.sturdy)
    };
    assert_eq!(faces(&[]), Some(Faces::NONE.with(Face::Down)));
    assert_eq!(faces(&[("type", "top")]), Some(Faces::NONE.with(Face::Up)));
    assert_eq!(faces(&[("type", "sideways")]), Some(Faces::NONE));
}

#[test]
fn a_block_no_row_describes_is_nothing_and_a_version_without_a_table_says_nothing() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["stone"], "sturdy": ["down", "up"],
    }]))
    .expect("valid");
    let none = IndexMap::new();
    assert_eq!(
        tables.joins("1.0", "minecraft:air", &none),
        Some(Joins::default())
    );
    assert_eq!(tables.joins("2.0", "minecraft:stone", &none), None);
    assert_eq!(
        tables.joins("1.0", "minecraft:stone", &none),
        Some(Joins {
            sturdy: [Face::Down, Face::Up].into_iter().collect(),
            ..Joins::default()
        }),
    );
}

#[test]
fn a_gate_is_read_with_its_facing_and_whether_it_stands_open() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["oak_fence_gate"], "kind": "fence_gate",
        "defaults": {"facing": "north", "open": "false"},
    }]))
    .expect("valid");
    let kind = |pairs: &[(&str, &str)]| {
        tables
            .joins("1.0", "minecraft:oak_fence_gate", &properties(pairs))
            .and_then(|joins| joins.kind)
    };
    assert_eq!(
        kind(&[("open", "true")]),
        Some(JoinKind::FenceGate {
            facing: Face::North,
            open: true
        }),
    );
    assert_eq!(kind(&[("facing", "up")]), None);
}

#[test]
fn a_chest_is_read_with_the_way_it_faces() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["chest"], "kind": "chest",
        "defaults": {"facing": "north"},
    }]))
    .expect("valid");
    let kind = |pairs: &[(&str, &str)]| {
        tables
            .joins("1.0", "minecraft:chest", &properties(pairs))
            .and_then(|joins| joins.kind)
    };
    assert_eq!(
        kind(&[]),
        Some(JoinKind::Chest {
            facing: Face::North
        })
    );
    assert_eq!(
        kind(&[("facing", "west"), ("type", "left")]),
        Some(JoinKind::Chest { facing: Face::West }),
    );
    assert_eq!(kind(&[("facing", "down")]), None);
    let refused = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["chest"], "kind": "chest",
    }]));
    assert_eq!(
        refused.err(),
        Some(ConnectionsError::MalformedRow {
            id: "chest".to_owned(),
            problem: "the rules read `facing`, which `defaults` does not give".to_owned(),
        }),
    );
}

#[test]
fn a_row_whose_rules_read_a_property_without_a_default_is_refused() {
    let refused = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["oak_fence_gate"], "kind": "fence_gate",
        "defaults": {"facing": "north"},
    }]));
    assert_eq!(
        refused.err(),
        Some(ConnectionsError::MalformedRow {
            id: "oak_fence_gate".to_owned(),
            problem: "the rules read `open`, which `defaults` does not give".to_owned(),
        }),
    );
}

#[test]
fn a_block_two_rows_describe_at_one_version_is_refused() {
    let refused = index(&serde_json::json!([
        {"mc_versions": ["1.0", "1.1"], "ids": ["stone"], "sturdy": ["down"]},
        {"mc_versions": ["1.1"], "ids": ["stone"], "never_joined_by_face": true},
    ]));
    assert_eq!(
        refused.err(),
        Some(ConnectionsError::DuplicateId {
            mc_version: "1.1".to_owned(),
            id: "minecraft:stone".to_owned(),
        }),
    );
}

#[test]
fn a_face_that_is_no_direction_and_a_key_of_the_wrong_arity_are_refused() {
    let unknown = index(&serde_json::json!([
        {"mc_versions": ["1.0"], "ids": ["stone"], "sturdy": ["sideways"]},
    ]));
    assert_eq!(
        unknown.err(),
        Some(ConnectionsError::MalformedRow {
            id: "stone".to_owned(),
            problem: "`sideways` is no face".to_owned(),
        }),
    );
    let arity = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["oak_door"],
        "defaults": {"facing": "north", "open": "false"},
        "sturdy": {"reads": ["facing", "open"], "map": {"north": ["south"]}},
    }]));
    assert_eq!(
        arity.err(),
        Some(ConnectionsError::MalformedRow {
            id: "oak_door".to_owned(),
            problem: "the `sturdy` key `north` holds no value for each of facing, open".to_owned(),
        }),
    );
}

#[test]
fn a_lookup_of_covered_parts_reads_the_post_and_the_sides_by_state() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["campfire"], "defaults": {"facing": "north"},
        "covers": {"reads": ["facing"], "map": {"north": ["post", "north", "south"], "east": []}},
    }]))
    .expect("valid");
    let covers = |pairs: &[(&str, &str)]| {
        tables
            .joins("1.0", "minecraft:campfire", &properties(pairs))
            .map(|joins| joins.covers)
    };
    assert_eq!(
        covers(&[]),
        Some(Covers {
            post: true,
            sides: [Face::North, Face::South].into_iter().collect(),
        }),
    );
    assert_eq!(covers(&[("facing", "east")]), Some(Covers::default()));
}

#[test]
fn a_covered_part_that_is_no_part_of_a_wall_is_refused() {
    for part in ["up", "middle"] {
        assert_eq!(
            malformed(&serde_json::json!({
                "mc_versions": ["1.0"], "ids": ["stone"], "covers": ["post", part],
            })),
            format!("`{part}` is no part of a wall: `post`, or a side's direction"),
        );
    }
}

#[test]
fn a_dust_face_that_is_no_side_is_refused() {
    for face in ["up", "down", "middle"] {
        assert_eq!(
            malformed(&serde_json::json!({
                "mc_versions": ["1.0"], "ids": ["stone"], "dust": ["north", face],
            })),
            format!("`{face}` is no sideways face: `north`, `south`, `east` or `west`"),
        );
    }
}

#[test]
fn a_gate_whose_default_facing_or_open_is_unreadable_is_refused() {
    let gate = |facing: &str, open: &str| {
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["stone"], "kind": "fence_gate",
            "defaults": {"facing": facing, "open": open},
        }))
    };
    assert_eq!(
        gate("up", "false"),
        "the default `facing=up` is no sideways face"
    );
    assert_eq!(
        gate("north", "maybe"),
        "the default `open=maybe` is neither true nor false"
    );
}

#[test]
fn a_chest_whose_default_facing_is_no_sideways_face_is_refused() {
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["stone"], "kind": "chest",
            "defaults": {"facing": "up"},
        })),
        "the default `facing=up` is no sideways face"
    );
}

#[test]
fn a_default_no_rule_reads_is_refused() {
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["stone"], "defaults": {"axis": "y"},
        })),
        "`defaults` gives `axis`, which the rules do not read",
    );
}

#[test]
fn a_lookup_that_reads_nothing_or_maps_nothing_is_refused() {
    let problem = "the `sturdy` lookup reads no property or maps no value; a list says what holds \
                   in every state";
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["stone"],
            "sturdy": {"reads": [], "map": {"": ["down"]}},
        })),
        problem,
    );
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["stone"], "defaults": {"axis": "y"},
            "sturdy": {"reads": ["axis"], "map": {}},
        })),
        problem,
    );
}

#[test]
fn a_lookup_with_a_field_of_its_own_does_not_deserialise() {
    let catalog = serde_json::from_value::<ConnectionsCatalog>(serde_json::json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "rows": [{
            "mc_versions": ["1.0"], "ids": ["stone"], "defaults": {"axis": "y"},
            "sturdy": {"reads": ["axis"], "map": {"y": ["down"]}, "default": ["up"]},
        }],
    }));
    assert!(catalog.is_err(), "{catalog:?}");
}

#[test]
fn a_catalog_without_rows_or_a_row_without_blocks_or_versions_is_refused() {
    assert_eq!(
        index(&serde_json::json!([])).err(),
        Some(ConnectionsError::NoRows)
    );
    let row = |versions: &[&str], ids: &[&str]| {
        index(&serde_json::json!([
            {"mc_versions": ["1.0"], "ids": ["stone"]},
            {"mc_versions": versions, "ids": ids},
        ]))
        .err()
    };
    assert_eq!(
        row(&["1.0"], &[]),
        Some(ConnectionsError::EmptyRow {
            index: 1,
            missing: "block"
        }),
    );
    assert_eq!(
        row(&[], &["dirt"]),
        Some(ConnectionsError::EmptyRow {
            index: 1,
            missing: "version"
        }),
    );
}

#[test]
fn a_catalog_newer_than_this_build_is_refused() {
    let catalog: ConnectionsCatalog = serde_json::from_value(serde_json::json!({
        "schema_version": SUPPORTED_CONNECTIONS_SCHEMA + 1,
        "namespace": "minecraft",
        "rows": [{"mc_versions": ["1.0"], "ids": ["stone"]}],
    }))
    .expect("the catalog deserialises");
    assert_eq!(
        ConnectionIndex::from_catalog(catalog).err(),
        Some(ConnectionsError::UnsupportedSchemaVersion {
            got: SUPPORTED_CONNECTIONS_SCHEMA + 1,
            supported: SUPPORTED_CONNECTIONS_SCHEMA,
        }),
    );
}

#[test]
fn the_states_a_table_names_are_its_defaults_and_its_lookup_keys() {
    let tables = index(&serde_json::json!([{
        "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
        "sturdy": {"reads": ["type"], "map": {"top": ["up"]}},
        "covers": {"reads": ["type"], "map": {"double": ["post"]}},
    }]))
    .expect("valid");
    let mut named: Vec<_> = tables.named_states("1.0").collect();
    named.sort_unstable();
    assert_eq!(
        named,
        [
            ("minecraft:oak_slab", "type", "bottom"),
            ("minecraft:oak_slab", "type", "double"),
            ("minecraft:oak_slab", "type", "top"),
        ],
    );
    assert_eq!(tables.named_states("2.0").count(), 0);
}
