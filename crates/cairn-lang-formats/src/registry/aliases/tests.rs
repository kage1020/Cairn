use super::*;

fn parse(src: &str) -> Result<AliasIndex, AliasError> {
    let catalog: AliasCatalog = serde_json::from_str(src).expect("test catalog parses as JSON");
    AliasIndex::from_catalog(catalog)
}

const RENAME_AND_SPLIT: &str = r#"{
    "schema_version": 1,
    "namespace": "minecraft",
    "groups": [
        { "spellings": ["stonebrick", "stone_bricks"] },
        { "spellings": ["light", "light_block_0", "light_block_1"] }
    ]
}"#;

#[test]
fn a_spelling_answers_with_its_whole_group() {
    let index = parse(RENAME_AND_SPLIT).expect("catalog");
    assert_eq!(
        index.spellings_of("minecraft:stonebrick"),
        [
            "minecraft:stonebrick".to_owned(),
            "minecraft:stone_bricks".to_owned()
        ],
    );
}

#[test]
fn the_group_answers_from_either_end() {
    // The rows carry no direction: which spelling is the old one is a
    // question about the target, and the target's block table is what
    // answers it.
    let index = parse(RENAME_AND_SPLIT).expect("catalog");
    assert_eq!(
        index.spellings_of("minecraft:stone_bricks"),
        index.spellings_of("minecraft:stonebrick"),
    );
}

#[test]
fn members_keep_the_order_the_file_wrote_them_in() {
    let index = parse(RENAME_AND_SPLIT).expect("catalog");
    assert_eq!(
        index.spellings_of("minecraft:light"),
        [
            "minecraft:light".to_owned(),
            "minecraft:light_block_0".to_owned(),
            "minecraft:light_block_1".to_owned(),
        ],
    );
}

#[test]
fn a_spelling_no_group_claims_answers_nothing() {
    let index = parse(RENAME_AND_SPLIT).expect("catalog");
    assert!(index.spellings_of("minecraft:oak_planks").is_empty());
}

#[test]
fn an_entry_may_carry_its_own_namespace() {
    let index = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [{ "spellings": ["create:cogwheel", "cogwheel"] }]
    }"#,
    )
    .expect("catalog");
    assert_eq!(
        index.spellings_of("create:cogwheel"),
        [
            "create:cogwheel".to_owned(),
            "minecraft:cogwheel".to_owned()
        ],
    );
}

#[test]
fn the_empty_index_claims_no_spelling() {
    let index = AliasIndex::empty();
    assert!(index.is_empty());
    assert_eq!(index.len(), 0);
    assert!(index.spellings_of("minecraft:light").is_empty());
}

#[test]
fn an_unsupported_schema_version_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 999,
        "namespace": "minecraft",
        "groups": [{ "spellings": ["a", "b"] }]
    }"#,
    )
    .expect_err("unsupported schema");
    assert_eq!(
        err,
        AliasError::UnsupportedSchemaVersion {
            got: 999,
            supported: SUPPORTED_ALIASES_SCHEMA,
        },
    );
}

#[test]
fn a_catalog_with_no_groups_is_refused() {
    let err = parse(r#"{ "schema_version": 1, "namespace": "minecraft", "groups": [] }"#)
        .expect_err("no groups");
    assert_eq!(err, AliasError::NoGroups);
}

#[test]
fn a_group_of_one_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [{ "spellings": ["light"] }]
    }"#,
    )
    .expect_err("group of one");
    assert_eq!(
        err,
        AliasError::GroupTooSmall {
            spellings: "minecraft:light".to_owned(),
        },
    );
}

#[test]
fn a_repeated_spelling_inside_one_group_is_refused() {
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [{ "spellings": ["light", "light"] }]
    }"#,
    )
    .expect_err("duplicate spelling");
    assert_eq!(
        err,
        AliasError::DuplicateSpelling {
            id: "minecraft:light".to_owned(),
        },
    );
}

#[test]
fn a_spelling_two_groups_claim_is_refused() {
    // Bedrock's `snow` is Java's `snow_block` and Java's `snow` is
    // Bedrock's `snow_layer`: the real shape of this mistake.
    let err = parse(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [
            { "spellings": ["snow_block", "snow"] },
            { "spellings": ["snow", "snow_layer"] }
        ]
    }"#,
    )
    .expect_err("two groups");
    assert_eq!(
        err,
        AliasError::SpellingInTwoGroups {
            id: "minecraft:snow".to_owned(),
        },
    );
}
