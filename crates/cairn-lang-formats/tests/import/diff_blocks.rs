//! `cairn diff-blocks`' library half: the document a lift compared at its structure's stamp
//! gives, and which refusal ends a run that compares nothing.

use cairn_lang_core::Edition;
use cairn_lang_formats::diff_blocks::{DiffBlocksRefusal, DiffBlocksRun, diff_blocks};
use cairn_lang_formats::import::{
    DecodedStructure, ImportFormat, ImportStamp, StampFit, decode_structure,
};
use cairn_lang_formats::info::Found;
use cairn_lang_formats::java_structure::prepare_structure;
use cairn_lang_formats::registry::builtin_pack;

use crate::common::lower_source;

/// A struct of stone, which every Java target declares.
const STONE: &str = "theme t:\n  slot wall -> @stone\n\nstruct s size=2x2\n  floor mat_slot=wall\n  walls \
     height=2 mat_slot=wall\n";

/// `source` written as a Java 1.21.4 `.nbt` named `s.nbt`, and decoded as an import reads it.
fn java_structure(source: &str) -> DecodedStructure {
    let ir = lower_source(source, Edition::Java, Some("1.21.4"));
    let target = builtin_pack(Edition::Java)
        .resolve_java_target("1.21.4")
        .expect("a shipped target");
    let mut bytes = Vec::new();
    prepare_structure(&ir.structures[0], &target)
        .expect("the array prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    decode_structure(bytes.as_slice(), ImportFormat::Nbt, "s.nbt").expect("the file decodes")
}

/// A structure of no block stamped `edition` `mc_version`, for a run refused before comparing.
fn stamped(edition: Edition, mc_version: &str) -> DecodedStructure {
    DecodedStructure {
        format: match edition {
            Edition::Java => ImportFormat::Nbt,
            Edition::Bedrock => ImportFormat::Mcstructure,
        },
        file_name: "s".to_owned(),
        stamp: ImportStamp {
            edition,
            mc_version: mc_version.to_owned(),
            saved: None,
            fit: StampFit::Exact,
        },
        units: Vec::new(),
        losses: Vec::new(),
    }
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
    let run = diff_blocks(conflict, &stamped(Edition::Bedrock, "1.21.0"));
    assert!(
        matches!(run.outcome, Err(DiffBlocksRefusal::Conflicts)),
        "{:?}",
        run.outcome
    );
    assert_eq!(run.conflicts.len(), 1);

    let partial = "theme t:\n  slot floor -> @oak_planks\n\nstruct good size=2x2\n  floor \
                   mat_slot=floor\n\nstruct bad\n  floor mat_slot=floor\n";
    let run = diff_blocks(partial, &stamped(Edition::Java, "1.21.4"));
    let Err(DiffBlocksRefusal::PartialBuild { dropped, built }) = &run.outcome else {
        panic!("a dropped scope refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        (dropped.as_slice(), *built),
        (["struct::bad".to_owned()].as_slice(), 1)
    );

    let floor = format!("@requires version>=26.3\n{STONE}");
    let run = diff_blocks(&floor, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Floor(declared)) = &run.outcome else {
        panic!("a floor past the stamp refuses the run: {:?}", run.outcome);
    };
    let floors: Vec<&str> = declared.floors.iter().map(|f| f.version.as_str()).collect();
    assert_eq!(floors, ["26.3"]);
    assert_eq!(
        serde_json::to_value(run.failure_document(&floor)).expect("serialises"),
        serde_json::json!({ "diagnostics": [] }),
    );
}
