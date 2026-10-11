use super::*;

fn parse(src: &str) -> Result<MaterialsIndex, MaterialsError> {
    let catalog: MaterialsCatalog = serde_json::from_str(src).expect("test catalog parses as JSON");
    MaterialsIndex::from_catalog(catalog)
}

#[test]
fn parses_minimal_catalog() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "floor.wood.broadleaf", "block": "oak_planks" }
        ]
    }"#;
    let index = parse(src).expect("catalog");
    assert_eq!(index.len(), 1);
    assert_eq!(
        index.lookup_id("floor.wood.broadleaf"),
        Some("minecraft:oak_planks"),
    );
}

#[test]
fn lookup_resolves_known_token() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "wall.stone.cobble", "block": "cobblestone" }
        ]
    }"#;
    let index = parse(src).unwrap();
    assert_eq!(
        index.lookup_id("wall.stone.cobble"),
        Some("minecraft:cobblestone")
    );
}

#[test]
fn lookup_unknown_returns_none() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "wood.dark", "block": "dark_oak_planks" }
        ]
    }"#;
    let index = parse(src).unwrap();
    assert!(index.lookup_id("totally.unknown.token").is_none());
}

#[test]
fn explicit_namespace_on_entry_overrides_catalog_default() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "tech.cog", "block": "create:cogwheel" }
        ]
    }"#;
    let index = parse(src).unwrap();
    assert_eq!(index.lookup_id("tech.cog"), Some("create:cogwheel"));
}

#[test]
fn duplicate_token_rejected_at_load_time() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "floor.wood.broadleaf", "block": "oak_planks" },
            { "token": "floor.wood.broadleaf", "block": "spruce_planks" }
        ]
    }"#;
    let err = parse(src).unwrap_err();
    assert_eq!(
        err,
        MaterialsError::DuplicateMaterialEntry {
            token: "floor.wood.broadleaf".into(),
        },
    );
}

#[test]
fn duplicate_override_for_one_version_rejected_at_load_time() {
    // Same reason as a duplicate token: the silent outcome is "the last
    // row wins", which the pack author cannot see from the file.
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "floor.stone.smooth", "block": "stone_bricks",
              "overrides": [
                { "mc_version": "1.21.0", "block": "stonebrick" },
                { "mc_version": "1.21.0", "block": "stone" }
              ] }
        ]
    }"#;
    let err = parse(src).unwrap_err();
    assert_eq!(
        err,
        MaterialsError::DuplicateMaterialOverride {
            token: "floor.stone.smooth".into(),
            mc_version: "1.21.0".into(),
        },
    );
}

#[test]
fn unsupported_schema_version_is_error() {
    let src = r#"{
        "schema_version": 999,
        "namespace": "minecraft",
        "entries": []
    }"#;
    let err = parse(src).unwrap_err();
    assert!(matches!(
        err,
        MaterialsError::UnsupportedSchemaVersion {
            got: 999,
            supported: SUPPORTED_MATERIALS_SCHEMA,
        }
    ));
}

#[test]
fn empty_catalog_loads_with_zero_entries() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": []
    }"#;
    let index = parse(src).unwrap();
    assert!(index.is_empty());
    assert!(index.lookup_id("anything").is_none());
}

#[test]
fn tokens_preserves_insertion_order() {
    let src = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "a.b.c", "block": "stone" },
            { "token": "x.y.z", "block": "dirt" }
        ]
    }"#;
    let index = parse(src).unwrap();
    let order: Vec<&str> = index.tokens().collect();
    assert_eq!(order, vec!["a.b.c", "x.y.z"]);
}
