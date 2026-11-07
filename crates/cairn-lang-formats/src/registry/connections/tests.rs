use cairn_lang_core::block_array::{Face, Faces, JoinKind, Joins};
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
        {"mc_versions": ["1.1"], "ids": ["stone"], "exception": true},
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
