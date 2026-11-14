use super::*;

/// Two targetable versions and one that is not.
fn versions() -> DataVersionTable {
    serde_json::from_str(
        r#"{
            "schema_version": 3,
            "latest": "1.2",
            "versions": [
                { "mc_version": "1.0", "data_version": 1, "targetable": false },
                { "mc_version": "1.1", "data_version": 2 },
                { "mc_version": "1.2", "data_version": 3 }
            ]
        }"#,
    )
    .expect("test table parses")
}

fn index(tables: &str) -> Result<PaintingIndex, PaintingsError> {
    let catalog: PaintingsCatalog = serde_json::from_str(&format!(
        r#"{{"schema_version": 1, "namespace": "minecraft", "tables": [{tables}]}}"#
    ))
    .expect("test catalog parses as JSON");
    PaintingIndex::from_catalog(catalog, &versions())
}

fn painting(spelling: &str, width: u32, height: u32) -> Painting {
    Painting {
        spelling: spelling.to_owned(),
        width,
        height,
    }
}

#[test]
fn each_version_has_the_paintings_of_every_table_that_names_it() {
    let index = index(
        r#"{"versions": ["1.1", "1.2"], "paintings": [
                {"variant": "kebab", "width": 1, "height": 1},
                {"variant": "skull_and_roses", "width": 2, "height": 2, "spelling": "SkullAndRoses"}
            ]},
            {"versions": ["1.2"], "paintings": [{"variant": "dennis", "width": 3, "height": 3}]}"#,
    )
    .expect("loads");
    assert_eq!(
        [
            index.at("1.1", "kebab"),
            index.at("1.1", "skull_and_roses"),
            index.at("1.1", "dennis"),
            index.at("1.2", "dennis"),
        ],
        [
            Some(&painting("minecraft:kebab", 1, 1)),
            Some(&painting("SkullAndRoses", 2, 2)),
            None,
            Some(&painting("minecraft:dennis", 3, 3)),
        ],
    );
    assert_eq!(
        index.variants_at("1.2").collect::<Vec<_>>(),
        ["kebab", "skull_and_roses", "dennis"],
    );
    assert_eq!(
        index
            .everywhere("dennis")
            .map(|(version, _)| version)
            .collect::<Vec<_>>(),
        ["1.2"],
    );
    assert_eq!(
        index.every_variant(),
        ["kebab", "skull_and_roses", "dennis"],
    );
    assert!(!index.is_empty());
}

#[test]
fn a_faulty_catalog_is_refused() {
    let one = r#"{"variant": "kebab", "width": 1, "height": 1}"#;
    let cases = [
        (
            r#"{"versions": [], "paintings": [{"variant": "kebab", "width": 1, "height": 1}]}"#
                .to_owned(),
            PaintingsError::EmptyTable {
                versions: String::new(),
            },
        ),
        (
            format!(r#"{{"versions": ["1.0", "1.1", "1.2"], "paintings": [{one}]}}"#),
            PaintingsError::UntargetableVersion {
                version: "1.0".to_owned(),
            },
        ),
        (
            format!(r#"{{"versions": ["1.1", "1.2"], "paintings": [{one}, {one}]}}"#),
            PaintingsError::DuplicateVariant {
                variant: "kebab".to_owned(),
                version: "1.1".to_owned(),
            },
        ),
        (
            format!(r#"{{"versions": ["1.2"], "paintings": [{one}]}}"#),
            PaintingsError::VersionWithoutPaintings {
                version: "1.1".to_owned(),
            },
        ),
        (
            r#"{"versions": ["1.1", "1.2"], "paintings": [
                {"variant": "minecraft:kebab", "width": 1, "height": 1}]}"#
                .to_owned(),
            PaintingsError::BadVariant {
                variant: "minecraft:kebab".to_owned(),
            },
        ),
        (
            r#"{"versions": ["1.1", "1.2"], "paintings": [
                {"variant": "kebab", "width": 17, "height": 1}]}"#
                .to_owned(),
            PaintingsError::BadSize {
                variant: "kebab".to_owned(),
                side: "width",
                value: 17,
            },
        ),
        (
            r#"{"versions": ["1.1", "1.2"], "paintings": [
                {"variant": "kebab", "width": 1, "height": 1, "spelling": " "}]}"#
                .to_owned(),
            PaintingsError::EmptySpelling {
                variant: "kebab".to_owned(),
            },
        ),
    ];
    for (tables, expected) in cases {
        assert_eq!(index(&tables).err(), Some(expected), "{tables}");
    }
}

#[test]
fn a_schema_newer_than_this_build_is_refused() {
    let catalog: PaintingsCatalog =
        serde_json::from_str(r#"{"schema_version": 2, "namespace": "minecraft", "tables": []}"#)
            .expect("parses");
    assert_eq!(
        PaintingIndex::from_catalog(catalog, &versions()).err(),
        Some(PaintingsError::UnsupportedSchemaVersion {
            got: 2,
            supported: SUPPORTED_PAINTINGS_SCHEMA,
        }),
    );
}

#[test]
fn an_empty_index_has_no_painting() {
    let index = PaintingIndex::empty();
    assert!(index.is_empty());
    assert_eq!(index.at("1.1", "kebab"), None);
    assert_eq!(index.every_variant(), Vec::<&str>::new());
}
