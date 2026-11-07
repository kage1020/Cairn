use super::*;

fn parse(src: &str) -> Result<AbsentIndex, AbsentError> {
    let catalog: AbsentCatalog = serde_json::from_str(src).expect("test catalog parses as JSON");
    AbsentIndex::from_catalog(catalog)
}

#[test]
fn every_id_of_an_entry_answers_with_its_reason() {
    let index = parse(
        r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "ids": ["allow", "deny"], "reason": "they gate building" },
                { "ids": ["other:camera"], "reason": "it takes photos" }
            ]
        }"#,
    )
    .expect("catalog");
    assert_eq!(
        [
            index.reason_for("minecraft:allow"),
            index.reason_for("minecraft:deny"),
            index.reason_for("other:camera"),
            index.reason_for("minecraft:camera"),
        ],
        [
            Some("they gate building"),
            Some("they gate building"),
            Some("it takes photos"),
            None,
        ],
    );
    assert_eq!(
        index.ids().collect::<Vec<_>>(),
        ["minecraft:allow", "minecraft:deny", "other:camera"],
    );
}

#[test]
fn a_faulty_catalog_is_refused_with_its_first_fault() {
    let cases = [
        (
            r#"{ "schema_version": 2, "namespace": "minecraft", "entries": [] }"#,
            AbsentError::UnsupportedSchemaVersion {
                got: 2,
                supported: 1,
            },
        ),
        (
            r#"{ "schema_version": 1, "namespace": "minecraft", "entries": [] }"#,
            AbsentError::NoEntries,
        ),
        (
            r#"{ "schema_version": 1, "namespace": "minecraft",
                 "entries": [{ "ids": [], "reason": "r" }] }"#,
            AbsentError::NoIds {
                reason: "r".to_owned(),
            },
        ),
        (
            r#"{ "schema_version": 1, "namespace": "minecraft",
                 "entries": [{ "ids": ["a", "b"], "reason": " " }] }"#,
            AbsentError::NoReason {
                ids: "a, b".to_owned(),
            },
        ),
        (
            r#"{ "schema_version": 1, "namespace": "minecraft",
                 "entries": [{ "ids": ["a"], "reason": "r" }, { "ids": ["minecraft:a"], "reason": "s" }] }"#,
            AbsentError::DuplicateId {
                id: "minecraft:a".to_owned(),
            },
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(parse(src).expect_err(src), expected, "{src}");
    }
}
