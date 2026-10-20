use strum::IntoEnumIterator;

use super::*;

/// Stable strings of every code the predicate accepts, sorted.
fn codes_where(predicate: impl Fn(DiagnosticCode) -> bool) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = DiagnosticCode::iter()
        .filter(|c| predicate(*c))
        .map(DiagnosticCode::as_str)
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn every_code_renders_its_documented_string() {
    assert_eq!(
        codes_where(|_| true),
        [
            "E_AMBIGUOUS_PORT",
            "E_CONNECT_ARITY",
            "E_DUPLICATE_ARG",
            "E_DUPLICATE_CIRCUIT",
            "E_DUPLICATE_HEADER",
            "E_DUPLICATE_ID",
            "E_DUPLICATE_ITEM",
            "E_DUPLICATE_PLACE_ID",
            "E_DUPLICATE_SELECTOR",
            "E_DUPLICATE_SIZE",
            "E_DUPLICATE_SLOT",
            "E_INCOMPATIBLE_MATERIAL",
            "E_INCOMPLETE_PLACE",
            "E_INTENDED_TARGET_CAP",
            "E_INVALID_PLACE_ID",
            "E_INVALID_PLACE_ORIGIN",
            "E_INVALID_REQUIRES",
            "E_INVALID_STRUCT_NAME",
            "E_MISPLACED_BINDING",
            "E_MISPLACED_MEMBER",
            "E_MISSING_MATERIAL",
            "E_MISSING_PATH_MATERIAL",
            "E_OUTPUT_NAME_COLLISION",
            "E_PARSE",
            "E_THEME_SELECTOR_UNMATCHED",
            "E_THEME_VARIANT_MISSING",
            "E_TRUTH_TABLE_CONFLICT",
            "E_TRUTH_TABLE_DUPLICATE_INPUT",
            "E_TRUTH_TABLE_EMPTY",
            "E_TYPE_MISMATCH_LABEL",
            "E_TYPE_MISMATCH_SIZE",
            "E_UNEXPECTED_POSITIONAL",
            "E_UNKNOWN_ABSTRACT_TOKEN",
            "E_UNKNOWN_ARGUMENT",
            "E_UNKNOWN_ID",
            "E_UNKNOWN_KEYWORD",
            "E_UNKNOWN_SLOT_TARGET",
            "E_UNPLACEABLE_PLATE",
            "E_UNRESOLVED_PLACE_REF",
            "E_UNRESOLVED_PORT",
            "E_UNRESOLVED_SLOT",
            "E_UNRESOLVED_THEME_REF",
            "E_UNSUPPORTED_NESTING",
            "W_ABSTRACT_TOKEN_DEFERRED",
            "W_DEFERRED_CONNECT",
            "W_DEFERRED_MEMBER",
            "W_DEFERRED_PLACE",
            "W_DEF_NO_SIZE",
            "W_DUPLICATE_WALKWAY",
            "W_FUTURE_CAIRN_VERSION",
            "W_IGNORED_ARGUMENT",
            "W_INTENDED_TARGET_CAP",
            "W_INTENDED_TARGET_UNSUPPORTED",
            "W_INVALID_CAIRN_VERSION",
            "W_INVALID_WALKWAY_IDENT",
            "W_NO_THEME_BOUND",
            "W_PALETTE_TOO_LARGE",
            "W_PHASE_CONFLICT",
            "W_STATE_LITERAL_UNCHECKED",
            "W_STRUCTURE_TOO_LARGE",
            "W_STRUCT_NO_SIZE",
            "W_THEME_VARIANT_REBOUND",
            "W_TRUTH_TABLE_DUPLICATE_ROW",
            "W_TRUTH_TABLE_PARTIAL",
            "W_UNUSED_DEF",
            "W_WALKWAY_BLOCKED",
        ],
    );
}

#[test]
fn every_code_is_classified_against_the_error_vs_warning_rule() {
    assert_eq!(
        codes_where(|c| c.severity() == Severity::Error),
        [
            "E_AMBIGUOUS_PORT",
            "E_CONNECT_ARITY",
            "E_DUPLICATE_ARG",
            "E_DUPLICATE_CIRCUIT",
            "E_DUPLICATE_HEADER",
            "E_DUPLICATE_ID",
            "E_DUPLICATE_ITEM",
            "E_DUPLICATE_PLACE_ID",
            "E_DUPLICATE_SELECTOR",
            "E_DUPLICATE_SIZE",
            "E_DUPLICATE_SLOT",
            "E_INCOMPATIBLE_MATERIAL",
            "E_INCOMPLETE_PLACE",
            "E_INTENDED_TARGET_CAP",
            "E_INVALID_PLACE_ID",
            "E_INVALID_PLACE_ORIGIN",
            "E_INVALID_REQUIRES",
            "E_INVALID_STRUCT_NAME",
            "E_MISPLACED_BINDING",
            "E_MISPLACED_MEMBER",
            "E_MISSING_MATERIAL",
            "E_MISSING_PATH_MATERIAL",
            "E_OUTPUT_NAME_COLLISION",
            "E_PARSE",
            "E_THEME_VARIANT_MISSING",
            "E_TRUTH_TABLE_CONFLICT",
            "E_TRUTH_TABLE_DUPLICATE_INPUT",
            "E_TRUTH_TABLE_EMPTY",
            "E_TYPE_MISMATCH_LABEL",
            "E_TYPE_MISMATCH_SIZE",
            "E_UNEXPECTED_POSITIONAL",
            "E_UNKNOWN_ABSTRACT_TOKEN",
            "E_UNKNOWN_ARGUMENT",
            "E_UNKNOWN_ID",
            "E_UNKNOWN_KEYWORD",
            "E_UNKNOWN_SLOT_TARGET",
            "E_UNPLACEABLE_PLATE",
            "E_UNRESOLVED_PLACE_REF",
            "E_UNRESOLVED_PORT",
            "E_UNRESOLVED_SLOT",
            "E_UNRESOLVED_THEME_REF",
            "E_UNSUPPORTED_NESTING",
        ],
    );
    assert_eq!(
        codes_where(|c| c.severity() == Severity::Warning),
        [
            "E_THEME_SELECTOR_UNMATCHED",
            "W_ABSTRACT_TOKEN_DEFERRED",
            "W_DEFERRED_CONNECT",
            "W_DEFERRED_MEMBER",
            "W_DEFERRED_PLACE",
            "W_DEF_NO_SIZE",
            "W_DUPLICATE_WALKWAY",
            "W_FUTURE_CAIRN_VERSION",
            "W_IGNORED_ARGUMENT",
            "W_INTENDED_TARGET_CAP",
            "W_INTENDED_TARGET_UNSUPPORTED",
            "W_INVALID_CAIRN_VERSION",
            "W_INVALID_WALKWAY_IDENT",
            "W_NO_THEME_BOUND",
            "W_PALETTE_TOO_LARGE",
            "W_PHASE_CONFLICT",
            "W_STATE_LITERAL_UNCHECKED",
            "W_STRUCTURE_TOO_LARGE",
            "W_STRUCT_NO_SIZE",
            "W_THEME_VARIANT_REBOUND",
            "W_TRUTH_TABLE_DUPLICATE_ROW",
            "W_TRUTH_TABLE_PARTIAL",
            "W_UNUSED_DEF",
            "W_WALKWAY_BLOCKED",
        ],
        "the prefix is not the severity: `E_THEME_SELECTOR_UNMATCHED` is a \
         warning and most `W_` codes are partial-build degradations without \
         that being what the letter means, so neither shape is by itself a \
         misclassification — but both are worth re-reading against \
         spec/lint \"Error vs warning\" whenever one lands",
    );
}

#[test]
fn diagnostic_data_walkway_blocked_serialises_with_kind_tag() {
    let value = serde_json::to_value(DiagnosticData::WalkwayBlocked { skipped: 3 })
        .expect("serialise payload");
    assert_eq!(
        value,
        serde_json::json!({"kind": "walkway_blocked", "skipped": 3}),
    );
}

#[test]
fn unknown_id_payload_omits_the_halves_it_does_not_have() {
    // `spec/lint` "Machine-readable payload": an optional field is absent, never `null`.
    let authored = serde_json::to_value(DiagnosticData::UnknownId {
        id: "minecraft:light".to_owned(),
        registry: "bedrock 1.21.60".to_owned(),
        origin: "authored".to_owned(),
        token: None,
        suggestion: None,
        aliases: Vec::new(),
    })
    .expect("serialise payload");
    assert_eq!(
        authored,
        serde_json::json!({
            "kind": "unknown_id",
            "id": "minecraft:light",
            "registry": "bedrock 1.21.60",
            "origin": "authored",
        }),
    );

    let from_catalog = serde_json::to_value(DiagnosticData::UnknownId {
        id: "minecraft:stone_bricks".to_owned(),
        registry: "bedrock 1.21.0".to_owned(),
        origin: "catalog".to_owned(),
        token: Some("floor.stone.smooth".to_owned()),
        suggestion: Some("minecraft:stonebrick".to_owned()),
        aliases: vec!["minecraft:stonebrick".to_owned()],
    })
    .expect("serialise payload");
    assert_eq!(
        from_catalog,
        serde_json::json!({
            "kind": "unknown_id",
            "id": "minecraft:stone_bricks",
            "registry": "bedrock 1.21.0",
            "origin": "catalog",
            "token": "floor.stone.smooth",
            "suggestion": "minecraft:stonebrick",
            "aliases": ["minecraft:stonebrick"],
        }),
    );
}

#[test]
fn rendered_diagnostic_omits_data_key_when_payload_absent() {
    let lines = LineStarts::new("abc\n");
    let diag = Diagnostic {
        code: DiagnosticCode::DuplicateSize,
        span: Span { start: 0, end: 3 },
        primary: "duplicate size".to_owned(),
        notes: vec![],
        data: None,
    };
    let rendered = diag.render("abc\n", &lines);
    let value = serde_json::to_value(&rendered).expect("serialise rendered");
    let object = value.as_object().expect("rendered as object");
    assert!(
        !object.contains_key("data"),
        "data key should be omitted when payload is None, got {value}",
    );
}

#[test]
fn rendered_diagnostic_propagates_data_payload_when_present() {
    let lines = LineStarts::new("abc\n");
    let diag = Diagnostic {
        code: DiagnosticCode::WalkwayBlocked,
        span: Span { start: 0, end: 3 },
        primary: "walkway skipped 3 cells".to_owned(),
        notes: vec![],
        data: Some(DiagnosticData::WalkwayBlocked { skipped: 3 }),
    };
    let rendered = diag.render("abc\n", &lines);
    assert_eq!(
        rendered.data,
        Some(DiagnosticData::WalkwayBlocked { skipped: 3 }),
    );
}
