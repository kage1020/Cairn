//! `cairn diff-blocks`' library half: the document a lift compared at its structure's stamp
//! gives, and which refusal ends a run that compares nothing.

use cairn_lang_core::{DiagnosticCode, Edition};
use cairn_lang_formats::build::artifact::prepare_artifact;
use cairn_lang_formats::build::finding::Found;
use cairn_lang_formats::build::run_level::{RunLevelAt, RunLevelKind, RunLevelMessage};
use cairn_lang_formats::data_version::resolve_target;
use cairn_lang_formats::diff_blocks::{DiffBlocksRefusal, DiffBlocksRun, diff_blocks};
use cairn_lang_formats::import::{DecodedStructure, ImportFormat, decode_structure};

use crate::common::{examples_dir, lower_source};

/// A struct of stone, which every Java target declares.
const STONE: &str = "theme t:\n  slot wall -> @stone\n\nstruct s size=2x2\n  floor mat_slot=wall\n  walls \
     height=2 mat_slot=wall\n";

/// `source` written at `edition` `mc_version` as `cairn compile` writes it, and decoded as an
/// import reads it.
fn written(source: &str, edition: Edition, mc_version: &str) -> DecodedStructure {
    let ir = lower_source(source, edition, Some(mc_version));
    let target = resolve_target(edition, mc_version).expect("a shipped target");
    let (scope, array) = ir
        .structures
        .first()
        .expect("the source builds a structure");
    let prepared = prepare_artifact(&ir, scope, array, &target).expect("the array prepares");
    let mut bytes = Vec::new();
    prepared
        .structure
        .write(&mut bytes)
        .expect("the file writes");
    let format = match edition {
        Edition::Java => ImportFormat::Nbt,
        Edition::Bedrock => ImportFormat::Mcstructure,
    };
    decode_structure(bytes.as_slice(), format, &prepared.file_name).expect("the file decodes")
}

/// `source` written as a Java 1.21.4 `.nbt` named `s.nbt`, and decoded as an import reads it.
fn java_structure(source: &str) -> DecodedStructure {
    written(source, Edition::Java, "1.21.4")
}

/// What `message` reports and where, without its notes.
fn summary(message: &RunLevelMessage) -> (RunLevelKind, RunLevelAt, &str) {
    (message.kind, message.at, message.primary.as_str())
}

/// The codes of a run's findings, in order.
fn codes(run: &DiffBlocksRun) -> Vec<&'static str> {
    run.findings
        .iter()
        .map(|found| match found {
            Found::Core(d) => d.code.as_str(),
            Found::Circuit(d) => d.code.as_str(),
        })
        .collect()
}

#[test]
fn a_lift_is_compared_with_its_structure_at_the_stamp() {
    let run = diff_blocks(STONE, &java_structure(STONE));
    let document = run.outcome.as_ref().expect("the lift is compared");
    let json = serde_json::to_value(document).expect("the document serialises");
    assert_eq!(
        (
            codes(&run),
            run.conflicts.len(),
            &json["structure"],
            &json["target"],
            &json["saved"],
            &json["converged"],
            &json["block_iou"],
        ),
        (
            Vec::new(),
            0,
            &serde_json::json!("s.nbt"),
            &serde_json::json!({ "edition": "java", "version": "1.21.4" }),
            &serde_json::json!("DataVersion 4189 (1.21.4)"),
            &serde_json::json!(true),
            &serde_json::json!(1.0),
        ),
    );
    assert_eq!(
        serde_json::to_value(run.failure_document(STONE)).expect("serialises"),
        serde_json::json!({ "diagnostics": [] }),
    );
}

#[test]
fn a_source_a_pass_refuses_lists_every_finding_in_the_failure_document() {
    let source = "struct s size=2x2 size=3x3\n  floor\n";
    let run = diff_blocks(source, &java_structure(STONE));
    assert!(
        matches!(run.outcome, Err(DiffBlocksRefusal::Findings)),
        "{:?}",
        run.outcome
    );
    assert_eq!(codes(&run), ["E_DUPLICATE_SIZE", "E_MISSING_MATERIAL"]);
    let document = serde_json::to_value(run.failure_document(source)).expect("serialises");
    let listed: Vec<&str> = document["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(listed, codes(&run));
}

#[test]
fn a_source_that_does_not_parse_is_refused_with_its_parse_failure() {
    let run = diff_blocks(
        "struct s size=2x2\n  floor mat_slot=(\n",
        &java_structure(STONE),
    );
    assert!(
        matches!(run.outcome, Err(DiffBlocksRefusal::Findings)),
        "{:?}",
        run.outcome
    );
    assert_eq!(codes(&run), ["E_PARSE"]);
}

#[test]
fn each_refusal_of_the_whole_run_comes_after_the_findings_in_the_order_compile_gives() {
    // Bedrock declares `minecraft:stonebrick` last at 1.21.0, below the floor and at the stamp.
    let conflict = "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> \
                    @stonebrick\n\nstruct s size=2x2\n  floor mat_slot=wall\n";
    let run = diff_blocks(conflict, &written(STONE, Edition::Bedrock, "1.21.0"));
    assert!(
        matches!(run.outcome, Err(DiffBlocksRefusal::Conflicts)),
        "{:?}",
        run.outcome
    );
    assert_eq!(run.conflicts.len(), 1);

    let partial = "theme t:\n  slot floor -> @oak_planks\n\nstruct good size=2x2\n  floor \
                   mat_slot=floor\n\nstruct bad\n  floor mat_slot=floor\n";
    let run = diff_blocks(partial, &java_structure(STONE));
    let Err(DiffBlocksRefusal::RunLevel(refusal)) = &run.outcome else {
        panic!("a dropped scope refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        summary(refusal),
        (
            RunLevelKind::PartialBuild,
            RunLevelAt::Source,
            "1 of 2 requested scopes did not lower; refusing to compare a partial build",
        ),
    );

    let floor = format!("@requires version>=26.3\n{STONE}");
    let run = diff_blocks(&floor, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Floor(refusal)) = &run.outcome else {
        panic!("a floor past the stamp refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        summary(refusal),
        (
            RunLevelKind::FloorRefusal(DiagnosticCode::VersionCap),
            RunLevelAt::Offset(0),
            "this file requires version>=26.3 (target 1.21.4).",
        ),
    );
    assert_eq!(
        serde_json::to_value(run.failure_document(&floor)).expect("serialises"),
        serde_json::json!({ "diagnostics": [] }),
    );
}

#[test]
fn what_the_stamp_cannot_write_refuses_the_run_and_what_it_degrades_is_warned_of() {
    let sign = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=wall \
                height=3\n  sign at=inside.front offset=2 y=1 back=\"Caf\u{e9}\"\n";
    let run = diff_blocks(sign, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Backend(refusal)) = &run.outcome else {
        panic!(
            "a sign the stamp cannot write refuses the run: {:?}",
            run.outcome
        );
    };
    assert_eq!(
        (summary(refusal), refusal.notes.as_slice()),
        (
            (
                RunLevelKind::BackendRefusal,
                RunLevelAt::Nothing,
                "checking `struct::s`: the sign at (2, 1, 3) has the line `Caf\u{e9}`, which cannot \
                 be written as NBT: nbt: string contains byte 0xc3 at index 4, not encodable here. \
                 Fix: write the sign's text in printable ASCII",
            ),
            &[][..],
        )
    );

    let tower = std::fs::read_to_string(examples_dir().join("themed-tower.crn"))
        .expect("the example reads");
    let run = diff_blocks(&tower, &written(&tower, Edition::Bedrock, "1.21.60"));
    assert!(run.outcome.is_ok(), "{:?}", run.outcome);
    let degraded: Vec<_> = run.degraded.iter().map(summary).collect();
    assert_eq!(
        degraded,
        [(
            RunLevelKind::IntentDegraded,
            RunLevelAt::Nothing,
            "struct::keep: minecraft:dark_oak_stairs[facing=south,half=top,shape=outer_left]: this \
             Bedrock version has no stair `shape` state; its stairs render straight, so corners \
             show visual gaps",
        )]
    );
}
