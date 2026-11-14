//! `info`: the version axes `cairn info --format json` prints, and what refuses them.

use cairn_lang_formats::build::gate::requested_editions;
use cairn_lang_wasm::report::{Informed, info};
use serde_json::json;

use crate::common::{BUILD, codes};

fn info_for(source: &str, editions: &[&str]) -> Informed {
    let editions: Vec<String> = editions.iter().map(|&e| e.to_owned()).collect();
    info(source, &requested_editions(&editions).expect("editions"))
}

#[test]
fn a_clean_source_answers_the_axes_for_each_edition_asked_about() {
    let informed = info_for(BUILD, &["java", "bedrock"]);
    assert_eq!(
        serde_json::to_value(&informed).expect("serialize"),
        json!({
            "ok": true,
            "diagnostics": [],
            "run_level": [],
            "axes": {
                "registry_compat": { "min": "0.0", "max": "latest" },
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
            },
        }),
    );
}

#[test]
fn one_edition_asked_about_gets_only_its_rows() {
    let informed = info_for(BUILD, &["bedrock"]);
    let axes = informed.axes.expect("a clean source answers its axes");
    let rows = serde_json::to_value(&axes).expect("serialize");
    assert_eq!(rows["edition_portability"][0]["edition"], "bedrock");
    assert_eq!(
        rows["edition_portability"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(rows["buildable_targets"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_parse_failure_answers_no_axes() {
    let informed = info_for("struct s size=\n", &["java", "bedrock"]);
    assert!(!informed.ok);
    assert_eq!(codes(&informed.diagnostics), [("E_PARSE", 1, 15)]);
    assert_eq!(informed.run_level, []);
    assert!(informed.axes.is_none());
}

#[test]
fn a_finding_the_gate_refuses_answers_no_axes() {
    let source =
        "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=nope\n";
    let informed = info_for(source, &["java", "bedrock"]);
    assert!(!informed.ok);
    assert_eq!(codes(&informed.diagnostics), [("E_UNRESOLVED_SLOT", 4, 3)]);
    assert!(informed.axes.is_none());
}
