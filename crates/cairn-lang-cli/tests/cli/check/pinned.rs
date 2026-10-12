//! `cairn check --edition E --target V`: the check gate runs the lowering pass `compile` runs,
//! against the same id table, writing nothing.

use std::path::PathBuf;

use crate::common::{assert_stream, cairn, crn_examples, examples_dir};

/// A theme slot bound to a block no version has.
const UNKNOWN_ID: &str =
    "theme t:\n  slot floor -> @totally_not_a_block\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// A slot bound to an id Bedrock spells only from 1.21.40.
const RENAMED_ID: &str =
    "theme t:\n  slot floor -> @stone_bricks\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// A typo'd abstract token, a lowering-stage error needing no id table, in a struct whose walls
/// still paint, so it is not a lost scope.
const ABSTRACT_TOKEN_TYPO: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot floor -> @floor.wood.broadlef\n",
    "  slot wall -> @cobblestone\n\n",
    "struct s size=3x3\n",
    "  floor mat_slot=floor\n",
    "  walls mat_slot=wall height=2\n",
);

/// A gable roof bound outside the stair family: `E_INCOMPATIBLE_MATERIAL`.
const ROOF_OUTSIDE_THE_STAIR_FAMILY: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "  slot roof -> @cobblestone\n\n",
    "struct hut size=5x3\n",
    "  walls class=outer mat_slot=wall height=3\n",
    "  roof kind=gable mat_slot=roof overhang=0\n",
);

/// Two scopes, one of which lowers to nothing — the `E_PARTIAL_BUILD` shape.
const ONE_SCOPE_WITHOUT_A_SIZE: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot floor -> @oak_planks\n\n",
    "struct good size=2x2\n",
    "  floor mat_slot=floor\n\n",
    "struct bad\n",
    "  floor mat_slot=floor\n",
);

/// A source with nothing to report, so its array is `[]`.
const NOTHING_TO_REPORT: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot floor -> @oak_planks\n\n",
    "struct s size=2x2\n",
    "  floor mat_slot=floor\n",
);

/// The same unknown id, through a `def` three `place` rows instantiate.
const UNKNOWN_ID_PLACED_THREE_TIMES: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot floor -> @totally_not_a_block\n\n",
    "def hut size=2x2:\n",
    "  floor mat_slot=floor\n\n",
    "site s:\n",
    "  place id=a use=hut theme=t at=origin\n",
    "  place id=b use=hut theme=t east_of=a gap=4\n",
    "  place id=c use=hut theme=t east_of=b gap=4\n",
);

fn fixture(dir: &std::path::Path, source: &str) -> PathBuf {
    let path = dir.join("s.crn");
    std::fs::write(&path, source).expect("write fixture");
    path
}

#[test]
fn a_pinned_target_reports_an_id_that_target_does_not_declare() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(
        &stderr,
        &[
            (2, 17, "error", "E_UNKNOWN_ID"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
    assert!(
        stderr.contains("java 1.21.4"),
        "the refusal names the pinned target, got: {stderr}",
    );
}

#[test]
fn without_a_target_the_same_source_still_passes() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn("check", &[src.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    // An `--edition` pin alone is not a version either.
    let out = cairn("check", &[src.to_str().unwrap(), "--edition", "java"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn a_renamed_id_is_judged_per_version_not_per_edition() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), RENAMED_ID);

    let refused = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.0",
        ],
    );
    let stderr = String::from_utf8(refused.stderr).expect("utf-8");
    assert_eq!(refused.status.code(), Some(1), "stderr={stderr}");
    assert_stream(
        &stderr,
        &[
            (2, 17, "error", "E_UNKNOWN_ID"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
    // From the pack's alias table, not a distance search.
    assert!(
        stderr.contains("stonebrick"),
        "the older spelling is the repair, got: {stderr}",
    );

    let accepted = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.40",
        ],
    );
    let stderr = String::from_utf8(accepted.stderr).expect("utf-8");
    assert_eq!(accepted.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn the_json_report_carries_the_unknown_id_payload() {
    // The one CLI run whose JSON carries `DiagnosticData::UnknownId`.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--format",
            "json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    let first = parsed
        .as_array()
        .and_then(|a| a.first())
        .unwrap_or_else(|| panic!("a diagnostics array with the finding in it: {stdout}"));
    assert_eq!(first["code"], "E_UNKNOWN_ID", "got: {stdout}");
    assert_eq!(first["data"]["kind"], "unknown_id", "got: {stdout}");
    assert_eq!(
        first["data"]["id"], "minecraft:totally_not_a_block",
        "got: {stdout}",
    );
    assert_eq!(first["data"]["registry"], "java 1.21.4", "got: {stdout}");
    assert_eq!(first["data"]["origin"], "authored", "got: {stdout}");
}

#[test]
fn target_without_edition_is_refused_as_a_usage_error() {
    // `spec/compilation` "Target axes" forbids `--target` alone; the file is never read.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn("check", &[src.to_str().unwrap(), "--target", "1.21.4"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(2), "stderr={stderr}");
    assert!(
        stderr.contains("--edition"),
        "the message names the flag that is missing, got: {stderr}",
    );
}

#[test]
fn a_target_the_edition_does_not_ship_refuses_the_run() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "9.9.9",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("unsupported java target `9.9.9`"),
        "the same refusal `cairn compile` gives, got: {stderr}",
    );
}

#[test]
fn a_target_that_does_not_resolve_still_lowers_the_file() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "9.9.9",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let finding = stderr
        .find("E_UNKNOWN_ABSTRACT_TOKEN")
        .unwrap_or_else(|| panic!("the lowering-stage finding must survive: {stderr}"));
    let refusal = stderr
        .find("unsupported java target")
        .unwrap_or_else(|| panic!("premise: the target is still refused: {stderr}"));
    assert!(finding < refusal, "the file comes first, got: {stderr}");
    // No `E_UNKNOWN_ID`: no version is pinned to ask.
    assert_stream(&stderr, &[(4, 17, "error", "E_UNKNOWN_ABSTRACT_TOKEN")]);
}

#[test]
fn the_json_report_of_an_unshipped_target_is_the_findings_and_the_exit_code() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "9.9.9",
            "--format",
            "json",
        ],
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    let codes: Vec<&str> = parsed
        .as_array()
        .expect("an array of findings")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(
        codes.contains(&"E_UNKNOWN_ABSTRACT_TOKEN"),
        "the array reports what was checked, got: {stdout}",
    );
    assert!(
        stderr.contains("unsupported java target `9.9.9`"),
        "and the refusal is on stderr, got: {stderr}",
    );
}

#[test]
fn a_json_consumer_tells_a_run_level_refusal_by_the_exit_over_an_errorless_array() {
    // Both sources raise warnings at most, so each refusal fails the run without an element.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let partial = fixture(tmp.path(), ONE_SCOPE_WITHOUT_A_SIZE);
    let clean = tmp.path().join("clean.crn");
    std::fs::write(&clean, NOTHING_TO_REPORT).expect("write fixture");

    let rows: [(&PathBuf, &str, &str, &[&str]); 2] = [
        (
            &partial,
            "1.21.4",
            "error[E_PARTIAL_BUILD]",
            &["W_STRUCT_NO_SIZE"],
        ),
        (&clean, "9.9.9", "unsupported java target `9.9.9`", &[]),
    ];
    for (src, target, refusal, expected) in rows {
        let out = cairn(
            "check",
            &[
                src.to_str().unwrap(),
                "--edition",
                "java",
                "--target",
                target,
                "--format",
                "json",
            ],
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert_eq!(out.status.code(), Some(1), "{refusal}: stderr={stderr}");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
        let findings = parsed.as_array().expect("an array of findings");
        let codes: Vec<&str> = findings.iter().filter_map(|d| d["code"].as_str()).collect();
        assert_eq!(codes, expected, "{refusal}: got: {stdout}");
        assert!(
            findings.iter().all(|d| d["severity"] != "error"),
            "{refusal}: the refusal is not an element, got: {stdout}",
        );
        assert!(stderr.contains(refusal), "got: {stderr}");
    }
}

#[test]
fn a_lowering_finding_that_is_not_an_id_verdict_reaches_a_pinned_check() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    let token = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = cairn(
        "check",
        &[
            token.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(4, 17, "error", "E_UNKNOWN_ABSTRACT_TOKEN")]);
    assert!(stderr.contains("floor.wood.broadleaf"), "got: {stderr}");

    let roof = tmp.path().join("hut.crn");
    std::fs::write(&roof, ROOF_OUTSIDE_THE_STAIR_FAMILY).expect("write fixture");
    let out = cairn(
        "check",
        &[
            roof.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(5, 16, "error", "E_INCOMPATIBLE_MATERIAL")]);
}

#[test]
fn a_lost_scope_refuses_the_pinned_check_as_it_refuses_the_compile() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ONE_SCOPE_WITHOUT_A_SIZE);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("error[E_PARTIAL_BUILD]") && stderr.contains("1 of 2 requested scopes"),
        "got: {stderr}",
    );
    assert!(
        stderr.contains("`struct::bad` produced no voxels"),
        "the refusal names what was lost, got: {stderr}",
    );
    assert!(
        !stderr.contains("refusing to certify"),
        "that is the compile's reason, not this one, got: {stderr}",
    );
    let out = cairn("check", &[src.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(!stderr.contains("E_PARTIAL_BUILD"), "got: {stderr}");
}

#[test]
fn latest_pins_a_version_like_any_other_target() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "latest",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(
        &stderr,
        &[
            (2, 17, "error", "E_UNKNOWN_ID"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
    assert!(
        !stderr.contains("java latest"),
        "the refusal names the resolved version, got: {stderr}",
    );
}

#[test]
fn a_bad_target_is_reported_after_the_findings_in_the_file() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    // The theme gives the floor a block, so the struct is not a lost scope.
    let src = fixture(
        tmp.path(),
        "theme t:\n  slot floor -> @stone\n\nstruct s size=2x2 size=3x3\n  floor mat_slot=floor\n",
    );
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "9.9.9",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let finding = stderr
        .find("E_DUPLICATE_SIZE")
        .unwrap_or_else(|| panic!("premise: the file's own finding is reported: {stderr}"));
    let usage = stderr
        .find("unsupported java target")
        .unwrap_or_else(|| panic!("premise: the target is still refused: {stderr}"));
    assert!(finding < usage, "got: {stderr}");
}

#[test]
fn every_shipped_example_passes_the_pinned_check_on_both_editions() {
    for path in crn_examples() {
        for edition in ["java", "bedrock"] {
            let out = cairn(
                "check",
                &[
                    path.to_str().unwrap(),
                    "--edition",
                    edition,
                    "--target",
                    "latest",
                ],
            );
            let stderr = String::from_utf8(out.stderr).expect("utf-8");
            assert_eq!(
                out.status.code(),
                Some(0),
                "{} on {edition}: stderr={stderr}",
                path.display(),
            );
        }
    }
}

#[test]
fn a_clean_example_passes_the_pinned_check_and_writes_nothing() {
    // From a scratch directory, so an artifact or lockfile written beside the source would show.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("cottage.crn");
    std::fs::copy(examples_dir().join("cottage.crn"), &src).expect("copy example");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(stderr.trim().is_empty(), "nothing to report, got: {stderr}");

    let mut left: Vec<String> = std::fs::read_dir(tmp.path())
        .expect("read scratch dir")
        .map(|entry| {
            entry
                .expect("read an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(left, vec!["cottage.crn".to_owned()], "check writes nothing");
}

#[test]
fn a_pinned_target_reports_an_unknown_id_once_per_source_line() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID_PLACED_THREE_TIMES);
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let refusals = stderr.matches("E_UNKNOWN_ID").count();
    assert_eq!(
        refusals, 1,
        "three placements read one slot line and there is one id to fix; \
         got {refusals} copies in: {stderr}",
    );
}
