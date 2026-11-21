//! `info`: the document `cairn info --format json` prints, for a source with no lock beside it.

use cairn_lang_formats::info::requested_editions;
use cairn_lang_wasm::report::{Informed, info};
use serde_json::{Value, json};

use crate::common::{BUILD, codes};

fn info_for(source: &str, editions: &[&str]) -> Informed {
    let editions: Vec<String> = editions.iter().map(|&e| e.to_owned()).collect();
    info(source, &requested_editions(&editions).expect("editions"))
}

fn as_json(informed: &Informed) -> Value {
    serde_json::to_value(informed).expect("serialize")
}

/// The failure document's findings, each as its code and position.
fn refused_codes(informed: &Informed) -> Vec<(&'static str, u32, u32)> {
    match informed {
        Informed::Refused(document) => codes(&document.diagnostics),
        Informed::Report(document) => panic!("the run reported {document:?}"),
    }
}

#[test]
fn a_clean_source_answers_the_report_with_no_lock_verifying_a_target() {
    assert_eq!(
        as_json(&info_for(BUILD, &["java", "bedrock"])),
        json!({
            "registry_compat": { "min": "0.0", "max": "latest" },
            "inferred_range": [
                { "edition": "java", "min": "0.0", "max": "latest", "empty": false },
                { "edition": "bedrock", "min": "0.0", "max": "latest", "empty": false },
            ],
            "edition_portability": [
                {
                    "edition": "java",
                    "portable": 1,
                    "degraded": 0,
                    "unsupported": 0,
                    "unsupported_entries": [],
                    "degraded_entries": [],
                },
                {
                    "edition": "bedrock",
                    "portable": 1,
                    "degraded": 0,
                    "unsupported": 0,
                    "unsupported_entries": [],
                    "degraded_entries": [],
                },
            ],
            "respelled": [
                { "edition": "java", "tokens": [] },
                { "edition": "bedrock", "tokens": [] },
            ],
            "semantic_sensitive": [],
            "buildable_targets": [
                {
                    "edition": "java",
                    "buildable": ["1.20.4", "1.21", "1.21.4", "26.3"],
                    "considered": ["1.20.4", "1.21", "1.21.4", "26.3"],
                },
                {
                    "edition": "bedrock",
                    "buildable": ["1.21.0", "1.21.40", "1.21.60", "1.26.60"],
                    "considered": ["1.21.0", "1.21.40", "1.21.60", "1.26.60"],
                },
            ],
            "intended_targets": [],
            "verified_target": { "path": "", "status": "missing" },
        }),
    );
}

#[test]
fn one_edition_asked_about_gets_only_its_rows() {
    let report = as_json(&info_for(BUILD, &["bedrock"]));
    for row in [
        "inferred_range",
        "edition_portability",
        "respelled",
        "buildable_targets",
    ] {
        let editions: Vec<&Value> = report[row]
            .as_array()
            .unwrap_or_else(|| panic!("`{row}` is a list"))
            .iter()
            .map(|entry| &entry["edition"])
            .collect();
        assert_eq!(editions, [&json!("bedrock")], "`{row}`");
    }
}

#[test]
fn a_token_a_version_writes_under_another_id_answers_its_respelled_row() {
    let source = "theme t:\n  slot floor -> @chain\nstruct s size=2x2\n  floor mat_slot=floor\n";
    assert_eq!(
        as_json(&info_for(source, &["java", "bedrock"]))["respelled"],
        json!([
            {
                "edition": "java",
                "tokens": [{
                    "id": "minecraft:chain",
                    "line": 2,
                    "col": 17,
                    "written": [
                        { "version": "26.3", "id": "minecraft:iron_chain", "through": "aliases" },
                    ],
                }],
            },
            {
                "edition": "bedrock",
                "tokens": [{
                    "id": "minecraft:chain",
                    "line": 2,
                    "col": 17,
                    "written": [
                        { "version": "1.26.60", "id": "minecraft:iron_chain", "through": "aliases" },
                    ],
                }],
            },
        ]),
    );
}

#[test]
fn a_floor_the_registry_contradicts_is_carried_in_the_inferred_range_without_refusing() {
    let source = "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> @stonebrick\n\n\
                  struct s size=2x2\n  floor mat_slot=wall\n";
    let report = as_json(&info_for(source, &["bedrock"]));
    assert_eq!(
        report["inferred_range"],
        json!([{
            "edition": "bedrock",
            "min": "0.0",
            "max": "1.21.0",
            "empty": false,
            "max_ids": [{ "id": "minecraft:stonebrick", "line": 3, "col": 16 }],
            "conflicts": [{
                "floor": { "declared": "bedrock version>=1.21.40", "line": 1, "col": 1 },
                "ids": [{
                    "id": "minecraft:stonebrick",
                    "last_declared": "1.21.0",
                    "line": 3,
                    "col": 16,
                }],
            }],
        }]),
    );
    assert_eq!(report["buildable_targets"][0]["buildable"], json!([]));
}

#[test]
fn a_parse_failure_answers_the_failure_document() {
    let informed = info_for("struct s size=\n", &["java", "bedrock"]);
    assert_eq!(refused_codes(&informed), [("E_PARSE", 1, 15)]);
    assert_eq!(
        as_json(&informed)
            .as_object()
            .map(|document| document.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["diagnostics".to_owned()]),
    );
}

#[test]
fn a_finding_the_gate_refuses_answers_the_failure_document() {
    let source =
        "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=nope\n";
    assert_eq!(
        refused_codes(&info_for(source, &["java", "bedrock"])),
        [("E_UNRESOLVED_SLOT", 4, 3)],
    );
}
