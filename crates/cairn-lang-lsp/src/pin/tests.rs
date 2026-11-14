//! The `cairn.edition` and `cairn.target` settings read as a pin, and the settings refused.

use serde_json::json;

use super::*;

fn pinned(settings: &serde_json::Value) -> Pin {
    Pin::from_settings(settings).unwrap_or_else(|refused| panic!("{settings} refused: {refused:?}"))
}

fn refused(settings: &serde_json::Value) -> PinRefused {
    Pin::from_settings(settings).expect_err("the settings are refused")
}

#[test]
fn settings_that_name_no_edition_pin_nothing() {
    for settings in [
        json!({}),
        json!(null),
        json!({ "cairn": { "serverPath": "/opt/cairn-lsp" } }),
        json!({ "cairn": { "edition": null, "target": null } }),
        json!({ "cairn": { "edition": "", "target": "  " } }),
    ] {
        assert_eq!(pinned(&settings), Pin::Nothing, "{settings}");
    }
}

#[test]
fn an_edition_alone_pins_the_edition() {
    assert_eq!(
        pinned(&json!({ "cairn": { "edition": "bedrock" } })),
        Pin::Edition(Edition::Bedrock),
    );
    assert_eq!(
        pinned(&json!({ "cairn": { "edition": "java", "target": "" } })),
        Pin::Edition(Edition::Java),
    );
}

#[test]
fn an_edition_and_a_target_it_ships_pin_the_target_at_the_version_it_resolves_to() {
    assert_eq!(
        pinned(&json!({ "cairn": { "edition": "java", "target": "1.21.4" } })),
        Pin::Target {
            edition: Edition::Java,
            version: "1.21.4".to_owned(),
        },
    );
    let latest = resolve_bedrock_target("latest").expect("latest resolves");
    assert_eq!(
        pinned(&json!({ "cairn": { "edition": "bedrock", "target": "latest" } })),
        Pin::Target {
            edition: Edition::Bedrock,
            version: latest.mc_version,
        },
    );
}

#[test]
fn a_target_with_no_edition_is_refused_and_pins_nothing() {
    assert_eq!(
        refused(&json!({ "cairn": { "target": "1.21.4" } })),
        PinRefused {
            reason: "`cairn.target` is `1.21.4`, but `cairn.edition` names no edition to read it \
                     in: set `cairn.edition` to java or bedrock"
                .to_owned(),
            pinned: Pin::Nothing,
        },
    );
}

#[test]
fn a_target_the_edition_does_not_ship_is_refused_and_pins_the_edition() {
    let unsupported = resolve_java_target("1.19").expect_err("Java ships no 1.19 target");
    assert_eq!(
        refused(&json!({ "cairn": { "edition": "java", "target": "1.19" } })),
        PinRefused {
            reason: format!("`cairn.target`: {unsupported}"),
            pinned: Pin::Edition(Edition::Java),
        },
    );
}

#[test]
fn a_name_that_is_no_edition_is_refused_with_the_editions_there_are() {
    let unknown = "jvaa".parse::<Edition>().expect_err("not an edition");
    assert_eq!(
        refused(&json!({ "cairn": { "edition": "jvaa", "target": "1.21.4" } })),
        PinRefused {
            reason: format!("`cairn.edition`: {unknown}"),
            pinned: Pin::Nothing,
        },
    );
}

#[test]
fn a_setting_that_is_not_a_string_is_refused() {
    assert_eq!(
        refused(&json!({ "cairn": { "edition": "java", "target": 1.21 } })),
        PinRefused {
            reason: "`cairn.target` is 1.21, which is not a string; the setting was ignored"
                .to_owned(),
            pinned: Pin::Edition(Edition::Java),
        },
    );
    assert_eq!(
        refused(&json!({ "cairn": { "edition": ["java"] } })).pinned,
        Pin::Nothing,
    );
}
