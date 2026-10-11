//! `cairn lower <file>` end to end: the IR in each format, and the findings on stderr.

use std::fs;
use tempfile::TempDir;

use crate::common::{assert_stream, cairn, crn_examples, examples_dir};

#[test]
fn cottage_exits_zero_and_names_the_struct() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("lower", &[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("struct::cottage"),
        "expected scope key in stdout, got: {stdout}",
    );
    assert!(
        stdout.contains("dims=11x10x9"),
        "expected dims line, got: {stdout}",
    );
}

#[test]
fn json_format_round_trips_as_block_array_ir() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let cottage = &parsed["structures"]["struct::cottage"];
    assert_eq!(cottage["dims"]["x"], 11);
    assert_eq!(cottage["dims"]["y"], 10);
    assert_eq!(cottage["dims"]["z"], 9);
    let palette = cottage["palette"].as_array().expect("palette is an array");
    let ids: Vec<&str> = palette
        .iter()
        .map(|p| p["id"].as_str().expect("id is a string"))
        .collect();
    assert!(ids.contains(&"minecraft:air"));
    assert!(ids.contains(&"minecraft:cobblestone"));
    assert!(ids.contains(&"minecraft:oak_planks"));
    assert!(ids.contains(&"minecraft:spruce_stairs"));
    assert!(ids.contains(&"minecraft:glass_pane"));
}

#[test]
fn deferred_member_warnings_print_to_stderr() {
    // Stair lowering has no `inner_left`, so it defers.
    let source = concat!(
        "theme t:\n",
        "  slot floor -> spruce_planks\n",
        "\n",
        "struct s size=2x2\n",
        "  floor mat_slot=floor\n",
        "  roof kind=flat overhang=1\n",
        "  stair kind=stairs side=front shape=inner_left\n",
    );
    let tmp = TempDir::new().expect("tempdir");
    let src_path = tmp.path().join("stair-inner-left.crn");
    fs::write(&src_path, source).expect("write source");
    let out = cairn("lower", &[src_path.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(
        &stderr,
        &[
            (2, 17, "error", "E_UNKNOWN_SLOT_TARGET"),
            (7, 3, "warning", "W_DEFERRED_MEMBER"),
        ],
    );
    assert!(
        stderr.contains("shape=inner_left"),
        "the deferred primary should name the offending `shape=inner_left`, stderr={stderr}",
    );
}

#[test]
fn all_examples_exit_zero() {
    for path in crn_examples() {
        let out = cairn("lower", &[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{} should exit 0, stderr={}",
            path.display(),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn a_note_that_points_at_a_second_line_is_printed_with_its_position() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("conflict.crn");
    fs::write(
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

    let out = cairn("lower", &[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "a warning must not fail the lower, stderr={}",
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
    assert!(
        stderr.contains(
            "
  note: If the override is deliberate"
        ),
        "a span-less note keeps the bare `  note:` form, got: {stderr}",
    );
}
