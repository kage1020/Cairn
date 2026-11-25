use indexmap::IndexMap;

use super::*;

fn index(rows: &serde_json::Value) -> Result<ConductorIndex, ConductorsError> {
    let catalog: ConductorsCatalog = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "rows": rows,
    }))
    .expect("the catalog deserialises");
    ConductorIndex::from_catalog(catalog)
}

/// The problem [`ConductorsError::MalformedRow`] gives for the one row `row`, whose first id is
/// `oak_slab`.
fn malformed(row: &serde_json::Value) -> String {
    match index(&serde_json::json!([row])) {
        Err(ConductorsError::MalformedRow { id, problem }) if id == "oak_slab" => problem,
        other => panic!("expected a malformed `oak_slab` row, got {other:?}"),
    }
}

fn properties(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn slabs() -> ConductorIndex {
    index(&serde_json::json!([
        {"mc_versions": ["1.0", "2.0"], "ids": ["stone"]},
        {
            "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
            "conducts": {"reads": ["type"], "map": {"bottom": false, "double": true, "top": false}},
        },
    ]))
    .expect("valid")
}

#[test]
fn a_block_a_row_names_without_a_lookup_conducts_in_every_state() {
    let tables = slabs();
    assert_eq!(
        tables.conducts("1.0", "minecraft:stone", &properties(&[])),
        Some(true)
    );
    assert_eq!(
        tables.conducts("2.0", "minecraft:stone", &properties(&[("any", "value")])),
        Some(true),
    );
}

#[test]
fn a_lookup_reads_the_written_values_and_the_defaults_for_the_rest() {
    let tables = slabs();
    let conducts =
        |pairs: &[(&str, &str)]| tables.conducts("1.0", "minecraft:oak_slab", &properties(pairs));
    assert_eq!(conducts(&[]), Some(false));
    assert_eq!(conducts(&[("type", "top")]), Some(false));
    assert_eq!(conducts(&[("type", "double")]), Some(true));
    assert_eq!(conducts(&[("type", "sideways")]), Some(false));
}

#[test]
fn a_block_no_row_names_does_not_conduct_and_a_version_without_a_table_says_nothing() {
    let tables = slabs();
    assert_eq!(
        tables.conducts("1.0", "minecraft:glass", &properties(&[])),
        Some(false)
    );
    assert_eq!(
        tables.conducts(
            "2.0",
            "minecraft:oak_slab",
            &properties(&[("type", "double")])
        ),
        Some(false),
    );
    assert_eq!(
        tables.conducts("3.0", "minecraft:stone", &properties(&[])),
        None
    );
}

#[test]
fn a_block_two_rows_describe_at_one_version_is_refused() {
    assert_eq!(
        index(&serde_json::json!([
            {"mc_versions": ["1.0", "2.0"], "ids": ["stone"]},
            {"mc_versions": ["2.0"], "ids": ["minecraft:stone"]},
        ]))
        .err(),
        Some(ConductorsError::DuplicateId {
            mc_version: "2.0".to_owned(),
            id: "minecraft:stone".to_owned(),
        }),
    );
}

#[test]
fn a_lookup_whose_defaults_and_reads_differ_is_refused() {
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"],
            "conducts": {"reads": ["type"], "map": {"double": true}},
        })),
        "the lookup reads `type`, which `defaults` does not give",
    );
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"],
            "defaults": {"type": "bottom", "waterlogged": "false"},
            "conducts": {"reads": ["type"], "map": {"double": true}},
        })),
        "`defaults` gives `waterlogged`, which the lookup does not read",
    );
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
        })),
        "`defaults` gives `type`, but the row has no `conducts` lookup to read it",
    );
}

#[test]
fn a_lookup_that_reads_nothing_maps_nothing_or_conducts_nowhere_is_refused() {
    let empty = "the `conducts` lookup reads no property or maps no value; a row without one \
                 conducts in every state";
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"],
            "conducts": {"reads": [], "map": {"": true}},
        })),
        empty,
    );
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
            "conducts": {"reads": ["type"], "map": {}},
        })),
        empty,
    );
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
            "conducts": {"reads": ["type"], "map": {"bottom": false, "top": false}},
        })),
        "the lookup maps no state to `true`; a block that conducts in no state is left out",
    );
}

#[test]
fn a_key_of_the_wrong_arity_is_refused() {
    assert_eq!(
        malformed(&serde_json::json!({
            "mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
            "conducts": {"reads": ["type"], "map": {"double,false": true}},
        })),
        "the `conducts` key `double,false` holds no value for each of type",
    );
}

#[test]
fn a_catalog_without_rows_or_a_row_without_blocks_or_versions_is_refused() {
    assert_eq!(
        index(&serde_json::json!([])).err(),
        Some(ConductorsError::NoRows)
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
        Some(ConductorsError::EmptyRow {
            index: 1,
            missing: "block"
        }),
    );
    assert_eq!(
        row(&[], &["dirt"]),
        Some(ConductorsError::EmptyRow {
            index: 1,
            missing: "version"
        }),
    );
}

#[test]
fn a_catalog_newer_than_this_build_is_refused() {
    let catalog: ConductorsCatalog = serde_json::from_value(serde_json::json!({
        "schema_version": SUPPORTED_CONDUCTORS_SCHEMA + 1,
        "namespace": "minecraft",
        "rows": [{"mc_versions": ["1.0"], "ids": ["stone"]}],
    }))
    .expect("the catalog deserialises");
    assert_eq!(
        ConductorIndex::from_catalog(catalog).err(),
        Some(ConductorsError::UnsupportedSchemaVersion {
            got: SUPPORTED_CONDUCTORS_SCHEMA + 1,
            supported: SUPPORTED_CONDUCTORS_SCHEMA,
        }),
    );
}

#[test]
fn a_row_with_a_field_of_its_own_does_not_deserialise() {
    let catalog = serde_json::from_value::<ConductorsCatalog>(serde_json::json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "rows": [{"mc_versions": ["1.0"], "ids": ["stone"], "kind": "full"}],
    }));
    assert!(catalog.is_err(), "{catalog:?}");
}

#[test]
fn the_states_a_table_names_are_its_defaults_and_its_lookup_keys() {
    let tables = slabs();
    let mut named: Vec<_> = tables.named_states("1.0").collect();
    named.sort_unstable();
    assert_eq!(
        named,
        [
            ("minecraft:oak_slab", "type", "bottom"),
            ("minecraft:oak_slab", "type", "bottom"),
            ("minecraft:oak_slab", "type", "double"),
            ("minecraft:oak_slab", "type", "top"),
        ],
    );
    assert_eq!(tables.named_states("2.0").count(), 0);
    assert_eq!(tables.named_states("3.0").count(), 0);
}
