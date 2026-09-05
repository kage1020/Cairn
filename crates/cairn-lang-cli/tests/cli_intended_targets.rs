//! `@intended_targets` against the floors the same file declares, from the
//! commands that report it.
//!
//! The core pass (`cairn-lang-core/tests/intended_targets.rs`) decides what
//! each header earns. What this file pins is the half only the CLI can
//! answer: which commands ask, which edition's table they ask with, and
//! that a file whose two version headers contradict each other stops
//! exiting 0.

use std::path::PathBuf;
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// A source in a directory of its own, removed when the test ends.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(label: &str, headers: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("cairn-intended-{}-{label}", std::process::id()));
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("cannot clear {}: {err}", dir.display()),
        }
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("s.crn"), format!("{headers}{BUILD}")).expect("write source");
        Self { dir }
    }

    fn source(&self) -> PathBuf {
        self.dir.join("s.crn")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One structure with something to paint, so nothing here is reported
/// against an empty build.
const BUILD: &str = "\
theme t:
  slot wall -> @oak_planks

struct hut size=5x5
  walls mat_slot=wall height=3
";

fn run(command: &str, fixture: &Fixture, args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(cargo_bin())
        .arg(command)
        .arg(fixture.source())
        .args(args)
        .output()
        .expect("failed to invoke cairn binary");
    (
        out.status.code(),
        String::from_utf8(out.stdout).expect("utf-8"),
        String::from_utf8(out.stderr).expect("utf-8"),
    )
}

/// The issue in one test: the file declares a floor and an intent that
/// cannot both hold, and `cairn check` used to exit 0 on it while
/// `cairn compile --target 1.20.4` refused.
#[test]
fn a_floor_above_every_intended_target_stops_check_exiting_zero() {
    let fixture = Fixture::new(
        "cap",
        "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n",
    );
    let (code, _, stderr) = run("check", &fixture, &[]);
    assert_eq!(code, Some(1), "got: {stderr}");
    assert!(
        stderr.contains("E_INTENDED_TARGET_CAP"),
        "expected the cap, got: {stderr}",
    );
    assert!(
        stderr.contains("1.20.4") && stderr.contains("version>=1.21"),
        "both halves of the contradiction have to be named: {stderr}",
    );
}

/// No pin, and the answer still arrives. The contradiction is inside the
/// file — a floor refusing every version the same file says it is for is a
/// mistake in one of two lines however it is later built — so the plain
/// `cairn check` an author runs is where it has to show up.
#[test]
fn the_contradiction_needs_no_edition_pin() {
    let fixture = Fixture::new(
        "nopin",
        "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n",
    );
    let (pinned, _, pinned_err) = run("check", &fixture, &["--edition", "java"]);
    let (bare, _, bare_err) = run("check", &fixture, &[]);
    assert_eq!(pinned, Some(1), "got: {pinned_err}");
    assert_eq!(bare, Some(1), "got: {bare_err}");
}

/// Part of the list below the floor leaves the rest buildable, so the
/// exit code does not move — the header is a hint, and a hint stated too
/// widely is not a file that cannot be built.
#[test]
fn part_of_the_list_below_the_floor_is_a_warning_and_exits_zero() {
    let fixture = Fixture::new(
        "partial",
        "@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n",
    );
    let (code, _, stderr) = run("check", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert!(
        stderr.contains("W_INTENDED_TARGET_CAP"),
        "expected the warning, got: {stderr}",
    );
}

/// A version no `--target` names is a question about one edition's pack,
/// so it is asked only by a command that named an edition. Unpinned, a
/// version Java cannot build is routinely the Bedrock target the author
/// means, and answering anyway would report every cross-edition list as a
/// mistake.
#[test]
fn a_version_no_target_names_is_reported_only_under_a_pin() {
    let fixture = Fixture::new("unsupported", "@intended_targets [\"1.19\"]\n");
    let (bare_code, _, bare_err) = run("check", &fixture, &[]);
    assert_eq!(bare_code, Some(0), "got: {bare_err}");
    assert!(
        !bare_err.contains("W_INTENDED_TARGET_UNSUPPORTED"),
        "unpinned, nobody has said which edition is being asked: {bare_err}",
    );
    let (code, _, stderr) = run("check", &fixture, &["--edition", "java"]);
    assert_eq!(
        code,
        Some(0),
        "a hint about a version we cannot build is not an error: {stderr}"
    );
    assert!(
        stderr.contains("W_INTENDED_TARGET_UNSUPPORTED"),
        "expected the unsupported warning, got: {stderr}",
    );
    assert!(
        stderr.contains("java builds against 1.20.4, 1.21, 1.21.4"),
        "the closed set of candidates is part of the message: {stderr}",
    );
}

/// The two editions number their releases differently, and the finding is
/// weighed in the table of the edition that was asked: `1.21.4` is Java's
/// newest release and names no Bedrock release at all.
#[test]
fn the_pinned_edition_decides_which_table_weighs_the_list() {
    let fixture = Fixture::new("cross", "@intended_targets [\"1.21.4\"]\n");
    let (java, _, java_err) = run("check", &fixture, &["--edition", "java"]);
    assert_eq!(java, Some(0));
    assert!(
        !java_err.contains("W_INTENDED_TARGET_UNSUPPORTED"),
        "1.21.4 is an ordinary Java target: {java_err}",
    );
    let (bedrock, _, bedrock_err) = run("check", &fixture, &["--edition", "bedrock"]);
    assert_eq!(bedrock, Some(0));
    assert!(
        bedrock_err.contains("names no bedrock release"),
        "and names no Bedrock one: {bedrock_err}",
    );
}

/// `compile` runs the same gate `check` does, so a source `check` refuses
/// does not compile — including for a `--target` the floor is fine with.
/// The contradiction is between two lines of the file and no target
/// resolves it.
#[test]
fn compile_refuses_the_source_check_refuses() {
    let fixture = Fixture::new(
        "compile",
        "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n",
    );
    let out = Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", "java", "--target", "1.21"])
        .arg("--out")
        .arg(fixture.dir.join("out"))
        .arg("--lock")
        .arg(fixture.dir.join("s.crn.lock"))
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "got: {stderr}");
    assert!(stderr.contains("E_INTENDED_TARGET_CAP"), "got: {stderr}");
    assert!(
        !fixture.dir.join("s.crn.lock").exists(),
        "a refused source must not leave a lock reading `verified: true`",
    );
}

/// `cairn info` reports what the file says it is for beside what it can be
/// built for. The row used to be absent entirely, so the one declaration
/// that names versions was the one the report never mentioned.
#[test]
fn info_prints_the_declared_targets_beside_the_buildable_ones() {
    let fixture = Fixture::new(
        "inforow",
        "@requires version>=1.20\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n",
    );
    let (code, stdout, stderr) = run("info", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert!(
        stdout.contains("intended targets:        1.20.4, 1.21.4"),
        "expected the declared row, got: {stdout}",
    );
    assert!(
        stdout.contains("buildable targets:"),
        "beside the computed one, got: {stdout}",
    );
}

/// A file declaring none still gets the row, saying so. An absent row and
/// a row reading `(none declared)` are different reports, and only the
/// second one answers the question.
#[test]
fn info_says_so_when_no_targets_are_declared() {
    let fixture = Fixture::new("inforow-empty", "");
    let (code, stdout, stderr) = run("info", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert!(
        stdout.contains("intended targets:        (none declared)"),
        "got: {stdout}",
    );
}

/// The JSON document carries the same fact, under a key of its own, so a
/// consumer reads the declaration without parsing the text rows.
#[test]
fn the_json_report_carries_the_declared_targets() {
    let fixture = Fixture::new("infojson", "@intended_targets [\"1.21.4\"]\n");
    let (code, stdout, stderr) = run("info", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(0), "got: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(
        parsed["intended_targets"]
            .as_array()
            .expect("intended_targets is an array"),
        &vec![serde_json::Value::String("1.21.4".to_owned())],
    );
}

/// The finding is machine-readable the way every other one is: the code,
/// the versions it is about, and the floor that refuses them, without a
/// consumer taking the sentence apart.
#[test]
fn the_finding_carries_a_structured_payload() {
    let fixture = Fixture::new(
        "json",
        "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n",
    );
    let (code, stdout, _) = run("check", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let first = &parsed[0];
    assert_eq!(first["code"], "E_INTENDED_TARGET_CAP");
    assert_eq!(first["severity"], "error");
    assert_eq!(first["data"]["kind"], "intended_targets");
    assert_eq!(first["data"]["edition"], "java");
    assert_eq!(first["data"]["floor"], "version>=1.21");
    assert_eq!(
        first["data"]["targets"].as_array().expect("targets"),
        &vec![serde_json::Value::String("1.20.4".to_owned())],
    );
}

/// One line to fix, one finding. Without a pin both editions weigh the
/// header, and a file both of them judge the same way would otherwise be
/// reported twice for one mistake.
///
/// A floor above every release either edition ships is what makes them
/// agree: `1.21` is a target both can build (Bedrock spells it `1.21.0`,
/// and trailing zeros are not a different version), and nothing satisfies
/// the floor anywhere.
#[test]
fn a_header_both_editions_judge_alike_is_reported_once() {
    let fixture = Fixture::new(
        "dedup",
        "@requires version>=99.0\n@intended_targets [\"1.21\"]\n",
    );
    let (code, stdout, _) = run("check", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let findings = parsed.as_array().expect("an array of findings");
    assert_eq!(
        findings.len(),
        1,
        "1.20 is below the floor in both editions' tables: {stdout}",
    );
}
