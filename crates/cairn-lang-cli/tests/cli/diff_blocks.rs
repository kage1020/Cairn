//! `cairn diff-blocks`: a structure file compared with what a source builds at the file's stamp,
//! the report in each format, and what it refuses before comparing.

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{
    Run, assert_stream, cairn_argv, compile, example_in_tempdir, json_stream, reported,
    write_source,
};

/// `cairn diff-blocks` with `args`.
fn diff_blocks(args: &[&Path]) -> Run {
    let args: Vec<&str> = args
        .iter()
        .map(|arg| arg.to_str().expect("a UTF-8 path"))
        .collect();
    Run::from(cairn_argv(&[["diff-blocks"].as_slice(), &args].concat()))
}

/// The cottage compiled for `edition` at `target` into `<tmp>/built`, its source, and the
/// artifact `name`.
fn built(edition: &str, target: &str, name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (tmp, src) = example_in_tempdir("cottage.crn");
    let out = tmp.path().join("built");
    let run = Run::from(compile(&src, edition, target, &out, None));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    (tmp, src, out.join(name))
}

/// The three metric lines of a report.
fn metric_lines(iou: &str, states: &str, residual: &str) -> String {
    format!(
        "block IoU       {iou:<6} converges at >= 0.985\n\
         state_accuracy  {states:<6} converges at >= 0.995\n\
         residual raw    {residual:<6} converges at <= 5%\n"
    )
}

/// The note every report short of converging gives the thresholds in.
const THRESHOLDS: &str = "  note: a lift converges at block IoU >= 0.985, state_accuracy >= 0.995 and \
                          residual raw <= 5%";

#[test]
fn a_source_that_builds_the_structure_converges_on_each_edition() {
    for (edition, target, file, reason) in [
        (
            "java",
            "1.21.4",
            "cottage.nbt",
            "the shipped target the file was saved at",
        ),
        (
            "bedrock",
            "1.21.60",
            "cottage.mcstructure",
            "a shipped target that saves that block version",
        ),
    ] {
        let (_tmp, src, artifact) = built(edition, target, file);
        let run = diff_blocks(&[&artifact, &src]);
        let expected = format!(
            "{} against {} at {edition} {target}, {reason}\n{}no cell differs\n",
            artifact.display(),
            src.display(),
            metric_lines("1.000", "1.000", "0%"),
        );
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(0), expected.as_str(), ""),
            "{edition}"
        );
    }
}

#[test]
fn a_lift_that_misses_names_its_regions_and_the_patch_that_fixes_them() {
    let (tmp, src, artifact) = built("java", "1.21.4", "cottage.nbt");
    let lift = write_source(
        tmp.path(),
        "lift.crn",
        &fs::read_to_string(&src)
            .expect("the source reads")
            .replace("height=4", "height=3")
            .replace("offset=2", "offset=1"),
    );
    let run = diff_blocks(&[&artifact, &lift]);
    let stdout = format!(
        "{} against {} at java 1.21.4, the shipped target the file was saved at\n{}\
         6 regions differ:\n  \
         missing cottage (0,5,0)..(10,9,8): 99 cells of minecraft:spruce_stairs → likely \
         `roof[0]` (line 18) stops short of it\n  \
         extra cottage (0,4,0)..(10,8,8): 99 cells of minecraft:spruce_stairs → built by \
         `roof[0]` (line 18)\n  \
         missing cottage (1,4,1)..(9,4,7): 28 cells of minecraft:cobblestone → likely \
         `walls[0]` `height=3` too small\n  \
         replaced cottage (4,2,7)..(6,3,7): 4 cells of minecraft:glass_pane built as \
         minecraft:cobblestone → built by `walls[0]` (line 15)\n  \
         replaced cottage (2,2,7)..(2,3,7): 2 cells of minecraft:cobblestone built as \
         minecraft:glass_pane → likely `window@front[0]` `offset=1` too small\n  \
         replaced cottage (8,2,7)..(8,3,7): 2 cells of minecraft:cobblestone built as \
         minecraft:glass_pane → built by `window@front[0]` (line 17)\n\
         suggested patch, reaching block IoU 1.000, state_accuracy 1.000, residual raw 0%:\n  \
         edit walls[0] set height=4\n  \
         edit window@front[0] set offset=2\n",
        artifact.display(),
        lift.display(),
        metric_lines("0.372", "1.000", "0%"),
    );
    let stderr = format!(
        "error[E_DECOMPILE_DIFF]: block IoU = 0.372 < threshold 0.985\n{THRESHOLDS}\n  \
         note: append the suggested patch to `{}`, or edit what the regions name, and run \
         `cairn diff-blocks` again\n",
        lift.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), stdout.as_str(), stderr.as_str())
    );

    let patched = format!(
        "{}edit walls[0] set height=4\nedit window@front[0] set offset=2\n",
        fs::read_to_string(&lift).expect("the lift reads")
    );
    fs::write(&lift, patched).expect("the patch appends");
    assert_eq!(diff_blocks(&[&artifact, &lift]).code, Some(0));
}

#[test]
fn the_json_report_is_one_document_naming_the_target_and_every_region() {
    let (tmp, src, artifact) = built("java", "1.21.4", "cottage.nbt");
    let lift = write_source(
        tmp.path(),
        "lift.crn",
        &fs::read_to_string(&src)
            .expect("the source reads")
            .replace("height=4", "height=3"),
    );
    let run = diff_blocks(&[&artifact, &lift, Path::new("--format"), Path::new("json")]);
    assert_eq!((run.code, run.stderr.as_str()), (Some(1), ""));
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("one JSON document");
    let keys: Vec<&str> = report
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "block_iou",
            "bodies",
            "code",
            "converged",
            "counts",
            "patch",
            "regions",
            "residual_ratio",
            "saved",
            "state_accuracy",
            "structure",
            "target",
            "thresholds",
        ]
    );
    assert_eq!(
        (
            &report["structure"],
            &report["target"],
            &report["saved"],
            &report["converged"],
            &report["code"],
        ),
        (
            &serde_json::json!("cottage.nbt"),
            &serde_json::json!({"edition": "java", "version": "1.21.4"}),
            &serde_json::json!("DataVersion 4189 (1.21.4)"),
            &serde_json::json!(false),
            &serde_json::json!("E_DECOMPILE_DIFF"),
        )
    );
    assert_eq!(
        report["thresholds"],
        serde_json::json!({"block_iou": 0.985, "state_accuracy": 0.995, "residual_ratio": 0.05})
    );
    assert_eq!(
        report["regions"][2],
        serde_json::json!({
            "kind": "missing",
            "body": "cottage",
            "from": [1, 4, 1],
            "to": [9, 4, 7],
            "cells": 28,
            "mat": "minecraft:cobblestone",
            "cause": {
                "kind": "short_of",
                "member": {"keyword": "walls", "address": "walls[0]", "line": 15},
            },
            "patch": "edit walls[0] set height=4",
        })
    );
    assert_eq!(
        report["patch"]["edits"],
        serde_json::json!(["edit walls[0] set height=4"])
    );
}

#[test]
fn a_raw_import_agrees_at_every_cell_and_falls_short_on_residual() {
    let (tmp, _src, artifact) = built("java", "1.21.4", "cottage.nbt");
    let raw = tmp.path().join("cottage.raw.crn");
    let import = Run::from(cairn_argv(&[
        "import",
        artifact.to_str().expect("UTF-8"),
        "--out",
        raw.to_str().expect("UTF-8"),
    ]));
    assert_eq!(import.code, Some(0), "stderr={}", import.stderr);
    let run = diff_blocks(&[&artifact, &raw]);
    let stdout = format!(
        "{} against {} at java 1.21.4, the shipped target the file was saved at\n{}no cell \
         differs\n",
        artifact.display(),
        raw.display(),
        metric_lines("1.000", "1.000", "100%"),
    );
    let stderr = format!(
        "error[E_DECOMPILE_DIFF]: residual raw = 100% > threshold 5%\n{THRESHOLDS}\n  note: every \
         cell matches; name what the `raw_block`, `raw_fill` and `raw_repeat` lines of `{}` build \
         as the members they are, and run `cairn diff-blocks` again\n",
        raw.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), stdout.as_str(), stderr.as_str())
    );
}

#[test]
fn a_source_that_does_not_build_at_the_stamp_is_refused_before_comparing() {
    let (tmp, src, artifact) = built("java", "1.21.4", "cottage.nbt");
    let source = fs::read_to_string(&src).expect("the source reads");
    let broken = write_source(
        tmp.path(),
        "broken.crn",
        &source.replace("roof   kind=gable", "rooof  kind=gable"),
    );
    let run = diff_blocks(&[&artifact, &broken]);
    let found = [
        (18, 3, "error", "E_UNKNOWN_KEYWORD"),
        (18, 3, "warning", "W_DEFERRED_MEMBER"),
    ];
    assert_eq!((run.code, run.stdout.as_str()), (Some(1), ""));
    assert_stream(&run.stderr, &found);
    let json = diff_blocks(&[&artifact, &broken, Path::new("--format"), Path::new("json")]);
    let document: serde_json::Value = serde_json::from_str(&json.stdout).expect("one document");
    let diagnostics = serde_json::to_string(&document["diagnostics"]).expect("it serialises");
    assert_eq!(
        (json.code, json.stderr.as_str(), json_stream(&diagnostics)),
        (Some(1), "", reported(&found))
    );

    let later = write_source(
        tmp.path(),
        "later.crn",
        &source.replace("@requires version>=1.20", "@requires version>=26.3"),
    );
    let run = diff_blocks(&[&artifact, &later]);
    let stderr = format!(
        "error[E_VERSION_CAP]: {}:2:1: this file requires version>=26.3 (target 1.21.4).\n  \
         valid java targets: 26.3\n  \
         fix: --target 26.3, or lower the `@requires` floor\n  \
         note: `cairn diff-blocks` builds the source at the stamp of `{}`, java 1.21.4, so its \
         `@requires` must admit that target\n",
        later.display(),
        artifact.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), "", stderr.as_str())
    );
    let json = diff_blocks(&[&artifact, &later, Path::new("--format"), Path::new("json")]);
    assert_eq!(
        (json.code, json.stdout.as_str(), json.stderr.as_str()),
        (Some(1), "{\n  \"diagnostics\": []\n}\n", stderr.as_str())
    );
}

#[test]
fn a_file_diff_blocks_cannot_read_is_refused() {
    let (tmp, src, artifact) = built("java", "1.21.4", "cottage.nbt");
    let missing = tmp.path().join("absent.nbt");
    let not_found = fs::File::open(&missing).expect_err("the file is absent");
    let truncated = tmp.path().join("truncated.nbt");
    fs::write(&truncated, b"junk").expect("write the truncated file");
    let absent_source = tmp.path().join("absent.crn");
    let cases: [(Vec<&Path>, i32, String); 4] = [
        (
            vec![&src, &src],
            2,
            format!(
                "error: `{}` is not a structure file `cairn diff-blocks` reads\n  note: name a \
                 `.nbt`, `.litematic`, `.schem` or `.mcstructure` file\n",
                src.display()
            ),
        ),
        (
            vec![&missing, &src],
            2,
            format!("error: cannot read `{}`: {not_found}\n", missing.display()),
        ),
        (
            vec![&artifact, &absent_source],
            2,
            format!(
                "error: cannot read `{}`: {not_found}\n",
                absent_source.display()
            ),
        ),
        (
            vec![&truncated, &src],
            1,
            format!(
                "error: cannot compare `{}`: nbt I/O: unexpected end of file\n",
                truncated.display()
            ),
        ),
    ];
    for (args, code, stderr) in cases {
        let run = diff_blocks(&args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(code), "", stderr.as_str()),
            "{args:?}"
        );
    }
}
