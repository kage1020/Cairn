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
