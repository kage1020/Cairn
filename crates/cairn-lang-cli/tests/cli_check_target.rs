//! `cairn check --edition E --target V`: the check gate with a version
//! pinned.
//!
//! Without a target the gate cannot ask whether a block id exists, because
//! the question has no answer at the edition level — `stone_bricks` is a
//! block on Bedrock 1.21.40 and `stonebrick` is the same block on Bedrock
//! 1.21.0. So `cairn check` skipped it, and a CI job gating on `check`
//! went green on a source `cairn compile` refuses with `E_UNKNOWN_ID`.
//! Pinning the pair the id belongs to closes that: the same lowering pass
//! `compile` runs, against the same table, writing nothing.

use std::path::PathBuf;
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The repro this flag was added for: a theme slot bound to a block no
/// version has.
const UNKNOWN_ID: &str =
    "theme t:\n  slot floor -> @totally_not_a_block\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// A slot bound to the id Bedrock only learned to spell in 1.21.40. The
/// interesting half of the check: not a typo, and correct on one target of
/// the very edition the other one refuses it on.
const RENAMED_ID: &str =
    "theme t:\n  slot floor -> @stone_bricks\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// A typo'd abstract token: a lowering-stage error that needs no id table,
/// so it is what says the pass ran even where no version was pinned.
const ABSTRACT_TOKEN_TYPO: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot floor -> @floor.wood.broadlef\n\n",
    "struct s size=3x3\n",
    "  floor mat_slot=floor\n",
);

/// A gable roof bound outside the stair family — `E_INCOMPATIBLE_MATERIAL`,
/// the other lowering-stage error a pinned check must not swallow.
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

fn fixture(dir: &std::path::Path, source: &str) -> PathBuf {
    let path = dir.join("s.crn");
    std::fs::write(&path, source).expect("write fixture");
    path
}

fn run_check(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("check")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn a_pinned_target_reports_an_id_that_target_does_not_declare() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_UNKNOWN_ID"),
        "the pinned check must reach the lowering-stage finding, got: {stderr}",
    );
    // The registry the answer is about is named, so the reader knows which
    // of the two pins decided it.
    assert!(
        stderr.contains("java 1.21.4"),
        "the refusal names the pinned target, got: {stderr}",
    );
}

#[test]
fn without_a_target_the_same_source_still_passes() {
    // The premise of the flag being opt-in: no source that passes today
    // starts failing, because with no version pinned there is nothing to
    // check the id against and guessing one would refuse ids that are fine
    // on the target the author actually compiles for.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[src.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    // And an `--edition` pin alone is still not a version: it picks theme
    // variants and the table `@intended_targets` is weighed in, neither of
    // which is an id table.
    let out = run_check(&[src.to_str().unwrap(), "--edition", "java"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn a_renamed_id_is_judged_per_version_not_per_edition() {
    // One id, one edition, two answers. This is why the flag pins a
    // version rather than reusing `--edition`, and why checking against
    // every version the edition ships (versioning-editions §10.4) would
    // have said nothing here.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), RENAMED_ID);

    let refused = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "bedrock",
        "--target",
        "1.21.0",
    ]);
    let stderr = String::from_utf8(refused.stderr).expect("utf-8");
    assert_eq!(refused.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_UNKNOWN_ID"),
        "1.21.0 does not declare it, got: {stderr}",
    );
    // A rename is answered from the pack's alias table rather than by a
    // distance search, so the older spelling is offered by name.
    assert!(
        stderr.contains("stonebrick"),
        "the older spelling is the repair, got: {stderr}",
    );

    let accepted = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "bedrock",
        "--target",
        "1.21.40",
    ]);
    let stderr = String::from_utf8(accepted.stderr).expect("utf-8");
    assert_eq!(accepted.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn the_json_report_carries_the_unknown_id_payload() {
    // `DiagnosticData::UnknownId` (spec/lint §11.2) was documented and
    // unreachable from any CLI JSON output: `compile` prints text and
    // `check --format json` never produced the code. This is the run that
    // makes the documented shape observable.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
        "--format",
        "json",
    ]);
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
    // Spec §4.2: `--target` alone is forbidden, because "1.21" names
    // different releases on Java and Bedrock. Exit 2 is the usage-error
    // code, not the "your file has a problem" one — the file was never
    // read.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[src.to_str().unwrap(), "--target", "1.21.4"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(2), "stderr={stderr}");
    assert!(
        stderr.contains("--edition"),
        "the message names the flag that is missing, got: {stderr}",
    );
}

#[test]
fn a_target_the_edition_does_not_ship_refuses_the_run() {
    // A check that could not check the ids it was asked to must not exit
    // 0: the caller asked a question about a version, and "that version
    // does not exist" is not an answer that clears the file.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("unsupported java target `9.9.9`"),
        "the same refusal `cairn compile` gives, got: {stderr}",
    );
}

#[test]
fn a_target_that_does_not_resolve_still_lowers_the_file() {
    // The refusal is about the command line; the findings are about the
    // file, and they are true whatever `--target` says. `run_compile`
    // lowers against the unpinned view when resolution fails and reports
    // everything that needs no id table, and this run must too — otherwise
    // the author fixes the flag, re-runs, and only then meets a pile of
    // findings that were true the first time.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let finding = stderr
        .find("E_UNKNOWN_ABSTRACT_TOKEN")
        .unwrap_or_else(|| panic!("the lowering-stage finding must survive: {stderr}"));
    let refusal = stderr
        .find("unsupported java target")
        .unwrap_or_else(|| panic!("premise: the target is still refused: {stderr}"));
    assert!(finding < refusal, "the file comes first, got: {stderr}");
    // Not `E_UNKNOWN_ID`: that one is the answer a version gives, and no
    // version was pinned. Reporting it here would be the guess §10.4 rules
    // out, dressed as a check.
    assert!(
        !stderr.contains("E_UNKNOWN_ID"),
        "no id table, so no id verdict, got: {stderr}",
    );
}

#[test]
fn the_json_report_of_an_unshipped_target_is_the_findings_and_the_exit_code() {
    // The decision this pins: a `--target` the edition does not ship is a
    // fact about the command line, not a finding at a span in the file, so
    // it is stderr and an exit code in both formats rather than an element
    // of the array — the shape `compile` gives it. What the array does
    // carry is every finding the unpinned lowering reached, so the product
    // is a truthful report of what was checked rather than an empty one.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
        "--format",
        "json",
    ]);
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
fn a_lowering_finding_that_is_not_an_id_verdict_reaches_a_pinned_check() {
    // `check_lowering`'s doc argues that filtering the lowering stream down
    // to `E_UNKNOWN_ID` — the code the flag is named for — would be the
    // same silence the flag exists to end. Without these two, that
    // narrowing passes every other test in this file.
    let tmp = tempfile::TempDir::new().expect("tempdir");

    let token = fixture(tmp.path(), ABSTRACT_TOKEN_TYPO);
    let out = run_check(&[
        token.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_UNKNOWN_ABSTRACT_TOKEN") && stderr.contains("floor.wood.broadleaf"),
        "got: {stderr}",
    );

    let roof = tmp.path().join("hut.crn");
    std::fs::write(&roof, ROOF_OUTSIDE_THE_STAIR_FAMILY).expect("write fixture");
    let out = run_check(&[
        roof.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("E_INCOMPATIBLE_MATERIAL"), "got: {stderr}");
}

#[test]
fn a_lost_scope_refuses_the_pinned_check_as_it_refuses_the_compile() {
    // The gap this flag closes, in its other half: a scope that lowers to
    // nothing is `E_PARTIAL_BUILD` at exit 1 from `cairn compile`, so a
    // pinned check that passed it would be the same green-CI-on-a-refused
    // source the flag was written against.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), ONE_SCOPE_WITHOUT_A_SIZE);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
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
    // A check certifies nothing, so it must not claim to be refusing to.
    assert!(
        !stderr.contains("refusing to certify"),
        "that is the compile's reason, not this one, got: {stderr}",
    );
    // And the unpinned run is unchanged: no lowering, so nothing is lost
    // and the warning alone does not fail the gate.
    let out = run_check(&[src.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(!stderr.contains("E_PARTIAL_BUILD"), "got: {stderr}");
}

#[test]
fn latest_pins_a_version_like_any_other_target() {
    // The invocation a CI job actually writes — "does this still build
    // against current" — and the one spelling of `--target` that is not a
    // version string. It resolves through the edition's data table, so the
    // id check runs and names the version it resolved to.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "latest",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("E_UNKNOWN_ID"), "got: {stderr}");
    // The alias is resolved before the report, so the reader is told which
    // version answered rather than being handed the word back.
    assert!(
        !stderr.contains("java latest"),
        "the refusal names the resolved version, got: {stderr}",
    );
}

#[test]
fn a_bad_target_is_reported_after_the_findings_in_the_file() {
    // The ordering `run_compile` keeps, for the same reason: the lines the
    // author edits come first, and a command-line mistake printed above
    // them buries the syntax error that is also true.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(
        tmp.path(),
        "struct s size=2x2 size=3x3\n  floor mat_slot=floor\n",
    );
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
    ]);
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
    // One example at one pin says nothing about the per-edition variant
    // path `check_lowering` resolves for: `edition-fallback.crn` and
    // `themed-tower.crn` are the sources whose slots differ per edition,
    // and it is the Bedrock half of them that nothing else here runs.
    //
    // `latest` on both, because the point is the pass rather than a
    // particular release, and a pack that adds a version should not need
    // this list edited.
    let mut seen = 0;
    let entries = std::fs::read_dir(examples_dir()).expect("read examples dir");
    for entry in entries {
        let path = entry.expect("read an entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        for edition in ["java", "bedrock"] {
            let out = run_check(&[
                path.to_str().unwrap(),
                "--edition",
                edition,
                "--target",
                "latest",
            ]);
            let stderr = String::from_utf8(out.stderr).expect("utf-8");
            assert_eq!(
                out.status.code(),
                Some(0),
                "{} on {edition}: stderr={stderr}",
                path.display(),
            );
            seen += 1;
        }
    }
    // A directory that stopped matching would pass every assertion above
    // by making none of them.
    assert!(seen > 0, "no examples were checked");
}

#[test]
fn a_clean_example_passes_the_pinned_check_and_writes_nothing() {
    // `check --target` runs the lowering `compile` runs; what it must not
    // acquire is `compile`'s output. Run from a scratch directory so an
    // artifact or lockfile written next to the source would show up here.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("cottage.crn");
    std::fs::copy(examples_dir().join("cottage.crn"), &src).expect("copy example");
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
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
