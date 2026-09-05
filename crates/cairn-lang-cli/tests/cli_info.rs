//! End-to-end tests for `cairn info <file>`.

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

fn run_info(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("info")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn info_1_clean_example_exits_zero_with_three_section_headers() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    for header in [
        "registry compatibility:",
        "edition portability:",
        "semantic-sensitive:",
    ] {
        assert!(
            stdout.contains(header),
            "expected `{header}` line in output, got: {stdout}",
        );
    }
}

#[test]
fn info_2_json_format_is_valid_version_axes() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    for field in [
        "registry_compat",
        "edition_portability",
        "semantic_sensitive",
    ] {
        assert!(
            parsed.get(field).is_some(),
            "JSON missing `{field}`: {parsed}",
        );
    }
    let compat = &parsed["registry_compat"];
    assert_eq!(compat["max"], "latest");
    // Cottage example pins `@requires version>=1.20`.
    assert_eq!(compat["min"], "1.20");

    let portability = parsed["edition_portability"]
        .as_array()
        .expect("portability is an array");
    // Default `--editions java,bedrock` → two entries.
    assert_eq!(portability.len(), 2);
    for entry in portability {
        for key in ["edition", "portable", "degraded", "unsupported"] {
            assert!(entry.get(key).is_some(), "missing `{key}` in {entry}");
        }
        assert_eq!(entry["degraded"], 0);
        assert_eq!(entry["unsupported"], 0);
    }

    let sensitive = parsed["semantic_sensitive"]
        .as_array()
        .expect("semantic_sensitive is an array");
    // The semantic-sensitivity catalog has not landed yet, so this list
    // stays empty.
    assert!(sensitive.is_empty());
}

#[test]
fn info_3_missing_file_exits_with_code_two() {
    let out = run_info(&["does-not-exist.crn"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_4_editions_flag_controls_portability_entries() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[
        path.to_str().unwrap(),
        "--editions",
        "java",
        "--format",
        "json",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let portability = parsed["edition_portability"]
        .as_array()
        .expect("portability is an array");
    assert_eq!(portability.len(), 1);
    assert_eq!(portability[0]["edition"], "java");
}

#[test]
fn info_5_all_examples_exit_zero() {
    for name in [
        "cottage.crn",
        "themed-tower.crn",
        "village.crn",
        "redstone-door.crn",
    ] {
        let path = examples_dir().join(name);
        let out = run_info(&[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{name} should exit 0, stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn info_6_registry_compatibility_renders_as_single_range_line() {
    // The registry-compatible range is currently edition-agnostic
    // (single `min/max` pair in `RegistryRange`). The text output must
    // not duplicate it per edition — that misled reviewers into reading
    // a per-edition divergence that the data does not carry.
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let line = stdout
        .lines()
        .find(|l| l.starts_with("registry compatibility:"))
        .expect("registry compatibility line");
    // One range, no `Java:` / `Bedrock:` prefix here — those belong on
    // axis 2 (edition portability) only.
    assert!(line.contains("1.20 .. latest"), "got: {line}");
    assert!(
        !line.contains("Java:"),
        "axis 1 should not repeat editions: {line}"
    );
    assert!(
        !line.contains("Bedrock:"),
        "axis 1 should not repeat editions: {line}"
    );
}

#[test]
fn info_7_empty_editions_value_is_rejected_with_exit_two() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap(), "--editions", ""]);
    assert_eq!(out.status.code(), Some(2));
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java,,bedrock"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_8_file_without_requires_defaults_min_to_zero_zero() {
    let path = tempfile_with_contents(
        "no_requires",
        "theme t:\n  slot m -> @oak_planks\n\nstruct s size=4x4\n  walls height=3 mat_slot=m\n",
    );
    let out = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(parsed["registry_compat"]["min"], "0.0");
}

/// The floor reaches `registry_compat.min` however the author spaced the
/// operator.
///
/// `version >= 1.21` used to leave `min` at `0.0` — the constraint was
/// dropped between the directive and the axis, and this is the command
/// where that was visible. Pinned at the CLI surface because that is where
/// it was seen; the library half is `cairn-lang-core`'s `check_requires`.
#[test]
fn info_9_requires_floor_reaches_the_registry_range_however_it_is_spaced() {
    for (label, header) in [
        ("tight", "@requires version>=1.21\n"),
        ("spaced", "@requires version >= 1.21\n"),
        ("left", "@requires version >=1.21\n"),
        ("right", "@requires version>= 1.21\n"),
    ] {
        let path = tempfile_with_contents(label, &format!("{header}struct s size=4x4\n"));
        let out = run_info(&[path.to_str().unwrap(), "--format", "json"]);
        assert!(
            out.status.success(),
            "{label}: stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        assert_eq!(parsed["registry_compat"]["min"], "1.21", "{label}");
    }
}

/// The `buildable targets` row is per edition, and so is the floor it is
/// weighed against.
///
/// `1.21.4` is Java's newest release and names no Bedrock release at all
/// — Bedrock ships `1.21.0 / 1.21.40 / 1.21.60`. The dotted-decimal
/// comparison this replaced read `1.21.40` as satisfying the floor on
/// `40 > 4` and reported two Bedrock targets as buildable; ordering by
/// `DataVersion` has no answer here and reports none, with the reason on
/// stderr rather than an empty row nobody can read.
#[test]
fn info_9b_a_floor_naming_no_release_of_an_edition_makes_none_of_it_buildable() {
    let path = tempfile_with_contents(
        "cross_edition_floor",
        "@requires version>=1.21.4\nstruct s size=4x4\n",
    );
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java,bedrock"]);
    assert!(
        out.status.success(),
        "info reports and does not refuse: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let row = stdout
        .lines()
        .find(|line| line.starts_with("buildable targets:"))
        .expect("the row is printed");
    assert!(
        row.contains("Java: 1.21.4") && row.contains("Bedrock: none"),
        "the floor is Java's newest release and no Bedrock release: {row}",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("names no bedrock release"),
        "`none` without a reason is not a report: {stderr}",
    );
}

/// And a floor scoped to one edition leaves the other alone.
#[test]
fn info_9c_a_scoped_floor_is_weighed_only_against_its_own_edition() {
    let path = tempfile_with_contents(
        "scoped_floor",
        "@requires java version>=1.21.4\nstruct s size=4x4\n",
    );
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java,bedrock"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let row = stdout
        .lines()
        .find(|line| line.starts_with("buildable targets:"))
        .expect("the row is printed");
    assert!(
        row.contains("Java: 1.21.4") && row.contains("Bedrock: 1.21.0, 1.21.40, 1.21.60"),
        "a Java-scoped floor says nothing about Bedrock: {row}",
    );
    // And the edition-neutral row takes only the unscoped floors, so a
    // floor written in one edition's numbering does not become the file's
    // range.
    let json = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(json.stdout).expect("utf-8")).expect("valid JSON");
    assert_eq!(parsed["registry_compat"]["min"], "0.0");
}

/// The note under `buildable targets` names the floor that refused, not
/// only the versions it refused.
///
/// A module may declare several floors and only one of them refuse. "These
/// versions are below the floor this file declares" then leaves the reader
/// to work out which line they have to edit, which is the half of the
/// report they can act on.
#[test]
fn info_9d_the_floor_note_names_the_line_that_refused() {
    let path = tempfile_with_contents(
        "two_floors",
        "@requires version>=1.20.4\n@requires java version>=1.21.4\nstruct s size=4x4\n",
    );
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("`java version>=1.21.4`"),
        "the refusing floor has to be named, with its scope: {stderr}",
    );
    assert!(
        !stderr.contains("version>=1.20.4`"),
        "and the floor every target clears is not part of that news: {stderr}",
    );
}

/// `registry compatibility: 0.0` beside a `buildable targets` row that
/// refuses versions is two true lines that disagree on their face.
///
/// The row is edition-neutral so only unscoped floors feed it, and `0.0`
/// therefore has two causes — no `@requires` line, and only scoped ones.
/// The reader is told which, rather than left to work it out.
#[test]
fn info_9e_a_scoped_only_file_says_why_the_neutral_row_is_empty() {
    let path = tempfile_with_contents(
        "scoped_only",
        "@requires java version>=1.21.4\nstruct s size=4x4\n",
    );
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("registry compatibility:  0.0 .. latest"),
        "the neutral row reads only unscoped floors: {stdout}",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("is scoped to one edition"),
        "and says so, at the line that scoped it: {stderr}",
    );
    assert!(
        stderr.contains(":1:1:"),
        "with the position, like every other note that names a line: {stderr}",
    );
}

/// A requirement `cairn check` rejects stops `cairn info` too, in both
/// output formats.
///
/// This is a contract change: the same file used to exit 0 and report
/// `min: "0.0"`. `info` runs the check passes, so a new `Error`-severity
/// code reaches it — and reporting a version range derived from a file
/// whose version declaration is itself a mistake is the confident-wrong
/// answer the code exists to stop.
#[test]
fn info_10_a_rejected_requirement_stops_info_in_both_formats() {
    let path = tempfile_with_contents(
        "bad_requires",
        "@requires version<1.20\nstruct s size=4x4\n",
    );
    let text = run_info(&[path.to_str().unwrap()]);
    assert_eq!(text.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&text.stdout).contains("E_INVALID_REQUIRES")
            || String::from_utf8_lossy(&text.stderr).contains("E_INVALID_REQUIRES"),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&text.stdout),
        String::from_utf8_lossy(&text.stderr),
    );

    let json = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(
        serde_json::from_slice::<serde_json::Value>(&json.stdout)
            .ok()
            .and_then(|v| v.get("registry_compat").cloned())
            .is_none(),
        "a refused file must not also report a range: {}",
        String::from_utf8_lossy(&json.stdout),
    );
}

/// Write a transient `.crn` under the system temp dir.
///
/// `label` distinguishes concurrent callers: the harness runs these as
/// threads of one process, so the pid alone would have two of them writing
/// and reading the same path.
fn tempfile_with_contents(label: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    path.push(format!("cairn-cli-info-{pid}-{label}.crn"));
    std::fs::write(&path, contents).expect("write tempfile");
    path
}

#[test]
fn info_9_a_note_that_points_at_a_second_line_is_printed_with_its_position() {
    // `cairn info` was one of the three commands whose note loop dropped
    // `note.span`; all six copies now share one renderer, and each of the
    // three needs its own end-to-end line or a fold that missed one of
    // them still passes.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("conflict.crn");
    std::fs::write(
        &path,
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "  slot glass -> @glass_pane\n",
            "\n",
            "struct t size=7x5\n",
            "  walls mat_slot=wall height=3\n",
            "  door side=front at=center\n",
            "  window side=front y=1 offset=3 size=1x2 mat_slot=glass\n",
        ),
    )
    .expect("write source");

    let out = run_info(&[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "a warning must not fail info, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains(":8:3: warning[W_PHASE_CONFLICT]"),
        "the finding anchors at the `window` on line 8, got: {stderr}",
    );
    assert!(
        stderr.contains(":7:3:   note: overwritten member declared here"),
        "the note must carry the `door` line's position, got: {stderr}",
    );
}
