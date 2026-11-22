//! `cairn diff-blocks`' library half: the document a lift compared at its structure's stamp
//! gives, and which refusal ends a run that compares nothing.

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{DiagnosticCode, Edition};
use cairn_lang_formats::build::artifact::prepare_artifact;
use cairn_lang_formats::build::finding::render_findings;
use cairn_lang_formats::build::run_level::{RunLevelKind, RunLevelMessage};
use cairn_lang_formats::data_version::resolve_target;
use cairn_lang_formats::diff_blocks::{DiffBlocksRefusal, DiffBlocksRun, diff_blocks};
use cairn_lang_formats::import::{
    DecodeFileError, DecodedStructure, ImportFormat, decode_structure, decode_structure_file,
};

use crate::common::{examples_dir, lower_source};

/// A struct of stone, which every Java target declares.
const STONE: &str = "theme t:\n  slot wall -> @stone\n\nstruct s size=2x2\n  floor mat_slot=wall\n  walls \
     height=2 mat_slot=wall\n";

/// `source` written at `edition` `mc_version` as `cairn compile` writes it, and decoded as an
/// import reads it.
fn written(source: &str, edition: Edition, mc_version: &str) -> DecodedStructure {
    let (bytes, file_name) = file_bytes(source, edition, mc_version);
    let format = match edition {
        Edition::Java => ImportFormat::Nbt,
        Edition::Bedrock => ImportFormat::Mcstructure,
    };
    decode_structure(bytes.as_slice(), format, &file_name).expect("the file decodes")
}

/// The bytes and name of the file `cairn compile` writes for `source` at `edition` `mc_version`.
fn file_bytes(source: &str, edition: Edition, mc_version: &str) -> (Vec<u8>, String) {
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
    (bytes, prepared.file_name)
}

/// `source` written as a Java 1.21.4 `.nbt` named `s.nbt`, and decoded as an import reads it.
fn java_structure(source: &str) -> DecodedStructure {
    written(source, Edition::Java, "1.21.4")
}

/// A `def` no site places, which `W_UNUSED_DEF` warns of without refusing the build.
const SPARE: &str = "\ndef spare size=2x2:\n  floor mat_slot=wall\n";

/// The lines `cairn diff-blocks` writes on stderr for `message`, about `source` named `s.crn`.
fn reported(message: &RunLevelMessage, source: &str) -> Vec<String> {
    message.lines("s.crn", source, &LineStarts::new(source))
}

/// Each finding's code, line and column, as the JSON document lists them.
fn listed(document: &serde_json::Value) -> Vec<(String, u64, u64)> {
    document["diagnostics"]
        .as_array()
        .expect("a diagnostics array")
        .iter()
        .map(|d| {
            (
                d["code"].as_str().expect("a code").to_owned(),
                d["line"].as_u64().expect("a line"),
                d["col"].as_u64().expect("a column"),
            )
        })
        .collect()
}

/// A run's findings as the JSON document would list them, whether or not it lists them.
fn found(run: &DiffBlocksRun, source: &str) -> Vec<(String, u64, u64)> {
    let lines = LineStarts::new(source);
    let rendered = render_findings(source, &lines, &run.findings);
    listed(&serde_json::json!({ "diagnostics": rendered }))
}

/// `(code, line, col)` for each of `findings`.
fn at(findings: &[(&str, u64, u64)]) -> Vec<(String, u64, u64)> {
    findings
        .iter()
        .map(|&(code, line, col)| (code.to_owned(), line, col))
        .collect()
}

/// The JSON document of `run` over `source`.
fn json(run: &DiffBlocksRun, source: &str) -> serde_json::Value {
    serde_json::to_value(run.json_document(source)).expect("the document serialises")
}

#[test]
fn a_lift_is_compared_with_its_structure_at_the_stamp() {
    let run = diff_blocks(STONE, &java_structure(STONE));
    let compared = run.outcome.as_ref().expect("the lift is compared");
    let document = json(&run, STONE);
    assert_eq!(
        (
            found(&run, STONE),
            compared.degraded.as_slice(),
            &document["structure"],
            &document["target"],
            &document["saved"],
            &document["converged"],
            &document["block_iou"],
        ),
        (
            Vec::new(),
            &[][..],
            &serde_json::json!("s.nbt"),
            &serde_json::json!({ "edition": "java", "version": "1.21.4" }),
            &serde_json::json!("DataVersion 4189 (1.21.4)"),
            &serde_json::json!(true),
            &serde_json::json!(1.0),
        ),
    );
    assert_eq!(
        document,
        serde_json::to_value(&compared.document).expect("serialises")
    );
}

/// A stone box of 40 blocks with an item frame and a glow item frame lying on its floor.
const FRAMED: &str = "theme t:\n  slot wall -> @stone\n\nstruct s size=4x4\n  floor mat_slot=wall\n  \
     walls height=2 mat_slot=wall\n  item_frame id=plain at=1,1,1 facing=up\n  item_frame id=lit \
     at=2,1,2 facing=up glowing=true\n";

#[test]
fn a_bedrock_item_frame_is_compared_as_the_block_it_is_saved_as() {
    let run = diff_blocks(FRAMED, &written(FRAMED, Edition::Bedrock, "1.21.60"));
    let document = json(&run, FRAMED);
    assert_eq!(
        (
            found(&run, FRAMED),
            &document["converged"],
            &document["counts"],
            &document["regions"],
        ),
        (
            Vec::new(),
            &serde_json::json!(true),
            &serde_json::json!({
                "structure_blocks": 42,
                "built_blocks": 42,
                "both_blocks": 42,
                "matched": 42,
                "states_matched": 42,
                "residual": 0,
            }),
            &serde_json::json!([]),
        ),
    );
}

#[test]
fn a_source_a_pass_refuses_lists_every_finding_in_the_document() {
    let source = "struct s size=2x2 size=3x3\n  floor\n";
    let run = diff_blocks(source, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Findings { conflicts }) = &run.outcome else {
        panic!("the findings refuse the run: {:?}", run.outcome);
    };
    assert_eq!(
        (conflicts.as_slice(), listed(&json(&run, source))),
        (
            &[][..],
            at(&[("E_DUPLICATE_SIZE", 1, 19), ("E_MISSING_MATERIAL", 2, 3)]),
        ),
    );
}

#[test]
fn a_source_that_does_not_parse_is_refused_with_its_parse_failure() {
    let source = "struct s size=2x2\n  floor mat_slot=(\n";
    let run = diff_blocks(source, &java_structure(STONE));
    assert!(
        matches!(&run.outcome, Err(DiffBlocksRefusal::Findings { conflicts }) if conflicts.is_empty()),
        "{:?}",
        run.outcome
    );
    assert_eq!(listed(&json(&run, source)), at(&[("E_PARSE", 2, 18)]));
}

/// Bedrock declares `minecraft:stonebrick` last at 1.21.0: below this floor, and at the stamp.
const CONFLICT: &str = "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> \
                        @stonebrick\n\nstruct s size=2x2\n  floor mat_slot=wall\n";

/// What `E_REQUIRES_CONFLICT` says of [`CONFLICT`]'s floor, as `cairn diff-blocks` writes it.
fn conflict_lines() -> Vec<String> {
    vec![
        "error[E_REQUIRES_CONFLICT]: s.crn:1:1: this file requires bedrock version>=1.21.40, but \
         no bedrock target it admits declares `minecraft:stonebrick`, which this build paints."
            .to_owned(),
        "  note: s.crn:3:16: `minecraft:stonebrick` is last declared at bedrock 1.21.0".to_owned(),
        "  fix: lower the `@requires` floor to 1.21.0 or below, or replace `minecraft:stonebrick` \
         with a block a bedrock target at or above 1.21.40 declares"
            .to_owned(),
    ]
}

#[test]
fn a_conflicting_floor_refuses_the_run_ahead_of_the_stamp_it_also_rules_out() {
    let source = format!("{CONFLICT}{SPARE}");
    let run = diff_blocks(&source, &written(STONE, Edition::Bedrock, "1.21.0"));
    let Err(DiffBlocksRefusal::Conflicts(conflicts)) = &run.outcome else {
        panic!("the conflict refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        (
            conflicts
                .iter()
                .map(|c| reported(c, &source))
                .collect::<Vec<_>>(),
            found(&run, &source),
            json(&run, &source),
        ),
        (
            vec![conflict_lines()],
            at(&[("W_UNUSED_DEF", 8, 1)]),
            serde_json::json!({ "diagnostics": [] }),
        ),
    );
}

#[test]
fn an_error_beside_a_conflicting_floor_refuses_the_run_by_findings_with_the_conflict() {
    let source = CONFLICT.replace("struct s size=2x2", "struct s size=2x2 size=3x3");
    let run = diff_blocks(&source, &written(STONE, Edition::Bedrock, "1.21.0"));
    let Err(DiffBlocksRefusal::Findings { conflicts }) = &run.outcome else {
        panic!("the error refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        (
            conflicts
                .iter()
                .map(|c| reported(c, &source))
                .collect::<Vec<_>>(),
            listed(&json(&run, &source)),
        ),
        (vec![conflict_lines()], at(&[("E_DUPLICATE_SIZE", 5, 19)]),),
    );
}

/// A site whose `b` builds nothing: `nada`'s one member does not lower under a raised `level`.
const PARTIAL: &str = "theme t:\n  slot floor -> @oak_planks\n\ndef box size=3x3:\n  floor \
                       mat_slot=floor\n\ndef nada size=3x3:\n  level y=1\n    floor \
                       mat_slot=floor\n\nsite s:\n  place id=a use=box theme=t at=origin\n  \
                       place id=b use=nada theme=t east_of=a gap=2\n";

#[test]
fn a_dropped_scope_refuses_the_run_ahead_of_a_floor_past_the_stamp() {
    for (source, member) in [
        (PARTIAL.to_owned(), 9),
        (format!("@requires version>=26.3\n{PARTIAL}"), 10),
    ] {
        let run = diff_blocks(&source, &java_structure(STONE));
        let Err(DiffBlocksRefusal::RunLevel(refusal)) = &run.outcome else {
            panic!("the dropped scope refuses the run: {:?}", run.outcome);
        };
        assert_eq!(
            (
                reported(refusal, &source),
                found(&run, &source),
                json(&run, &source),
            ),
            (
                vec![
                    "error[E_PARTIAL_BUILD]: s.crn: 1 of 2 requested scopes did not lower; \
                     refusing to compare a partial build"
                        .to_owned(),
                    "  note: `site::s::b` produced no voxels".to_owned(),
                ],
                at(&[("W_DEFERRED_MEMBER", member, 5)]),
                serde_json::json!({ "diagnostics": [] }),
            ),
            "{source}"
        );
    }
}

#[test]
fn a_floor_past_the_stamp_refuses_the_run_with_a_note_naming_the_stamp() {
    let source = format!("@requires version>=26.3\n{STONE}");
    let run = diff_blocks(&source, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Floor(refusal)) = &run.outcome else {
        panic!("a floor past the stamp refuses the run: {:?}", run.outcome);
    };
    assert_eq!(
        (
            refusal.kind,
            reported(refusal, &source),
            json(&run, &source)
        ),
        (
            RunLevelKind::FloorRefusal(DiagnosticCode::VersionCap),
            vec![
                "error[E_VERSION_CAP]: s.crn:1:1: this file requires version>=26.3 (target \
                 1.21.4)."
                    .to_owned(),
                "  valid java targets: 26.3".to_owned(),
                "  fix: --target 26.3, or lower the `@requires` floor".to_owned(),
                "  note: `cairn diff-blocks` builds the source at the stamp of `s.nbt`, java \
                 1.21.4, so its `@requires` must admit that target"
                    .to_owned(),
            ],
            serde_json::json!({ "diagnostics": [] }),
        ),
    );
}

#[test]
fn what_the_stamp_cannot_write_refuses_the_run_and_what_it_degrades_is_warned_of() {
    let sign = format!(
        "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=wall \
         height=3\n  sign at=inside.front offset=2 y=1 back=\"Caf\u{e9}\"\n{SPARE}"
    );
    let run = diff_blocks(&sign, &java_structure(STONE));
    let Err(DiffBlocksRefusal::Backend(refusal)) = &run.outcome else {
        panic!(
            "a sign the stamp cannot write refuses the run: {:?}",
            run.outcome
        );
    };
    assert_eq!(
        (
            reported(refusal, &sign),
            found(&run, &sign),
            json(&run, &sign)
        ),
        (
            vec![
                "error: checking `struct::s`: the sign at (2, 1, 3) has the line `Caf\u{e9}`, \
                 which cannot be written as NBT: nbt: string contains byte 0xc3 at index 4, not \
                 encodable here. Fix: write the sign's text in printable ASCII"
                    .to_owned()
            ],
            at(&[("W_UNUSED_DEF", 8, 1)]),
            serde_json::json!({ "diagnostics": [] }),
        ),
    );

    let tower = std::fs::read_to_string(examples_dir().join("themed-tower.crn"))
        .expect("the example reads");
    let run = diff_blocks(&tower, &written(&tower, Edition::Bedrock, "1.21.60"));
    let compared = run.outcome.as_ref().expect("the tower is compared");
    let degraded: Vec<_> = compared
        .degraded
        .iter()
        .map(|message| reported(message, &tower))
        .collect();
    assert_eq!(
        degraded,
        [["warning[W_INTENT_DEGRADED]: struct::keep: \
             minecraft:dark_oak_stairs[facing=south,half=top,shape=outer_left]: this Bedrock \
             version has no stair `shape` state; its stairs render straight, so corners show \
             visual gaps"]]
    );
}

#[test]
fn a_structure_file_on_disk_is_decoded_as_its_extension_names_or_refused_naming_it() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let (bytes, _) = file_bytes(STONE, Edition::Java, "1.21.4");
    let saved = tmp.path().join("s.NBT");
    std::fs::write(&saved, &bytes).expect("write the structure");
    assert_eq!(
        decode_structure_file(&saved).expect("the file decodes"),
        decode_structure(bytes.as_slice(), ImportFormat::Nbt, "s.NBT").expect("the bytes decode"),
    );

    let source = tmp.path().join("s.crn");
    let legacy = tmp.path().join("old.schematic");
    let missing = tmp.path().join("absent.nbt");
    let not_found = std::fs::File::open(&missing).expect_err("the file is absent");
    let truncated = tmp.path().join("truncated.nbt");
    std::fs::write(&truncated, b"junk").expect("write the truncated file");
    let refusals: Vec<(bool, String)> = [&source, &legacy, &missing, &truncated]
        .into_iter()
        .map(|path| {
            let err = decode_structure_file(path).expect_err("the file is refused");
            let unknown = matches!(err, DecodeFileError::UnknownExtension(_));
            (unknown, err.to_string())
        })
        .collect();
    assert_eq!(
        refusals,
        [
            (
                true,
                format!(
                    "`{}` is not a structure file this build reads; name a `.nbt`, `.litematic`, \
                     `.schem` or `.mcstructure` file",
                    source.display()
                ),
            ),
            (
                true,
                format!(
                    "`{}` is not a structure file this build reads; a `.schematic` stores the \
                     numeric block ids of the releases before 1.13, which this version does not \
                     read; save it as a `.schem` from WorldEdit 7 or later and name that; name a \
                     `.nbt`, `.litematic`, `.schem` or `.mcstructure` file",
                    legacy.display()
                ),
            ),
            (
                false,
                format!("cannot read `{}`: {not_found}", missing.display())
            ),
            (
                false,
                format!(
                    "cannot read `{}` as a Java structure file: nbt I/O: unexpected end of file",
                    truncated.display()
                ),
            ),
        ],
    );
}
