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
    built_example("cottage.crn", edition, target, name)
}

/// [`built`] for the example `example`.
fn built_example(
    example: &str,
    edition: &str,
    target: &str,
    name: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (tmp, src) = example_in_tempdir(example);
    let artifact = compiled(tmp.path(), &src, edition, target, name);
    (tmp, src, artifact)
}

/// `src` compiled for `edition` at `target` into `<dir>/built`, as its artifact `name`.
fn compiled(dir: &Path, src: &Path, edition: &str, target: &str, name: &str) -> PathBuf {
    let out = dir.join("built");
    let run = Run::from(compile(src, edition, target, &out, None));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    out.join(name)
}

/// A 5x5 hut of a floor and 3-high walls.
const WALLED_HUT: &str = "theme t:\n  slot floor -> @oak_planks\n  slot wall  -> @cobblestone\n\n\
                   struct hut size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n";

/// [`WALLED_HUT`] compiled for Java 1.21.4, as [`built`] gives the cottage.
fn built_hut() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", WALLED_HUT);
    let artifact = compiled(tmp.path(), &src, "java", "1.21.4", "hut.nbt");
    (tmp, src, artifact)
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
fn an_item_frame_on_bedrock_compares_as_the_block_it_is_saved_as() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let source = format!(
        "{WALLED_HUT}  item_frame id=plain at=2,1,2 facing=up\n  \
         item_frame id=lit at=1,1,1 facing=up item=diamond glowing=true\n"
    );
    let src = write_source(tmp.path(), "hut.crn", &source);
    let artifact = compiled(tmp.path(), &src, "bedrock", "1.21.60", "hut.mcstructure");
    let run = diff_blocks(&[&artifact, &src]);
    let expected = format!(
        "{} against {} at bedrock 1.21.60, a shipped target that saves that block version\n{}\
         no cell differs\n",
        artifact.display(),
        src.display(),
        metric_lines("1.000", "1.000", "0%"),
    );
    let note = format!(
        "note: only blocks are compared, not the block entity data `{}` holds\n",
        artifact.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(0), expected.as_str(), note.as_str())
    );
}

#[test]
fn a_lift_that_misses_names_its_region_and_the_patch_that_fixes_it() {
    let (tmp, src, artifact) = built_hut();
    let lift = write_source(
        tmp.path(),
        "lift.crn",
        &fs::read_to_string(&src)
            .expect("the source reads")
            .replace("height=3", "height=2"),
    );
    let run = diff_blocks(&[&artifact, &lift]);
    let stdout = format!(
        "{} against {} at java 1.21.4, the shipped target the file was saved at\n{}\
         1 region differs:\n  \
         missing hut (0,3,0)..(4,3,4): 16 cells of minecraft:cobblestone → likely `walls[0]` \
         `height=2` too small\n\
         suggested patch, reaching block IoU 1.000, state_accuracy 1.000, residual raw 0%:\n  \
         edit walls[0] set height=3\n",
        artifact.display(),
        lift.display(),
        metric_lines("0.780", "1.000", "0%"),
    );
    let stderr = format!(
        "error[E_DECOMPILE_DIFF]: block IoU = 0.780 < threshold 0.985\n{THRESHOLDS}\n  \
         note: append the suggested patch to `{}`, or edit what the regions name, and run \
         `cairn diff-blocks` again\n",
        lift.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), stdout.as_str(), stderr.as_str())
    );

    let patched = format!(
        "{}edit walls[0] set height=3\n",
        fs::read_to_string(&lift).expect("the lift reads")
    );
    fs::write(&lift, patched).expect("the patch appends");
    assert_eq!(diff_blocks(&[&artifact, &lift]).code, Some(0));
}

#[test]
fn the_json_report_is_one_document_of_the_whole_comparison() {
    let (tmp, src, artifact) = built_hut();
    let lift = write_source(
        tmp.path(),
        "lift.crn",
        &fs::read_to_string(&src)
            .expect("the source reads")
            .replace("height=3", "height=2"),
    );
    let run = diff_blocks(&[&artifact, &lift, Path::new("--format"), Path::new("json")]);
    assert_eq!((run.code, run.stderr.as_str()), (Some(1), ""));
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("one JSON document");
    let thresholds =
        serde_json::json!({"block_iou": 0.985, "state_accuracy": 0.995, "residual_ratio": 0.05});
    let counts = serde_json::json!({
        "structure_blocks": 73,
        "built_blocks": 57,
        "both_blocks": 57,
        "matched": 57,
        "states_matched": 57,
        "residual": 0,
    });
    let iou = 57.0 / 73.0;
    assert_eq!(
        report,
        serde_json::json!({
            "structure": "hut.nbt",
            "target": {"edition": "java", "version": "1.21.4"},
            "saved": "DataVersion 4189 (1.21.4)",
            "converged": false,
            "code": "E_DECOMPILE_DIFF",
            "block_iou": iou,
            "state_accuracy": 1.0,
            "residual_ratio": 0.0,
            "thresholds": thresholds,
            "counts": counts,
            "bodies": [{
                "structure": "hut",
                "source": "hut",
                "block_iou": iou,
                "state_accuracy": 1.0,
                "residual_ratio": 0.0,
                "counts": counts,
            }],
            "regions": [{
                "kind": "missing",
                "body": "hut",
                "from": [0, 3, 0],
                "to": [4, 3, 4],
                "cells": 16,
                "mat": "minecraft:cobblestone",
                "cause": {
                    "kind": "short_of",
                    "member": {"keyword": "walls", "address": "walls[0]", "line": 7},
                },
                "patch": "edit walls[0] set height=3",
            }],
            "patch": {
                "edits": ["edit walls[0] set height=3"],
                "block_iou": 1.0,
                "state_accuracy": 1.0,
                "residual_ratio": 0.0,
                "counts": agreeing(73),
            },
        })
    );
}

#[test]
fn a_converged_json_report_carries_no_code_regions_or_patch() {
    let (_tmp, src, artifact) = built_hut();
    let run = diff_blocks(&[&artifact, &src, Path::new("--format"), Path::new("json")]);
    assert_eq!((run.code, run.stderr.as_str()), (Some(0), ""));
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("one JSON document");
    assert_eq!(
        report,
        serde_json::json!({
            "structure": "hut.nbt",
            "target": {"edition": "java", "version": "1.21.4"},
            "saved": "DataVersion 4189 (1.21.4)",
            "converged": true,
            "block_iou": 1.0,
            "state_accuracy": 1.0,
            "residual_ratio": 0.0,
            "thresholds": {"block_iou": 0.985, "state_accuracy": 0.995, "residual_ratio": 0.05},
            "counts": agreeing(73),
            "bodies": [{
                "structure": "hut",
                "source": "hut",
                "block_iou": 1.0,
                "state_accuracy": 1.0,
                "residual_ratio": 0.0,
                "counts": agreeing(73),
            }],
            "regions": [],
        })
    );
}

/// The JSON counts of `blocks` cells that agree in every way, none of them residual.
fn agreeing(blocks: u64) -> serde_json::Value {
    serde_json::json!({
        "structure_blocks": blocks,
        "built_blocks": blocks,
        "both_blocks": blocks,
        "matched": blocks,
        "states_matched": blocks,
        "residual": 0,
    })
}

#[test]
fn a_long_report_lists_ten_regions_the_metrics_of_each_pair_and_what_to_edit() {
    let (tmp, src, artifact) = built_hut();
    let stones = (0..12)
        .map(|i| {
            format!(
                "  raw_block mat=minecraft:stone at={},0,{}\n",
                3 * (i % 4),
                3 * (i / 4)
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let lift = write_source(
        tmp.path(),
        "lift.crn",
        &format!(
            "{}\nstruct shed size=12x12\n{stones}",
            fs::read_to_string(&src).expect("the source reads")
        ),
    );
    let run = diff_blocks(&[&artifact, &lift]);
    let regions: String = [
        (0, 0),
        (3, 0),
        (6, 0),
        (9, 0),
        (0, 3),
        (3, 3),
        (6, 3),
        (9, 3),
        (0, 6),
        (3, 6),
    ]
    .map(|(x, z)| {
        format!(
            "  extra shed ({x},0,{z})..({x},0,{z}): 1 cell of minecraft:stone → the structure \
                 holds no `shed`\n"
        )
    })
    .concat();
    let stdout = format!(
        "{} against {} at java 1.21.4, the shipped target the file was saved at\n{}  \
         hut: block IoU 1.000, state_accuracy 1.000, residual raw 0%\n  \
         (none) / shed: block IoU 0.000, state_accuracy 1.000, residual raw 100%\n\
         12 regions differ:\n{regions}  and 2 more; `--format json` lists every region\n",
        artifact.display(),
        lift.display(),
        metric_lines("0.858", "1.000", "14.2%"),
    );
    let stderr = format!(
        "error[E_DECOMPILE_DIFF]: block IoU = 0.858 < threshold 0.985, residual raw = 14.2% > \
         threshold 5%\n{THRESHOLDS}\n  note: edit what the regions name in `{}`, and run \
         `cairn diff-blocks` again\n",
        lift.display()
    );
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), stdout.as_str(), stderr.as_str())
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
         note: `cairn diff-blocks` builds the source at the stamp of `cottage.nbt`, java 1.21.4, \
         so its `@requires` must admit that target\n",
        later.display(),
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
fn a_floor_no_target_meets_is_refused_ahead_of_the_stamp() {
    let (tmp, _src, artifact) = built("bedrock", "1.21.0", "cottage.mcstructure");
    // Bedrock declares `@stonebrick` last at 1.21.0, and no later version writes it.
    let conflicted = write_source(
        tmp.path(),
        "conflicted.crn",
        "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> @stonebrick\n\nstruct s \
         size=2x2\n  floor mat_slot=wall\n",
    );
    let path = conflicted.display();
    let stderr = format!(
        "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires bedrock version>=1.21.40, \
         but no bedrock target it admits declares `minecraft:stonebrick`, which this build \
         paints.\n  note: {path}:3:16: `minecraft:stonebrick` is last declared at bedrock \
         1.21.0\n  fix: lower the `@requires` floor to 1.21.0 or below, or replace \
         `minecraft:stonebrick` with a block a bedrock target at or above 1.21.40 declares\n",
    );
    let run = diff_blocks(&[&artifact, &conflicted]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), "", stderr.as_str()),
        "the conflict stops the comparison ahead of `E_VERSION_CAP`, as it stops `compile`",
    );
    let json = diff_blocks(&[
        &artifact,
        &conflicted,
        Path::new("--format"),
        Path::new("json"),
    ]);
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
                "error: cannot read `{}` as a Java structure file: nbt I/O: unexpected end of \
                 file\n",
                truncated.display()
            ),
        ),
    ];
    for (args, code, stderr) in &cases {
        let run = diff_blocks(args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(*code), "", stderr.as_str()),
            "{args:?}"
        );
    }
    let json = diff_blocks(&[&truncated, &src, Path::new("--format"), Path::new("json")]);
    assert_eq!(
        (json.code, json.stdout.as_str(), json.stderr.as_str()),
        (
            Some(1),
            "{\n  \"diagnostics\": []\n}\n",
            cases[3].2.as_str()
        ),
        "a structure that cannot be read as its format still writes one document"
    );
}

#[test]
fn a_structure_that_cannot_be_read_is_refused_ahead_of_a_source_that_does_not_parse() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let truncated = tmp.path().join("truncated.nbt");
    fs::write(&truncated, b"junk").expect("write the truncated file");
    let broken = write_source(
        tmp.path(),
        "broken.crn",
        "struct s size=2x2\n  floor mat_slot=(\n",
    );
    let stderr = format!(
        "error: cannot read `{}` as a Java structure file: nbt I/O: unexpected end of file\n",
        truncated.display()
    );
    let run = diff_blocks(&[&truncated, &broken]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), "", stderr.as_str())
    );
    let json = diff_blocks(&[
        &truncated,
        &broken,
        Path::new("--format"),
        Path::new("json"),
    ]);
    assert_eq!(
        (json.code, json.stdout.as_str(), json.stderr.as_str()),
        (Some(1), "{\n  \"diagnostics\": []\n}\n", stderr.as_str())
    );
}

#[test]
fn a_partial_build_is_refused_in_either_format() {
    let (tmp, _src, artifact) = built_hut();
    // `nada`'s one member does not lower under a raised `level`, so `b` builds nothing.
    let partial = write_source(
        tmp.path(),
        "partial.crn",
        "theme t:\n  slot floor -> @stone\n\ndef box size=3x3:\n  floor mat_slot=floor\n\n\
         def nada size=3x3:\n  level y=1\n    floor mat_slot=floor\n\nsite s:\n  \
         place id=a use=box theme=t at=origin\n  place id=b use=nada theme=t east_of=a gap=2\n",
    );
    let run = diff_blocks(&[&artifact, &partial]);
    assert_eq!((run.code, run.stdout.as_str()), (Some(1), ""));
    assert_stream(
        &run.stderr,
        &[
            (9, 5, "warning", "W_DEFERRED_MEMBER"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
    let refusal = format!(
        "error[E_PARTIAL_BUILD]: {}: 1 of 2 requested scopes did not lower; refusing to compare \
         a partial build\n  note: `site::s::b` produced no voxels\n",
        partial.display()
    );
    assert!(run.stderr.ends_with(&refusal), "stderr={}", run.stderr);
    let json = diff_blocks(&[
        &artifact,
        &partial,
        Path::new("--format"),
        Path::new("json"),
    ]);
    assert_eq!(
        (json.code, json.stdout.as_str(), json.stderr.as_str()),
        (
            Some(1),
            "{\n  \"diagnostics\": []\n}\n",
            run.stderr.as_str()
        )
    );
}

#[test]
fn a_structure_the_backend_cannot_write_is_refused_in_either_format() {
    let (tmp, _src, artifact) = built_hut();
    let long = "a".repeat(usize::from(u16::MAX));
    let sign = write_source(
        tmp.path(),
        "sign.crn",
        &format!(
            "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x5\n  \
             walls mat_slot=wall height=3\n  sign at=inside.front offset=2 y=1 back=\"{long}\"\n"
        ),
    );
    let stderr = format!(
        "error: checking `struct::s`: the sign at (2, 1, 3) has the line `{long}`, which cannot \
         be written as NBT: nbt: string length 65537 exceeds wire limit 65535. Fix: shorten the \
         sign's text\n"
    );
    let run = diff_blocks(&[&artifact, &sign]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(1), "", stderr.as_str())
    );
    let json = diff_blocks(&[&artifact, &sign, Path::new("--format"), Path::new("json")]);
    assert_eq!(
        (json.code, json.stdout.as_str(), json.stderr.as_str()),
        (Some(1), "{\n  \"diagnostics\": []\n}\n", stderr.as_str())
    );
}

#[test]
fn an_intent_the_target_degrades_is_warned_of_as_compile_warns_of_it() {
    let (_tmp, src, artifact) =
        built_example("themed-tower.crn", "bedrock", "1.21.60", "keep.mcstructure");
    let run = diff_blocks(&[&artifact, &src]);
    assert_eq!(
        (run.code, run.stderr.as_str()),
        (
            Some(0),
            "warning[W_INTENT_DEGRADED]: struct::keep: \
             minecraft:dark_oak_stairs[facing=south,half=top,shape=outer_left]: this Bedrock \
             version has no stair `shape` state; its stairs render straight, so corners show \
             visual gaps\n"
        )
    );
}

#[test]
fn a_structure_holding_block_entity_data_says_only_blocks_are_compared() {
    let (_tmp, src, artifact) = built_example("crossbar.crn", "java", "26.3", "crossbar.nbt");
    let run = diff_blocks(&[&artifact, &src]);
    let stderr = format!(
        "note: only blocks are compared, not the block entity data `{}` holds\n",
        artifact.display()
    );
    assert_eq!((run.code, run.stderr.as_str()), (Some(0), stderr.as_str()));
}
