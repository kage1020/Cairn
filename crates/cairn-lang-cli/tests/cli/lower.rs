//! `cairn lower <file>` end to end: the IR in each format, and the findings on stderr.

use std::collections::BTreeSet;
use std::fs;
use tempfile::TempDir;

use crate::common::{
    Run, WALL_LEVER, assert_stream, cairn, crn_examples, examples_dir, write_source,
};

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
fn members_under_edition_guards_are_left_out_and_counted_in_a_note() {
    let path = examples_dir().join("edition-fallback.crn");
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert_eq!(
        stderr,
        "note: no edition is pinned here, so the 2 members under `@edition` guards are left out; \
         `cairn lower --edition E` lowers the ones E's guards hold\n",
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    let palette = parsed["structures"]["struct::shop"]["palette"]
        .as_array()
        .expect("palette is an array");
    let mut ids: Vec<&str> = palette
        .iter()
        .map(|p| p["id"].as_str().expect("id is a string"))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids,
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:oak_door",
            "minecraft:oak_sign",
            "minecraft:stone_bricks",
        ],
    );
}

/// The distinct block ids of the struct `key` in the JSON IR `stdout`, sorted.
fn palette_ids(stdout: &str, key: &str) -> Vec<String> {
    let parsed: serde_json::Value = serde_json::from_str(stdout).expect("valid JSON");
    let ids: BTreeSet<String> = parsed["structures"][key]["palette"]
        .as_array()
        .expect("palette is an array")
        .iter()
        .map(|p| p["id"].as_str().expect("id is a string").to_owned())
        .collect();
    ids.into_iter().collect()
}

#[test]
fn an_edition_pin_lowers_the_members_its_guards_hold() {
    let path = examples_dir().join("edition-fallback.crn");
    let run = Run::from(cairn(
        "lower",
        &[
            path.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--format",
            "json",
        ],
    ));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert_eq!(run.stderr, "");
    assert_eq!(
        palette_ids(&run.stdout, "struct::shop"),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:light_block_15",
            "minecraft:stone_bricks",
            "minecraft:wall_sign",
            "minecraft:wooden_door",
        ],
    );
}

#[test]
fn a_struct_rises_over_its_circuit_only_where_an_edition_is_pinned() {
    let path = examples_dir().join("redstone-door.crn");
    let file = path.to_str().unwrap();
    let dims = |stdout: &str| {
        let parsed: serde_json::Value = serde_json::from_str(stdout).expect("valid JSON");
        parsed["structures"]["struct::gatehouse"]["dims"].clone()
    };
    let unpinned = Run::from(cairn("lower", &[file, "--format", "json"]));
    assert_eq!(unpinned.code, Some(0), "stderr={}", unpinned.stderr);
    assert_eq!(
        unpinned.stderr,
        "note: no edition is pinned here, so the circuit of the scope with a `circuit` line is \
         left out; `cairn lower --edition E` lays the ones E builds\n",
    );
    assert_eq!(
        dims(&unpinned.stdout),
        serde_json::json!({"x": 9, "y": 5, "z": 7})
    );
    for edition in ["java", "bedrock"] {
        let pinned = Run::from(cairn(
            "lower",
            &[file, "--edition", edition, "--format", "json"],
        ));
        assert_eq!(pinned.code, Some(0), "{edition}: stderr={}", pinned.stderr);
        assert_eq!(pinned.stderr, "", "{edition}");
        assert_eq!(
            dims(&pinned.stdout),
            serde_json::json!({"x": 9, "y": 8, "z": 7}),
            "{edition}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&pinned.stdout).expect("valid JSON");
        assert_eq!(
            parsed["members"]["struct::gatehouse"][0]["role"], "circuit",
            "{edition}"
        );
    }
}

#[test]
fn a_circuit_finding_fails_a_lower_pinned_to_an_edition() {
    let tmp = TempDir::new().expect("tempdir");
    let file = write_source(tmp.path(), "wall-lever.crn", WALL_LEVER);
    let file = file.to_str().unwrap();
    let pinned = Run::from(cairn("lower", &[file, "--edition", "java"]));
    assert_eq!(pinned.code, Some(1), "stderr={}", pinned.stderr);
    assert_stream(
        &pinned.stderr,
        &[(8, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")],
    );
    assert_eq!(pinned.stdout, "");
    let unpinned = Run::from(cairn("lower", &[file]));
    assert_eq!(unpinned.code, Some(0), "stderr={}", unpinned.stderr);
    assert_stream(&unpinned.stderr, &[]);
}

#[test]
fn the_note_counts_the_members_under_guards_and_not_the_guards() {
    let source = "struct s size=5x5\n  raw_block mat=minecraft:dirt at=1,0,1\n  @edition java\n    \
                  raw_block mat=minecraft:stone at=2,0,2\n    raw_block mat=minecraft:stone at=3,0,3\n";
    let tmp = TempDir::new().expect("tempdir");
    let src_path = write_source(tmp.path(), "guarded.crn", source);
    let run = Run::from(cairn("lower", &[src_path.to_str().unwrap()]));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert_eq!(
        run.stderr,
        "note: no edition is pinned here, so the 2 members under `@edition` guards are left out; \
         `cairn lower --edition E` lowers the ones E's guards hold\n",
    );
}

#[test]
fn a_guarded_line_check_refuses_fails_the_lowering_that_leaves_it_out() {
    let source = "struct s size=5x5\n  @edition java\n    raw_block mat=minecraft:stone at=2,4,2\n  \
                  raw_block mat=minecraft:dirt at=1,0,1\n";
    let tmp = TempDir::new().expect("tempdir");
    let src_path = write_source(tmp.path(), "guarded.crn", source);
    let run = Run::from(cairn("lower", &[src_path.to_str().unwrap()]));
    assert_eq!(run.code, Some(1), "stderr={}", run.stderr);
    assert_stream(&run.stderr, &[(3, 5, "error", "E_INVALID_MEMBER")]);
    assert_eq!(run.stdout, "");
}

#[test]
fn deferred_member_warnings_print_to_stderr() {
    // Lowering has no roof under a raised `level`, so it defers.
    let source = concat!(
        "theme t:\n",
        "  slot floor -> @spruce_planks\n",
        "\n",
        "struct s size=2x2\n",
        "  floor mat_slot=floor\n",
        "  level y=1\n",
        "    roof kind=flat\n",
    );
    let tmp = TempDir::new().expect("tempdir");
    let src_path = tmp.path().join("level-roof.crn");
    fs::write(&src_path, source).expect("write source");
    let out = cairn("lower", &[src_path.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert_stream(&stderr, &[(7, 5, "warning", "W_DEFERRED_MEMBER")]);
    assert!(
        stderr.contains("level-scoped `roof` is not yet supported"),
        "the deferred primary should name what is not lowered, stderr={stderr}",
    );
}

#[test]
fn a_lowering_stage_error_exits_one() {
    // `check` passes this source: only lowering looks the token up.
    let source = concat!(
        "theme t:\n",
        "  slot floor -> @floor.wood.broadlef\n",
        "\n",
        "struct s size=2x2\n",
        "  floor mat_slot=floor\n",
    );
    let tmp = TempDir::new().expect("tempdir");
    let src_path = write_source(tmp.path(), "token-typo.crn", source);
    let run = Run::from(cairn("lower", &[src_path.to_str().unwrap()]));
    assert_eq!(run.code, Some(1), "stderr={}", run.stderr);
    assert_stream(&run.stderr, &[(2, 17, "error", "E_UNKNOWN_ABSTRACT_TOKEN")]);
    assert_eq!(run.stdout, "");
}

/// A block Java first declares at 1.21.4, on line 2.
const PALE_OAK_SHED: &str = concat!(
    "struct shed size=3x3\n",
    "  raw_block mat=minecraft:pale_oak_planks at=1,0,1\n",
);

#[test]
fn a_target_checks_block_ids_against_its_version() {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(tmp.path(), "shed.crn", PALE_OAK_SHED);
    let path = path.to_str().unwrap();

    let before = Run::from(cairn(
        "lower",
        &[path, "--edition", "java", "--target", "1.20.4"],
    ));
    assert_eq!(before.code, Some(1), "stderr={}", before.stderr);
    assert_stream(&before.stderr, &[(2, 17, "error", "E_UNKNOWN_ID")]);
    assert_eq!(before.stdout, "");

    let unpinned = Run::from(cairn("lower", &[path, "--edition", "java"]));
    assert_eq!(unpinned.code, Some(0), "stderr={}", unpinned.stderr);
    let since = Run::from(cairn(
        "lower",
        &[
            path,
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--format",
            "json",
        ],
    ));
    assert_eq!(since.code, Some(0), "stderr={}", since.stderr);
    assert_eq!(
        palette_ids(&since.stdout, "struct::shed"),
        ["minecraft:air", "minecraft:pale_oak_planks"],
    );
}

#[test]
fn a_target_the_edition_does_not_ship_is_refused_before_the_file_is_read() {
    let run = Run::from(cairn(
        "lower",
        &["missing.crn", "--edition", "java", "--target", "9.9.9"],
    ));
    assert_eq!(run.code, Some(1));
    assert!(
        run.stderr
            .starts_with("error: unsupported java target `9.9.9`. supported targets: "),
        "stderr={}",
        run.stderr,
    );
    assert_eq!(run.stderr.lines().count(), 1, "stderr={}", run.stderr);
    assert_eq!(run.stdout, "");
}

#[test]
fn a_target_without_an_edition_is_a_usage_error() {
    let path = examples_dir().join("cottage.crn");
    let run = Run::from(cairn(
        "lower",
        &[path.to_str().unwrap(), "--target", "1.21.4"],
    ));
    assert_eq!(run.code, Some(2), "stderr={}", run.stderr);
    assert_eq!(run.stdout, "");
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
