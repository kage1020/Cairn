//! End-to-end tests for `cairn lower <file>`.

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

fn run_lower(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("lower")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn lower_1_cottage_exits_zero_and_names_the_struct() {
    let path = examples_dir().join("cottage.crn");
    let out = run_lower(&[path.to_str().unwrap()]);
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
    // overhang=1 inflates the bounding box to 11×?×9; the y axis grows by
    // the gable extra height (ceil(min(11,9)/2) = 5) above the floor +
    // walls, so dims=11x10x9.
    assert!(
        stdout.contains("dims=11x10x9"),
        "expected dims line, got: {stdout}",
    );
}

#[test]
fn lower_2_json_format_round_trips_as_block_array_ir() {
    let path = examples_dir().join("cottage.crn");
    let out = run_lower(&[path.to_str().unwrap(), "--format", "json"]);
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
fn lower_3_deferred_member_warnings_print_to_stderr() {
    // cottage.crn voxelises clean now; for the deferred-member regression
    // we use themed-tower.crn, whose top-level `level` blocks remain
    // outside the implemented phases.
    let path = examples_dir().join("themed-tower.crn");
    let out = run_lower(&[path.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("W_DEFERRED_MEMBER"),
        "expected at least one W_DEFERRED_MEMBER on themed-tower, stderr={stderr}",
    );
}

#[test]
fn lower_4_missing_file_exits_with_code_two() {
    let out = run_lower(&["does-not-exist.crn"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn lower_5_themed_tower_lifts_abstract_tokens_through_builtin_materials() {
    // M3-PR2: the built-in registry pack ships a materials catalog covering
    // every abstract token themed-tower.crn binds, so `W_ABSTRACT_TOKEN_DEFERRED`
    // must NOT appear. `W_DEFERRED_MEMBER` is still expected on its top-level
    // `level` blocks (level lowering arrives later), so exit stays 0.
    let path = examples_dir().join("themed-tower.crn");
    let out = run_lower(&[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        !stderr.contains("W_ABSTRACT_TOKEN_DEFERRED"),
        "abstract tokens must lift via the built-in materials catalog; stderr={stderr}",
    );
    assert!(
        !stderr.contains("E_UNKNOWN_ABSTRACT_TOKEN"),
        "every token themed-tower binds must be declared by the catalog; stderr={stderr}",
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    // The lifted ids must surface in the lowered palette.
    assert!(
        stdout.contains("minecraft:oak_planks"),
        "expected oak_planks from @floor.wood.broadleaf lift, stdout={stdout}",
    );
    assert!(
        stdout.contains("minecraft:cobblestone"),
        "expected cobblestone from @wall.stone.cobble lift, stdout={stdout}",
    );
}

#[test]
fn lower_7_unknown_abstract_token_exits_nonzero() {
    // Source rigs an abstract token the built-in catalog does NOT declare,
    // so resolution must surface E_UNKNOWN_ABSTRACT_TOKEN with a `did you
    // mean ...?` note and the process must exit non-zero. The closest
    // declared token (`floor.wood.broadleaf`) is one substitution away from
    // the typo, so `nearest_match` should propose it.
    let tmp = std::env::temp_dir().join("cairn-cli-unknown-abstract.crn");
    std::fs::write(
        &tmp,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "theme t:\n",
            "  slot floor -> @floor.wood.broadlef\n",
            "\n",
            "struct s size=3x3\n",
            "  floor mat_slot=floor\n",
        ),
    )
    .expect("write tmp .crn");
    let out = run_lower(&[tmp.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        !out.status.success(),
        "exit should be non-zero; stderr={stderr}",
    );
    assert!(
        stderr.contains("E_UNKNOWN_ABSTRACT_TOKEN"),
        "stderr should carry the new diagnostic code; got: {stderr}",
    );
    assert!(
        stderr.contains("floor.wood.broadleaf"),
        "stderr should surface the nearest declared token; got: {stderr}",
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn lower_6_all_examples_exit_zero() {
    for name in [
        "cottage.crn",
        "themed-tower.crn",
        "village.crn",
        "redstone-door.crn",
    ] {
        let path = examples_dir().join(name);
        let out = run_lower(&[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{name} should exit 0, stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
