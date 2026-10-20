//! `cairn info <file>` end to end: the version axes as text and JSON, and `--editions`.

use crate::common::{Fixture, assert_stream, cairn, crn_examples, examples_dir, stderr_of};

#[test]
fn clean_example_exits_zero_with_three_section_headers() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("info", &[path.to_str().unwrap()]);
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
fn json_format_is_valid_version_axes() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
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
    // `cottage.crn` declares `@requires version>=1.20`.
    assert_eq!(compat["min"], "1.20");

    let portability = parsed["edition_portability"]
        .as_array()
        .expect("portability is an array");
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
    // Nothing populates the semantic-sensitivity catalog, so this is empty.
    assert_eq!(*sensitive, [] as [serde_json::Value; 0]);
}

#[test]
fn editions_flag_controls_portability_entries() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn(
        "info",
        &[
            path.to_str().unwrap(),
            "--editions",
            "java",
            "--format",
            "json",
        ],
    );
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
fn all_examples_exit_zero() {
    for path in crn_examples() {
        let out = cairn("info", &[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{} should exit 0, stderr={}",
            path.display(),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn registry_compatibility_renders_as_single_range_line() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("info", &[path.to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let line = stdout
        .lines()
        .find(|l| l.starts_with("registry compatibility:"))
        .expect("registry compatibility line");
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
fn a_malformed_editions_list_is_a_usage_error() {
    let path = examples_dir().join("cottage.crn");
    for (editions, names) in [
        ("", &["empty entries"][..]),
        ("java,,bedrock", &["empty entries"][..]),
        ("foo", &["`foo`", "java, bedrock"][..]),
        ("java,foo", &["`foo`", "java, bedrock"][..]),
    ] {
        let out = cairn("info", &[path.to_str().unwrap(), "--editions", editions]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "--editions {editions:?}: stderr={stderr}",
        );
        assert!(
            out.stdout.is_empty(),
            "--editions {editions:?} printed a report",
        );
        for name in names {
            assert!(
                stderr.contains(name),
                "--editions {editions:?}: the refusal should say {name}, got: {stderr}",
            );
        }
    }
}

#[test]
fn file_without_requires_defaults_min_to_zero_zero() {
    let fixture = Fixture::new(
        "theme t:\n  slot m -> @oak_planks\n\nstruct s size=4x4\n  walls height=3 mat_slot=m\n",
    );
    let path = fixture.source();
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(parsed["registry_compat"]["min"], "0.0");
}

#[test]
fn requires_floor_reaches_the_registry_range_however_it_is_spaced() {
    for (label, header) in [
        ("tight", "@requires version>=1.21\n"),
        ("spaced", "@requires version >= 1.21\n"),
        ("left", "@requires version >=1.21\n"),
        ("right", "@requires version>= 1.21\n"),
    ] {
        let fixture = Fixture::new(&format!("{header}struct s size=4x4\n"));
        let path = fixture.source();
        let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
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

/// `1.21.4` names no Bedrock release; compared as dotted decimals, `1.21.40` would clear it.
#[test]
fn a_floor_naming_no_release_of_an_edition_makes_none_of_it_buildable() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.4\n{PAINTED_STRUCT}"));
    let path = fixture.source();
    let out = cairn(
        "info",
        &[path.to_str().unwrap(), "--editions", "java,bedrock"],
    );
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

#[test]
fn a_scoped_floor_is_weighed_only_against_its_own_edition() {
    let fixture = Fixture::new(&format!("@requires java version>=1.21.4\n{PAINTED_STRUCT}"));
    let path = fixture.source();
    let out = cairn(
        "info",
        &[path.to_str().unwrap(), "--editions", "java,bedrock"],
    );
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
    let json = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(json.stdout).expect("utf-8")).expect("valid JSON");
    assert_eq!(parsed["registry_compat"]["min"], "0.0");
}

#[test]
fn the_floor_note_names_the_line_that_refused() {
    let fixture = Fixture::new(
        "@requires version>=1.20.4\n@requires java version>=1.21.4\nstruct s size=4x4\n",
    );
    let path = fixture.source();
    let out = cairn("info", &[path.to_str().unwrap(), "--editions", "java"]);
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

#[test]
fn a_scoped_only_file_says_why_the_neutral_row_is_empty() {
    let fixture = Fixture::new("@requires java version>=1.21.4\nstruct s size=4x4\n");
    let path = fixture.source();
    let out = cairn("info", &[path.to_str().unwrap(), "--editions", "java"]);
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

#[test]
fn a_rejected_requirement_stops_info_in_both_formats() {
    let fixture = Fixture::new("@requires version<1.20\nstruct s size=4x4\n");
    let path = fixture.source();
    let text = cairn("info", &[path.to_str().unwrap()]);
    assert_eq!(text.status.code(), Some(1));
    assert_stream(&stderr_of(&text), &[(1, 1, "error", "E_INVALID_REQUIRES")]);

    let json = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
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

/// A struct that lays blocks on every target; a struct of air would refuse every target first.
const PAINTED_STRUCT: &str =
    "theme t:\n  slot m -> @oak_planks\n\nstruct s size=4x4\n  floor mat_slot=m\n";

#[test]
fn a_note_that_points_at_a_second_line_is_printed_with_its_position() {
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

    let out = cairn("info", &[path.to_str().unwrap()]);
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

/// `spec/versioning-editions` "Fail-loud and minimum-version inference": a composite's minimum
/// version is the max of its parts.
#[test]
fn the_neutral_row_reads_a_floor_a_placed_def_declares() {
    let fixture = Fixture::new(
        "theme t:\n  slot floor -> @oak_planks\n\
         \ndef cottage size=4x4:\n  requires version>=1.21.4\n  floor mat_slot=floor\n\
         \nsite hamlet:\n  place id=a use=cottage theme=t at=origin\n",
    );
    let path = fixture.source();
    let out = cairn("info", &[path.to_str().unwrap(), "--editions", "java"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("registry compatibility:  1.21.4 .. latest"),
        "{stdout}",
    );
    assert!(
        stdout.contains("buildable targets:       Java: 1.21.4"),
        "and the per-edition row is held to it too: {stdout}",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("below the `version>=1.21.4` `def cottage` declares"),
        "the note names the part, not `this file`: {stderr}",
    );
}

#[test]
fn a_theme_bound_per_edition_is_left_out_of_the_neutral_row() {
    let fixture = Fixture::new(
        "theme shop:\n  requires version>=1.21.4\n  slot floor -> @oak_planks\n\
         \ntheme shop_bedrock:\n  slot floor -> @oak_planks\n\
         \nstruct s size=4x4\n  floor mat_slot=floor\n",
    );
    let path = fixture.source();
    let out = cairn("info", &[path.to_str().unwrap(), "--editions", "bedrock"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("registry compatibility:  0.0 .. latest"),
        "a floor Bedrock does not inherit must not set the row's edge: {stdout}",
    );
    assert!(
        stdout.contains("buildable targets:       Bedrock: 1.21.0, 1.21.40, 1.21.60"),
        "and the per-edition row refuses nothing, so the two agree: {stdout}",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("the two editions bind different variants of it"),
        "`0.0` beside a floor in the file needs its reason: {stderr}",
    );
}

/// `pale_moss_block` arrives in Java 1.21.4 and Bedrock 1.21.60.
#[test]
fn the_declared_range_is_not_an_answer_about_which_versions_build() {
    let floored_fixture = Fixture::new(
        "@requires version>=1.20.4\n\ntheme pale:\n  slot floor -> @pale_moss_block\n\
         \x20\x20slot wall -> @cobblestone\n\
         \nstruct hut size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n",
    );
    let floored = floored_fixture.source();
    let stdout = info_rows(&floored, "java");
    let (edge, refused) = declared_edge_and_refusals(&stdout);
    assert!(
        refused.contains(&edge.as_str()),
        "the row's own lower edge is among the versions that refuse: {stdout}",
    );
    assert!(
        stdout.contains("registry compatibility:  1.20.4 .. latest"),
        "the declared floor is the row's lower edge: {stdout}",
    );
    assert!(
        stdout.contains("buildable targets:       Java: 1.21.4 (1.20.4, 1.21 refuse)"),
        "and the derived row names what actually builds: {stdout}",
    );

    let floorless_fixture = Fixture::new(
        "theme pale:\n  slot floor -> @pale_moss_block\n  slot wall -> @cobblestone\n\
         \nstruct hut size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n",
    );
    let floorless = floorless_fixture.source();
    let stdout = info_rows(&floorless, "java,bedrock");
    assert!(
        stdout.contains("registry compatibility:  0.0 .. latest"),
        "no `@requires` line and no part declaring one, so nothing feeds the row: {stdout}",
    );
    assert!(
        stdout.contains(
            "buildable targets:       Java: 1.21.4 (1.20.4, 1.21 refuse)   \
             Bedrock: 1.21.60 (1.21.0, 1.21.40 refuse)"
        ),
        "four of the six supported versions refuse it, on both editions: {stdout}",
    );
}

/// `cairn info`'s stdout rows for one source, or a panic naming stderr.
fn info_rows(path: &std::path::Path, editions: &str) -> String {
    let out = cairn("info", &[path.to_str().unwrap(), "--editions", editions]);
    assert!(
        out.status.success(),
        "the rows are a report, not a refusal: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

/// The lower edge of the `registry compatibility` row, and every version `buildable targets`
/// refuses, each read off its own row.
fn declared_edge_and_refusals(stdout: &str) -> (String, Vec<&str>) {
    let row = |label: &str| {
        stdout
            .lines()
            .find(|line| line.starts_with(label))
            .unwrap_or_else(|| panic!("every completed run prints `{label}`: {stdout}"))
    };
    let edge = row("registry compatibility:")
        .split_whitespace()
        .nth(2)
        .expect("`<min> .. <max>`")
        .to_owned();
    let refused: Vec<&str> = row("buildable targets:")
        .split('(')
        .skip(1)
        .filter_map(|tail| tail.split_once(" refuse"))
        .flat_map(|(names, _)| names.trim_end_matches(" all").split(", "))
        .collect();
    assert!(!refused.is_empty(), "premise: something refuses: {stdout}");
    (edge, refused)
}

/// The Java-scoped floor gives stderr a note per Java walk, which a repeated `java` would double.
#[test]
fn an_edition_named_twice_is_reported_once() {
    let fixture = Fixture::new(&format!("@requires java version>=1.21.4\n{PAINTED_STRUCT}"));
    let path = fixture.source();
    let path = path.to_str().unwrap();
    for (repeated, distinct) in [
        ("java,java", "java"),
        ("java,bedrock,java", "java,bedrock"),
        ("bedrock,bedrock", "bedrock"),
        ("bedrock,java,bedrock", "bedrock,java"),
    ] {
        for format in ["text", "json"] {
            let run = |editions: &str| {
                let out = cairn("info", &[path, "--editions", editions, "--format", format]);
                assert!(
                    out.status.success(),
                    "--editions {editions} --format {format}: {}",
                    String::from_utf8_lossy(&out.stderr),
                );
                (
                    String::from_utf8(out.stdout).expect("utf-8"),
                    String::from_utf8(out.stderr).expect("utf-8"),
                )
            };
            let (stdout, stderr) = run(distinct);
            assert!(
                !stderr.is_empty(),
                "premise: the floor puts a note on stderr: {stderr}",
            );
            if format == "json" {
                let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
                for axis in ["edition_portability", "buildable_targets"] {
                    let listed: Option<Vec<&str>> = parsed[axis].as_array().map(|entries| {
                        entries
                            .iter()
                            .map(|entry| entry["edition"].as_str().unwrap_or_default())
                            .collect()
                    });
                    assert_eq!(
                        listed,
                        Some(distinct.split(',').collect()),
                        "one {axis} entry per distinct edition asked, in the order typed: {parsed}",
                    );
                }
            }
            assert_eq!(
                run(repeated),
                (stdout, stderr),
                "--editions {repeated} should report what --editions {distinct} does \
                 (--format {format})",
            );
        }
    }
}
