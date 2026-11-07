use super::*;

fn parse(src: &str) -> Result<BlocksIndex, BlocksError> {
    let catalog: BlocksCatalog = serde_json::from_str(src).expect("test catalog parses as JSON");
    BlocksIndex::from_catalog(catalog)
}

const TWO_VERSIONS: &str = r#"{
    "schema_version": 1,
    "namespace": "minecraft",
    "base": { "mc_version": "1.0", "blocks": ["stonebrick", "cobblestone"] },
    "diffs": [
        { "mc_version": "1.1", "inherits": "1.0",
          "added": ["stone_bricks"], "removed": ["stonebrick"] }
    ]
}"#;

#[test]
fn base_is_namespaced_and_sorted() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(
        index.ids_for("1.0"),
        Some(
            [
                "minecraft:cobblestone".to_owned(),
                "minecraft:stonebrick".to_owned()
            ]
            .as_slice()
        ),
    );
}

#[test]
fn a_diff_applies_its_rename_to_that_version_only() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    let old = index.ids_for("1.0").expect("base version");
    let new = index.ids_for("1.1").expect("diffed version");
    assert!(old.contains(&"minecraft:stonebrick".to_owned()));
    assert!(!old.contains(&"minecraft:stone_bricks".to_owned()));
    assert!(new.contains(&"minecraft:stone_bricks".to_owned()));
    assert!(!new.contains(&"minecraft:stonebrick".to_owned()));
}

#[test]
fn versions_are_listed_in_resolution_order() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(index.versions().collect::<Vec<_>>(), vec!["1.0", "1.1"]);
    assert_eq!(index.len(), 2);
}

#[test]
fn a_version_the_catalog_never_names_has_no_table() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert!(index.ids_for("1.2").is_none());
}

#[test]
fn an_entry_may_carry_its_own_namespace() {
    let index = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["create:cogwheel"] }
    }"#,
    )
    .expect("catalog");
    assert_eq!(
        index.ids_for("1.0"),
        Some(["create:cogwheel".to_owned()].as_slice()),
    );
}

#[test]
fn the_empty_index_describes_no_version() {
    let index = BlocksIndex::empty();
    assert!(index.is_empty());
    assert!(index.ids_for("1.0").is_none());
}

#[test]
fn an_id_only_one_version_declares_is_still_declared_by_the_edition() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(
        index.declared_by_some_version("minecraft:stonebrick"),
        Some(true),
        "declared by 1.0 alone",
    );
    assert_eq!(
        index.declared_by_some_version("minecraft:stone_bricks"),
        Some(true),
        "declared by 1.1 alone",
    );
    assert_eq!(
        index.declared_by_some_version("minecraft:cobblestone"),
        Some(true),
        "declared by both",
    );
}

#[test]
fn an_id_no_version_declares_is_absent_from_the_edition() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(
        index.declared_by_some_version("minecraft:oak_sign"),
        Some(false),
    );
}

#[test]
fn a_prefix_of_a_declared_id_is_not_itself_declared() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(
        index.declared_by_some_version("minecraft:stone"),
        Some(false),
    );
}

#[test]
fn an_index_with_no_tables_answers_nothing_rather_than_no() {
    assert_eq!(
        BlocksIndex::empty().declared_by_some_version("minecraft:stone"),
        None,
    );
}

#[test]
fn an_unsupported_schema_version_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 999,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] }
    }"#,
    )
    .expect_err("unsupported schema");
    assert_eq!(
        err,
        BlocksError::UnsupportedSchemaVersion {
            got: 999,
            supported: SUPPORTED_BLOCKS_SCHEMA,
        },
    );
}

#[test]
fn an_empty_base_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": [] }
    }"#,
    )
    .expect_err("empty base");
    assert_eq!(
        err,
        BlocksError::EmptyBase {
            version: "1.0".to_owned(),
        },
    );
}

#[test]
fn a_repeated_id_in_the_base_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone", "stone"] }
    }"#,
    )
    .expect_err("duplicate id");
    assert_eq!(
        err,
        BlocksError::DuplicateBlock {
            version: "1.0".to_owned(),
            id: "minecraft:stone".to_owned(),
        },
    );
}

#[test]
fn a_repeated_version_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.0", "inherits": "1.0", "added": ["dirt"] }
        ]
    }"#,
    )
    .expect_err("duplicate version");
    assert_eq!(
        err,
        BlocksError::DuplicateVersion {
            version: "1.0".to_owned(),
        },
    );
}

#[test]
fn a_diff_that_inherits_a_later_version_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.2", "inherits": "1.1", "added": ["dirt"] },
            { "mc_version": "1.1", "inherits": "1.0", "added": ["gravel"] }
        ]
    }"#,
    )
    .expect_err("forward inherits");
    assert_eq!(
        err,
        BlocksError::UnknownInherits {
            version: "1.2".to_owned(),
            inherits: "1.1".to_owned(),
        },
    );
}

#[test]
fn removing_an_id_the_parent_lacks_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0", "removed": ["granite"] }
        ]
    }"#,
    )
    .expect_err("removed not in parent");
    assert_eq!(
        err,
        BlocksError::RemovedNotInParent {
            version: "1.1".to_owned(),
            inherits: "1.0".to_owned(),
            id: "minecraft:granite".to_owned(),
        },
    );
}

#[test]
fn adding_an_id_the_parent_already_has_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0", "added": ["stone"] }
        ]
    }"#,
    )
    .expect_err("added already in parent");
    assert_eq!(
        err,
        BlocksError::AddedAlreadyInParent {
            version: "1.1".to_owned(),
            inherits: "1.0".to_owned(),
            id: "minecraft:stone".to_owned(),
        },
    );
}

#[test]
fn a_removal_and_a_re_add_of_the_same_id_both_apply() {
    // Removing and re-adding an id is how a diff says "same name, new meaning".
    let index = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0",
              "added": ["stone"], "removed": ["stone"] }
        ]
    }"#,
    )
    .expect("catalog");
    assert_eq!(
        index.ids_for("1.1"),
        Some(["minecraft:stone".to_owned()].as_slice()),
    );
}

#[test]
fn a_diff_that_removes_everything_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0", "removed": ["stone"] }
        ]
    }"#,
    )
    .expect_err("folds to nothing");
    assert_eq!(
        err,
        BlocksError::EmptyTable {
            version: "1.1".to_owned(),
        },
    );
}

#[test]
fn a_chain_of_diffs_folds_through_its_middle_version() {
    let index = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0", "added": ["dirt"] },
            { "mc_version": "1.2", "inherits": "1.1", "added": ["gravel"] }
        ]
    }"#,
    )
    .expect("catalog");
    let last = index.ids_for("1.2").expect("last version");
    assert_eq!(
        last,
        [
            "minecraft:dirt".to_owned(),
            "minecraft:gravel".to_owned(),
            "minecraft:stone".to_owned(),
        ],
    );
}

const WITH_STATES: &str = r#"{
    "schema_version": 2,
    "namespace": "minecraft",
    "base": { "mc_version": "1.0", "blocks": {
        "oak_log": { "axis": ["x", "y", "z"] },
        "stone": {}
    } },
    "diffs": [
        { "mc_version": "1.1", "inherits": "1.0",
          "added": { "lever": { "powered": [false, true], "facing_direction": [0, 1] } },
          "changed": { "oak_log": { "pillar_axis": ["y", "x", "z"] } },
          "removed": ["stone"] }
    ]
}"#;

#[test]
fn a_block_reads_its_states_in_the_tables_order() {
    let index = parse(WITH_STATES).expect("catalog");
    let log = index
        .states_of("1.0", "minecraft:oak_log")
        .expect("oak_log has states");
    let axis: Vec<String> = log
        .domain("axis")
        .expect("axis")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(axis, ["x", "y", "z"]);
    assert!(
        index
            .states_of("1.0", "minecraft:stone")
            .is_some_and(BlockStates::is_empty)
    );
}

#[test]
fn a_bedrock_value_keeps_its_type_and_prints_as_a_literal_writes_it() {
    let index = parse(WITH_STATES).expect("catalog");
    let lever = index
        .states_of("1.1", "minecraft:lever")
        .expect("lever has states");
    assert_eq!(
        lever.properties().collect::<Vec<_>>(),
        [
            (
                "powered",
                [StateValue::Bool(false), StateValue::Bool(true)].as_slice()
            ),
            (
                "facing_direction",
                [StateValue::Int(0), StateValue::Int(1)].as_slice()
            ),
        ],
    );
    assert_eq!(StateValue::Bool(true).to_string(), "true");
    assert_eq!(StateValue::Int(3).to_string(), "3");
}

#[test]
fn a_changed_block_reads_its_new_states_in_that_version_only() {
    let index = parse(WITH_STATES).expect("catalog");
    let old = index.states_of("1.0", "minecraft:oak_log").expect("1.0");
    let new = index.states_of("1.1", "minecraft:oak_log").expect("1.1");
    assert!(old.domain("axis").is_some() && old.domain("pillar_axis").is_none());
    assert!(new.domain("pillar_axis").is_some() && new.domain("axis").is_none());
}

#[test]
fn a_removed_block_has_no_states_in_the_version_that_removes_it() {
    let index = parse(WITH_STATES).expect("catalog");
    assert_eq!(index.states_of("1.1", "minecraft:stone"), None);
    assert_eq!(index.states_of("1.2", "minecraft:oak_log"), None);
}

#[test]
fn a_table_of_bare_ids_records_no_states() {
    let index = parse(TWO_VERSIONS).expect("catalog");
    assert_eq!(index.states_of("1.0", "minecraft:cobblestone"), None);
}

#[test]
fn since_and_until_are_the_first_and_last_versions_declaring_an_id() {
    let index = parse(
        r#"{
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": { "stone": {}, "chain": {} } },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0", "added": { "lantern": {} } },
            { "mc_version": "1.2", "inherits": "1.1", "removed": ["chain"],
              "added": { "iron_chain": {} } }
        ]
    }"#,
    )
    .expect("catalog");
    let span = |id: &str| (index.since(id), index.until(id));
    assert_eq!(span("minecraft:stone"), (Some("1.0"), Some("1.2")));
    assert_eq!(span("minecraft:lantern"), (Some("1.1"), Some("1.2")));
    assert_eq!(span("minecraft:chain"), (Some("1.0"), Some("1.1")));
    assert_eq!(span("minecraft:iron_chain"), (Some("1.2"), Some("1.2")));
    assert_eq!(span("minecraft:dirt"), (None, None));
}

#[test]
fn changing_an_id_the_parent_lacks_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": { "stone": {} } },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0",
              "changed": { "lever": { "powered": ["true", "false"] } } }
        ]
    }"#,
    )
    .expect_err("changed not in parent");
    assert_eq!(
        err,
        BlocksError::ChangedNotInParent {
            version: "1.1".to_owned(),
            inherits: "1.0".to_owned(),
            id: "minecraft:lever".to_owned(),
        },
    );
}

#[test]
fn a_property_with_no_value_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": { "lever": { "powered": [] } } }
    }"#,
    )
    .expect_err("empty domain");
    assert_eq!(
        err,
        BlocksError::EmptyDomain {
            version: "1.0".to_owned(),
            id: "minecraft:lever".to_owned(),
            property: "powered".to_owned(),
        },
    );
}
