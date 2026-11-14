//! `compile`: the findings, the run-level messages, the IR and the structure files of one build.

use std::io::Read;

use cairn_lang_core::Edition;
use cairn_lang_core::Severity;
use cairn_lang_core::check::RenderedNote;
use cairn_lang_formats::build::run_level::RenderedRunLevelMessage;
use cairn_lang_wasm::report::compile;
use flate2::read::GzDecoder;
use serde_json::json;

use crate::common::{BUILD, codes, example};

fn names(compiled: &cairn_lang_wasm::report::Compiled) -> Vec<&str> {
    compiled
        .artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect()
}

fn note(message: &str) -> RenderedNote {
    RenderedNote {
        line: None,
        col: None,
        message: message.to_owned(),
    }
}

#[test]
fn a_clean_build_answers_its_ir_beside_one_structure_file_per_structure() {
    let compiled = compile(BUILD, Edition::Java, "latest");
    assert_eq!(
        serde_json::to_value(&compiled).expect("serialize"),
        json!({
            "ok": true,
            "diagnostics": [],
            "run_level": [],
            "ir": {
                "structures": {
                    "struct::s": {
                        "dims": { "x": 2, "y": 1, "z": 2 },
                        "palette": [{ "id": "minecraft:air" }, { "id": "minecraft:oak_planks" }],
                        "voxels": [1, 1, 1, 1],
                        "source_scope": "struct::s",
                    },
                },
                "members": {
                    "struct::s": [{
                        "role": "floor",
                        "resolved_state": [{
                            "state": { "id": "minecraft:oak_planks" },
                            "runs": [
                                { "from": [0, 0, 0], "len": 2 },
                                { "from": [0, 0, 1], "len": 2 },
                            ],
                        }],
                    }],
                },
            },
        }),
    );
    assert_eq!(names(&compiled), ["s.nbt"]);
    let mut nbt = Vec::new();
    GzDecoder::new(compiled.artifacts[0].bytes.as_slice())
        .read_to_end(&mut nbt)
        .expect("a `.nbt` is gzip");
    // A root compound tag with an empty name, big-endian.
    assert_eq!(nbt[..3], [10, 0, 0]);
}

#[test]
fn bedrock_writes_an_uncompressed_little_endian_mcstructure() {
    let compiled = compile(BUILD, Edition::Bedrock, "latest");
    assert!(compiled.ok, "{compiled:?}");
    assert_eq!(names(&compiled), ["s.mcstructure"]);
    // A root compound tag with an empty name, little-endian, then the `format_version` int.
    assert_eq!(compiled.artifacts[0].bytes[..4], [10, 0, 0, 3]);
}

#[test]
fn a_site_writes_each_placed_structure_and_walkway_in_the_irs_order() {
    let compiled = compile(&example("village"), Edition::Java, "latest");
    assert!(compiled.ok, "{compiled:?}");
    assert_eq!(
        names(&compiled),
        [
            "home1.nbt",
            "home2.nbt",
            "home3.nbt",
            "hamlet_walkway_home1_entry__home2_entry.nbt",
            "hamlet_walkway_home1_entry__home3_entry.nbt",
        ],
    );
    let ir = compiled.ir.expect("an ok build answers its IR");
    assert_eq!(ir.structures.len(), compiled.artifacts.len());
}

#[test]
fn a_parse_failure_is_the_one_finding_and_answers_no_ir() {
    let compiled = compile("struct s size=\n", Edition::Java, "latest");
    assert!(!compiled.ok);
    assert_eq!(codes(&compiled.diagnostics), [("E_PARSE", 1, 15)]);
    assert_eq!(compiled.diagnostics[0].severity, Severity::Error);
    assert_eq!(compiled.run_level, []);
    assert!(compiled.ir.is_none());
    assert_eq!(compiled.artifacts, []);
}

#[test]
fn an_id_the_target_lacks_is_a_finding_that_refuses_the_build_before_its_ir() {
    let source =
        "theme t:\n  slot floor -> @pale_oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";
    let compiled = compile(source, Edition::Java, "1.21");
    assert!(!compiled.ok);
    assert_eq!(codes(&compiled.diagnostics), [("E_UNKNOWN_ID", 2, 17)]);
    assert_eq!(
        compiled.diagnostics[0].primary,
        "`minecraft:pale_oak_planks` is not a block in `java 1.21`",
    );
    assert_eq!(compiled.run_level, []);
    assert!(compiled.ir.is_none());
    assert_eq!(compiled.artifacts, []);
}

#[test]
fn an_unshipped_target_is_refused_beside_the_findings_with_the_ir_kept() {
    let compiled = compile(BUILD, Edition::Java, "1.2");
    assert!(!compiled.ok);
    assert_eq!(codes(&compiled.diagnostics), []);
    assert_eq!(
        compiled.run_level,
        [RenderedRunLevelMessage {
            code: None,
            severity: Severity::Error,
            line: None,
            col: None,
            primary: "unsupported java target `1.2`. did you mean `1.21`? supported targets: \
                      1.20.4, 1.21, 1.21.4, 26.3, latest"
                .to_owned(),
            notes: Vec::new(),
        }],
    );
    assert!(compiled.ir.is_some());
    assert_eq!(compiled.artifacts, []);
}

#[test]
fn a_floor_the_target_is_below_is_refused_at_the_line_that_declares_it() {
    let source = format!("@cairn 2026.7\n\n@requires version>=1.21\n{BUILD}");
    let compiled = compile(&source, Edition::Java, "1.20.4");
    assert!(!compiled.ok);
    assert_eq!(codes(&compiled.diagnostics), []);
    assert_eq!(
        compiled.run_level,
        [RenderedRunLevelMessage {
            code: Some("E_VERSION_CAP"),
            severity: Severity::Error,
            line: Some(3),
            col: Some(1),
            primary: "this file requires version>=1.21 (target 1.20.4).".to_owned(),
            notes: vec![
                note("valid java targets: 1.21, 1.21.4, 26.3"),
                note("fix: --target 1.21, or lower the `@requires` floor"),
            ],
        }],
    );
    assert_eq!(compiled.artifacts, []);
}

#[test]
fn a_scope_that_lowers_to_nothing_refuses_the_partial_build() {
    let source = "theme t:\n  slot floor -> @oak_planks\nstruct good size=2x2\n  floor mat_slot=floor\n\
                  struct bad\n  floor mat_slot=floor\n";
    let compiled = compile(source, Edition::Java, "latest");
    assert!(!compiled.ok);
    assert_eq!(codes(&compiled.diagnostics), [("W_STRUCT_NO_SIZE", 5, 1)]);
    assert_eq!(
        compiled.run_level,
        [RenderedRunLevelMessage {
            code: Some("E_PARTIAL_BUILD"),
            severity: Severity::Error,
            line: None,
            col: None,
            primary: "1 of 2 requested scopes did not lower; refusing to certify a partial build"
                .to_owned(),
            notes: vec![note("note: `struct::bad` produced no voxels")],
        }],
    );
    assert_eq!(compiled.artifacts, []);
}

#[test]
fn an_intent_the_target_can_only_approximate_warns_and_still_writes() {
    let compiled = compile(&example("themed-tower"), Edition::Bedrock, "1.21.60");
    assert!(compiled.ok, "{compiled:?}");
    assert_eq!(
        compiled.run_level,
        [RenderedRunLevelMessage {
            code: Some("W_INTENT_DEGRADED"),
            severity: Severity::Warning,
            line: None,
            col: None,
            primary: "struct::keep: minecraft:dark_oak_stairs[facing=south,half=top,\
                      shape=outer_left]: this Bedrock version has no stair `shape` state; its \
                      stairs render straight, so corners show visual gaps"
                .to_owned(),
            notes: Vec::new(),
        }],
    );
    assert_eq!(names(&compiled), ["keep.mcstructure"]);
}
