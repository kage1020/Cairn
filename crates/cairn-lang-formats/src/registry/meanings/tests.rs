use super::*;

fn parse(src: &str) -> Result<MeaningIndex, MeaningsError> {
    let catalog: MeaningsCatalog = serde_json::from_str(src).expect("test catalog parses as JSON");
    MeaningIndex::from_catalog(catalog)
}

const CAULDRON: &str = r#"{
    "schema_version": 1,
    "namespace": "minecraft",
    "tokens": [
        { "token": "cauldron", "meaning": "fluid",
          "blocks": { "water": "water_cauldron", "lava": "lava_cauldron" } }
    ]
}"#;

fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: id.to_owned(),
        properties: properties
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

#[test]
fn a_meaning_token_names_its_block_and_leaves_the_other_states() {
    let index = parse(CAULDRON).expect("catalog");
    assert_eq!(
        index.meant(&state(
            "minecraft:cauldron",
            &[("fluid", "water"), ("level", "3")]
        )),
        Some(state("minecraft:water_cauldron", &[("level", "3")])),
    );
}

#[test]
fn a_value_the_row_lacks_or_a_token_without_one_names_nothing() {
    let index = parse(CAULDRON).expect("catalog");
    assert_eq!(
        index.meant(&state("minecraft:cauldron", &[("fluid", "milk")])),
        None
    );
    assert_eq!(index.meant(&state("minecraft:cauldron", &[])), None);
    assert_eq!(
        index.meant(&state("minecraft:oak_planks", &[("fluid", "water")])),
        None
    );
}

#[test]
fn the_meaning_token_is_offered_with_its_values() {
    let index = parse(CAULDRON).expect("catalog");
    assert_eq!(
        index.meaning_of("minecraft:cauldron"),
        Some(("fluid", vec!["water".to_owned(), "lava".to_owned()])),
    );
    assert_eq!(index.meaning_of("minecraft:water_cauldron"), None);
}

#[test]
fn a_catalog_is_refused_for_each_fault() {
    let cases = [
        (
            r#"{"schema_version": 2, "namespace": "minecraft", "tokens": []}"#,
            MeaningsError::UnsupportedSchemaVersion {
                got: 2,
                supported: 1,
            },
        ),
        (
            r#"{"schema_version": 1, "namespace": "minecraft", "tokens": []}"#,
            MeaningsError::NoTokens,
        ),
        (
            r#"{"schema_version": 1, "namespace": "minecraft",
                "tokens": [{"token": "cauldron", "meaning": "fluid", "blocks": {}}]}"#,
            MeaningsError::NoBlocks {
                token: "minecraft:cauldron".to_owned(),
            },
        ),
        (
            r#"{"schema_version": 1, "namespace": "minecraft", "tokens": [
                {"token": "cauldron", "meaning": "fluid", "blocks": {"water": "water_cauldron"}},
                {"token": "cauldron", "meaning": "liquid", "blocks": {"lava": "lava_cauldron"}}
            ]}"#,
            MeaningsError::DuplicateToken {
                token: "minecraft:cauldron".to_owned(),
            },
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(parse(src).expect_err("refused"), expected, "{src}");
    }
}
