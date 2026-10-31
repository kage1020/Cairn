//! `cairn info --editions`: each edition's portability figures and buildable targets match what
//! `cairn compile` does with the same source.

use std::path::PathBuf;
use std::process::Command;

use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_formats::bedrock_state::degradation_detail;
use serde_json::Value;

use crate::common::{
    assert_stream, cargo_bin, compile, crn_examples, example_in_tempdir, examples_dir, stderr_of,
};

fn info_json(file: &str, editions: &str) -> Value {
    info_json_at(&examples_dir().join(file), editions)
}

/// Run `cairn info --format json`, checking each per-edition axis has one entry per edition.
fn info_json_at(path: &std::path::Path, editions: &str) -> Value {
    let out = Command::new(cargo_bin())
        .args([
            "info",
            path.to_str().unwrap(),
            "--editions",
            editions,
            "--format",
            "json",
        ])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "cairn info failed for {}; stderr={}",
        path.display(),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let axes: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let mut distinct: Vec<&str> = Vec::new();
    for name in editions.split(',') {
        if !distinct.contains(&name) {
            distinct.push(name);
        }
    }
    for axis in ["edition_portability", "buildable_targets"] {
        assert_eq!(
            axes[axis].as_array().map(Vec::len),
            Some(distinct.len()),
            "one {axis} entry per distinct edition in `--editions {editions}`: {axes}",
        );
    }
    axes
}

/// Run `cairn info` and hand back the exit code with both streams.
fn info_raw(path: &std::path::Path, editions: &str) -> (Option<i32>, String, String) {
    let out = Command::new(cargo_bin())
        .args(["info", path.to_str().unwrap(), "--editions", editions])
        .output()
        .expect("run cairn");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The `buildable_targets` row for one edition.
fn buildable_entry<'a>(axes: &'a Value, edition: &str) -> &'a Value {
    axes["buildable_targets"]
        .as_array()
        .expect("buildable_targets is a JSON array")
        .iter()
        .find(|e| e["edition"] == edition)
        .unwrap_or_else(|| panic!("edition `{edition}` missing from the buildable list"))
}

/// A JSON string array as owned strings, so a failure prints the versions.
fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` is not an array in {value}"))
        .iter()
        .map(|v| v.as_str().expect("version is a string").to_owned())
        .collect()
}

/// Whether `cairn compile` accepts this source for one pinned target.
///
/// `--lock` goes inside `out`, so no call reads back another's lockfile.
fn compile_accepts(
    path: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
) -> bool {
    compile(
        path,
        edition,
        target,
        out,
        Some(&out.join("build.crn.lock")),
    )
    .status
    .success()
}

fn portability_entry<'a>(axes: &'a Value, edition: &str) -> &'a Value {
    axes["edition_portability"]
        .as_array()
        .expect("edition_portability is a JSON array")
        .iter()
        .find(|e| e["edition"] == edition)
        .unwrap_or_else(|| panic!("edition `{edition}` missing from portability list"))
}

fn as_u64(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or_else(|| {
        panic!(
            "expected {key} to be an unsigned integer, got {}",
            value[key]
        )
    })
}

#[test]
fn themed_tower_bedrock_reports_at_least_one_degraded_palette_entry() {
    // The eave's `shape=outer_left` has no Bedrock state.
    let axes = info_json("themed-tower.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    let degraded = as_u64(bedrock, "degraded");
    let unsupported = as_u64(bedrock, "unsupported");
    assert!(
        degraded >= 1,
        "expected at least one degraded Bedrock palette entry in themed-tower, got {degraded}",
    );
    assert_eq!(
        unsupported, 0,
        "themed-tower on Bedrock should have no unsupported entries (only degraded stair shape), got {unsupported}",
    );
}

#[test]
fn cottage_ports_to_bedrock_without_loss() {
    // Gable stairs are `shape=straight`, Bedrock's default, so dropping the shape loses nothing.
    let axes = info_json("cottage.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        (as_u64(bedrock, "unsupported"), as_u64(bedrock, "degraded")),
        (0, 0),
    );
}

#[test]
fn the_java_axis_is_pure_portable_for_a_source_java_can_build() {
    // Java is the base edition for states (`spec/versioning-editions` "Backend = data tables").
    for file in ["themed-tower.crn", "cottage.crn"] {
        let axes = info_json(file, "java");
        let java = portability_entry(&axes, "java");
        assert_eq!(
            as_u64(java, "degraded"),
            0,
            "{file}: Java must never report degraded entries",
        );
        assert_eq!(
            as_u64(java, "unsupported"),
            0,
            "{file}: Java must never report unsupported entries",
        );
        assert!(
            as_u64(java, "portable") > 0,
            "{file}: Java should always have at least one portable palette entry",
        );
    }
}

/// Compile a temp copy of `examples/<example>` for Bedrock; return its `W_INTENT_DEGRADED` lines.
fn degraded_warnings(example: &str) -> Vec<String> {
    let (tmp, copied) = example_in_tempdir(example);
    let compiled = compile(&copied, "bedrock", "latest", tmp.path(), None);
    let stderr = stderr_of(&compiled);
    assert!(
        compiled.status.success(),
        "compile {example} failed; stderr={stderr}"
    );
    stderr
        .lines()
        .filter(|line| line.contains("W_INTENT_DEGRADED"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn info_degraded_count_matches_compile_intent_degraded_warnings() {
    let axes = info_json("themed-tower.crn", "bedrock");
    let degraded = as_u64(portability_entry(&axes, "bedrock"), "degraded");
    assert!(
        degraded >= 1,
        "expected themed-tower Bedrock degraded >= 1 for cross-consistency check, got {degraded}",
    );

    let warnings = degraded_warnings("themed-tower.crn");
    assert!(
        !warnings.is_empty(),
        "info reported degraded >= 1 but compile emitted no W_INTENT_DEGRADED; \
         info and compile paths must share `translate_states` as source of truth",
    );
}

#[test]
fn each_dropped_intent_gets_a_warning_naming_the_entry_info_names() {
    // `roof-hip`'s four degraded entries are four states of one id.
    let axes = info_json("roof-hip.crn", "bedrock");
    let entries = portability_entry(&axes, "bedrock")["degraded_entries"]
        .as_array()
        .expect("degraded_entries is a JSON array");
    assert!(entries.len() > 1, "premise: several entries degrade");

    let from_info: Vec<String> = entries
        .iter()
        .flat_map(|entry| {
            let id = entry["id"].as_str().expect("an id");
            let states = entry["states"].as_str().expect("states");
            let dropped = entry["dropped"].as_array().expect("what it lost");
            dropped.iter().map(move |one| {
                assert_eq!(one["key"], "shape", "got: {entry}");
                let value = one["value"].as_str().expect("the value that was asked for");
                let detail = degradation_detail(&DroppedIntent::Shape {
                    value: value.to_owned(),
                });
                format!("warning[W_INTENT_DEGRADED]: struct::roof_hip: {id}[{states}]: {detail}")
            })
        })
        .collect();

    let warnings = degraded_warnings("roof-hip.crn");
    assert_eq!(warnings, from_info);
    assert_eq!(
        warnings[0],
        "warning[W_INTENT_DEGRADED]: struct::roof_hip: \
         minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]: Bedrock has no \
         `shape` state; its stairs render straight, so corners show visual gaps",
    );
}

#[test]
fn an_edition_specific_resolve_failure_is_reported_not_counted_away() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("split.crn");
    std::fs::write(
        &src,
        "@cairn 2026.06\n\n\
         theme shop_java:\n\
         \x20\x20slot wall           -> @wall.stone.cobble\n\
         \x20\x20slot floating_text  -> @sign.oak\n\n\
         theme shop_bedrock:\n\
         \x20\x20slot wall           -> @wall.stone.cobble\n\n\
         struct shop size=5x5\n\
         \x20\x20walls  class=outer mat_slot=wall height=3\n\
         \x20\x20window class=display side=front offset=2 y=2 size=1x1 mat_slot=floating_text\n",
    )
    .expect("write split source");
    let path = src.to_str().unwrap();

    // Premise: the edition-neutral gate passes.
    let check = Command::new(cargo_bin())
        .args(["check", path])
        .output()
        .expect("run cairn");
    assert_eq!(check.status.code(), Some(0), "premise: check accepts it");

    // Bedrock cannot resolve it, and `compile` says so.
    let compiled = compile(&src, "bedrock", "latest", &tmp.path().join("out"), None);
    assert_eq!(
        compiled.status.code(),
        Some(1),
        "premise: compile refuses it"
    );

    let info = Command::new(cargo_bin())
        .args(["info", path, "--editions", "java,bedrock"])
        .output()
        .expect("run cairn");
    let stderr = String::from_utf8_lossy(&info.stderr).into_owned();
    assert_eq!(
        info.status.code(),
        Some(1),
        "info must not pass a source compile refuses; stderr={stderr}",
    );
    assert_stream(&stderr, &[(12, 3, "error", "E_UNRESOLVED_SLOT")]);
    assert!(
        stderr.contains("bedrock"),
        "the report must name the edition the finding belongs to; stderr={stderr}",
    );
    assert!(
        info.stdout.is_empty(),
        "no portability table should be printed when a figure could not be computed",
    );
    assert_eq!(
        stderr.matches("E_UNRESOLVED_SLOT").count(),
        1,
        "a finding the range-wide pass already reported is not repeated per \
         target; stderr={stderr}",
    );
}
// -- buildable targets ----------------------------------------------------

#[test]
fn a_source_no_supported_version_can_build_says_so() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("split.crn");
    std::fs::write(
        &src,
        "theme t:\n\
         \x20\x20slot floor -> @stonebrick\n\
         \x20\x20slot wall  -> @pale_moss_block\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n",
    )
    .expect("write source");

    // Both blocks exist on Bedrock, on different parts of its range.
    let axes = info_json_at(&src, "bedrock");
    assert_eq!(
        as_u64(portability_entry(&axes, "bedrock"), "unsupported"),
        0
    );
    let entry = buildable_entry(&axes, "bedrock");
    assert!(
        strings(entry, "buildable").is_empty(),
        "no supported target builds this source, so none should be listed",
    );
    assert_eq!(
        strings(entry, "considered"),
        ["1.21.0", "1.21.40", "1.21.60"]
    );

    // The premise, asked of the command that decides it.
    let out = tmp.path().join("out");
    for version in ["1.21.0", "1.21.40", "1.21.60"] {
        assert!(
            !compile_accepts(&src, "bedrock", version, &out.join(version)),
            "premise: bedrock {version} should refuse this source",
        );
    }

    // Matched as a whole line: `none` alone is also under `semantic-sensitive`.
    let (code, stdout, stderr) = info_raw(&src, "bedrock");
    assert_eq!(
        code,
        Some(0),
        "info reports; the build refuses. stderr={stderr}",
    );
    assert!(
        stdout.contains(
            "buildable targets:       Bedrock: none (1.21.0, 1.21.40, 1.21.60 all refuse)"
        ),
        "the row must carry the same answer the JSON does, got: {stdout}",
    );
    assert_stream(
        &stderr,
        &[
            (3, 17, "error", "E_UNKNOWN_ID"),
            (2, 17, "error", "E_UNKNOWN_ID"),
            (3, 17, "error", "E_UNKNOWN_ID"),
            (2, 17, "error", "E_UNKNOWN_ID"),
        ],
    );
    assert!(
        stderr.contains("pale_moss_block"),
        "the refusing ids should reach the user: {stderr}",
    );
}

/// Unpinned, `@floor.stone.smooth` takes its default mapping, which 1.21.0 respells.
#[test]
fn a_palette_whose_default_ids_never_overlap_can_still_have_a_buildable_target() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("mix.crn");
    std::fs::write(
        &src,
        "theme t:\n\
         \x20\x20slot floor -> @floor.stone.smooth\n\
         \x20\x20slot wall  -> @stonebrick\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n",
    )
    .expect("write source");

    let axes = info_json_at(&src, "bedrock");
    let entry = buildable_entry(&axes, "bedrock");
    assert_eq!(strings(entry, "buildable"), ["1.21.0"]);
    assert_eq!(
        strings(entry, "considered"),
        ["1.21.0", "1.21.40", "1.21.60"],
        "the versions weighed are carried too, so an empty set can be read",
    );

    let out = tmp.path().join("out");
    assert!(compile_accepts(&src, "bedrock", "1.21.0", &out));
    assert!(!compile_accepts(&src, "bedrock", "1.21.40", &out));
}

#[test]
fn a_warning_at_every_version_leaves_every_target_buildable() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("warned.crn");
    std::fs::write(
        &src,
        "theme t:\n\
         \x20\x20slot floor -> @cobblestone\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20level y=1\n\
         \x20\x20\x20\x20roof kind=flat\n",
    )
    .expect("write source");

    let axes = info_json_at(&src, "bedrock");
    let entry = buildable_entry(&axes, "bedrock");
    assert_eq!(
        strings(entry, "buildable"),
        strings(entry, "considered"),
        "a roof lowering does not draw under a raised `level` is a warning, not a refusal",
    );
}

#[test]
fn every_example_lists_every_supported_target() {
    for path in crn_examples() {
        let name = path.display();
        let axes = info_json_at(&path, "java,bedrock");
        for edition in ["java", "bedrock"] {
            let entry = buildable_entry(&axes, edition);
            assert_eq!(
                strings(entry, "buildable"),
                strings(entry, "considered"),
                "{name} on {edition} should build on every supported target",
            );
        }
    }
}

#[test]
fn the_buildable_set_is_the_set_of_targets_compile_accepts() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let fixtures: &[(&str, &str)] = &[
        // Two blocks Bedrock declares on disjoint parts of its range.
        (
            "split.crn",
            "theme t:\n\
             \x20\x20slot floor -> @stonebrick\n\
             \x20\x20slot wall  -> @pale_moss_block\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\
             \x20\x20walls class=outer mat_slot=wall height=3\n",
        ),
        // A default mapping the earliest target respells, beside the respelling written out.
        (
            "mix.crn",
            "theme t:\n\
             \x20\x20slot floor -> @floor.stone.smooth\n\
             \x20\x20slot wall  -> @stonebrick\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\
             \x20\x20walls class=outer mat_slot=wall height=3\n",
        ),
        // A floor above part of the supported range.
        (
            "floor.crn",
            "@requires version>=1.21.40\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A scope that does not lower: only `E_PARTIAL_BUILD` refuses it.
        (
            "partial.crn",
            "theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\n\
             struct nosize\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A floor no edition's table can place, refusing every version at once.
        (
            "unorderable.crn",
            "@requires version>=24w14a\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A floor Java places and Bedrock cannot, so the editions disagree in one run.
        (
            "cross_edition.crn",
            "@requires version>=1.21.4\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A state Bedrock's writer cannot carry, refused after every source gate has passed.
        (
            "states.crn",
            "theme t:\n\
             \x20\x20slot floor -> @oak_log[axis=x]\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // One that builds everywhere, so "refuse everything" fails.
        (
            "clean.crn",
            "theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
    ];

    for (name, source) in fixtures {
        let path = tmp.path().join(name);
        std::fs::write(&path, source).expect("write fixture");
        let axes = info_json_at(&path, "java,bedrock");
        for edition in ["java", "bedrock"] {
            let entry = buildable_entry(&axes, edition);
            let reported = strings(entry, "buildable");
            let accepted: Vec<String> = strings(entry, "considered")
                .into_iter()
                .filter(|version| {
                    let out = tmp.path().join(format!("{name}-{edition}-{version}"));
                    compile_accepts(&path, edition, version, &out)
                })
                .collect();
            assert_eq!(
                reported, accepted,
                "{name} on {edition}: the row and `compile --target` disagree",
            );
        }
    }
}

#[test]
fn the_versions_weighed_are_listed_in_release_order() {
    let axes = info_json_at(&examples_dir().join("cottage.crn"), "java,bedrock");
    assert_eq!(
        strings(buildable_entry(&axes, "java"), "considered"),
        ["1.20.4", "1.21", "1.21.4"],
    );
    assert_eq!(
        strings(buildable_entry(&axes, "bedrock"), "considered"),
        ["1.21.0", "1.21.40", "1.21.60"],
    );
}

// -- why `buildable` is empty ---------------------------------------------

/// `source`'s `buildable_targets` row for `edition`, with its temp dir kept alive.
fn buildable_row(source: &str, edition: &str) -> (tempfile::TempDir, Value) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("s.crn");
    std::fs::write(&src, source).expect("write source");
    let axes = info_json_at(&src, edition);
    let row = buildable_entry(&axes, edition).clone();
    (tmp, row)
}

/// The per-version list as `(version, refusal tag)`, in the row's order.
fn refusals(row: &Value) -> Vec<(String, String)> {
    row["reason"]["versions"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a per-version list, got {row}"))
        .iter()
        .map(|entry| {
            (
                entry["version"].as_str().expect("a version").to_owned(),
                entry["refusal"].as_str().expect("a refusal tag").to_owned(),
            )
        })
        .collect()
}

/// `(declared, line, col)` for each floor under `value`'s `key`.
fn floors(value: &Value, key: &str) -> Vec<(String, u64, u64)> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` is not an array in {value}"))
        .iter()
        .map(|floor| {
            (
                floor["declared"].as_str().expect("declared").to_owned(),
                floor["line"].as_u64().expect("line"),
                floor["col"].as_u64().expect("col"),
            )
        })
        .collect()
}

#[test]
fn a_floor_this_edition_cannot_place_says_so_for_the_edition() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.45\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        floors(&row["reason"], "unplaceable_floors"),
        [("bedrock version>=1.21.45".to_owned(), 2, 1)],
        "the `@requires` line, not the file: {row}",
    );
    assert!(
        row["reason"]["unplaceable_floors"][0]
            .get("declared_by")
            .is_none(),
        "a header floor is the file's own, and the line already says where: {row}",
    );
    assert!(
        row["reason"].get("versions").is_none(),
        "a version with nothing against it contributes no entry: {row}",
    );
}

#[test]
fn an_unplaceable_floor_does_not_hide_what_each_version_refused() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.45\n\n\
         theme t:\n\
         \x20\x20slot floor -> @pale_moss_block\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        floors(&row["reason"], "unplaceable_floors"),
        [("bedrock version>=1.21.45".to_owned(), 2, 1)],
        "{row}",
    );
    // `1.21.60` declares the block, so only the floor refuses it, and that is said above.
    assert_eq!(
        refusals(&row),
        [
            ("1.21.0".to_owned(), "lowering_refused".to_owned()),
            ("1.21.40".to_owned(), "lowering_refused".to_owned()),
        ],
        "both causes, in one run: {row}",
    );
    assert_eq!(
        strings(&row, "considered"),
        ["1.21.0", "1.21.40", "1.21.60"],
        "{row}",
    );
    assert_eq!(
        row["reason"]["versions"][0]["findings"][0]["data"]["id"], "minecraft:pale_moss_block",
        "{row}",
    );
}

/// Two floors at different heights: `1.21.0` is under both, the two after it only the second.
#[test]
fn a_floor_above_every_release_names_the_line_under_each_version() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.40\n\n\
         def cottage size=9x7:\n\
         \x20\x20requires bedrock version>=1.99\n\
         \x20\x20floor id=floor mat_slot=floor\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         site v:\n\
         \x20\x20place id=h1 use=cottage theme=t at=origin\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        refusals(&row),
        [
            ("1.21.0".to_owned(), "below_floor".to_owned()),
            ("1.21.40".to_owned(), "below_floor".to_owned()),
            ("1.21.60".to_owned(), "below_floor".to_owned()),
        ],
        "{row}",
    );
    assert_eq!(
        floors(&row["reason"]["versions"][0], "floors"),
        [
            ("bedrock version>=1.21.40".to_owned(), 2, 1),
            ("bedrock version>=1.99".to_owned(), 5, 3),
        ],
        "{row}",
    );
    for later in [1, 2] {
        assert_eq!(
            floors(&row["reason"]["versions"][later], "floors"),
            [("bedrock version>=1.99".to_owned(), 5, 3)],
            "only the floor that refuses this version: {row}",
        );
    }
    let header = &row["reason"]["versions"][0]["floors"][0];
    let inherited = &row["reason"]["versions"][0]["floors"][1];
    assert!(header.get("declared_by").is_none(), "{row}");
    assert_eq!(inherited["declared_by"]["keyword"], "def", "{row}");
    assert_eq!(inherited["declared_by"]["name"], "cottage", "{row}");
}

#[test]
fn a_floor_is_credited_only_with_the_versions_it_refuses() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("s.crn");
    std::fs::write(
        &src,
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.40\n\n\
         def cottage size=9x7:\n\
         \x20\x20requires bedrock version>=1.99\n\
         \x20\x20floor id=floor mat_slot=floor\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         site v:\n\
         \x20\x20place id=h1 use=cottage theme=t at=origin\n",
    )
    .expect("write source");
    let (_code, _stdout, stderr) = info_raw(&src, "bedrock");
    let below: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("below the"))
        .collect();
    assert_eq!(below.len(), 2, "one note per refusing floor: {stderr}");
    assert!(
        below[0]
            .ends_with("bedrock 1.21.0 is below the `bedrock version>=1.21.40` this file declares"),
        "the header floor refuses the oldest release alone, and `is` follows: {stderr}",
    );
    assert!(
        below[1].ends_with(
            "bedrock 1.21.0, 1.21.40, 1.21.60 are below the `bedrock version>=1.99` `def cottage` \
             declares"
        ),
        "{stderr}",
    );
}

#[test]
fn a_scope_that_produced_no_voxels_names_the_scope() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         struct good size=2x2\n\
         \x20\x20floor mat_slot=floor\n\n\
         struct bad\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        strings(&row["reason"], "dropped_scopes"),
        ["struct::bad"],
        "the member that produced nothing, not the versions: {row}",
    );
    assert!(
        row["reason"].get("versions").is_none(),
        "no release has anything against it of its own: {row}",
    );
}

#[test]
fn a_floor_does_not_hide_a_scope_that_produced_no_voxels() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.99\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         struct good size=2x2\n\
         \x20\x20floor mat_slot=floor\n\n\
         struct bad\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert_eq!(
        strings(&row["reason"], "dropped_scopes"),
        ["struct::bad"],
        "{row}",
    );
    assert_eq!(
        refusals(&row),
        [
            ("1.21.0".to_owned(), "below_floor".to_owned()),
            ("1.21.40".to_owned(), "below_floor".to_owned()),
            ("1.21.60".to_owned(), "below_floor".to_owned()),
        ],
        "both causes, in one run: {row}",
    );
}

#[test]
fn a_state_the_writer_cannot_carry_names_the_palette_entry() {
    let (tmp, row) = buildable_row(
        "theme t:\n\
         \x20\x20slot floor -> @oak_log[axis=x]\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        strings(&row["reason"], "states_unmapped"),
        ["minecraft:oak_log[axis=x]"],
        "the entry the writer refuses, not the versions: {row}",
    );
    assert!(row["reason"].get("versions").is_none(), "{row}");

    let (code, stdout, stderr) = info_raw(&tmp.path().join("s.crn"), "bedrock");
    assert_eq!(code, Some(0), "info reports; the build refuses: {stderr}");
    assert!(
        stdout.contains(
            "buildable targets:       Bedrock: none (1.21.0, 1.21.40, 1.21.60 all refuse)"
        ),
        "{stdout}",
    );
    let notes: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("note: no bedrock target"))
        .collect();
    assert_eq!(
        notes,
        [
            "note: no bedrock target can build this source: its writer cannot carry the states \
             of `minecraft:oak_log[axis=x]`"
        ],
        "{stderr}",
    );
}

/// The findings ride along in the shape `spec/lint` "Machine-readable payload" gives them.
#[test]
fn a_refused_lowering_names_what_each_version_refused() {
    let (_tmp, row) = buildable_row(
        "theme t:\n\
         \x20\x20slot floor -> @stonebrick\n\
         \x20\x20slot wall  -> @pale_moss_block\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        refusals(&row),
        [
            ("1.21.0".to_owned(), "lowering_refused".to_owned()),
            ("1.21.40".to_owned(), "lowering_refused".to_owned()),
            ("1.21.60".to_owned(), "lowering_refused".to_owned()),
        ],
        "{row}",
    );
    let finding = &row["reason"]["versions"][0]["findings"][0];
    assert_eq!(finding["code"], "E_UNKNOWN_ID", "{row}");
    assert_eq!(finding["data"]["id"], "minecraft:pale_moss_block", "{row}");
    assert_eq!(
        finding["data"]["registry"], "bedrock 1.21.0",
        "the registry that refused it, so two versions' findings are not read as one: {row}",
    );
    let later = &row["reason"]["versions"][1]["findings"][0];
    assert_eq!(later["data"]["id"], "minecraft:stonebrick", "{row}");
}

/// The floor refuses the oldest release, and an id the two after it.
#[test]
fn versions_refused_for_different_reasons_each_carry_their_own() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.40\n\n\
         theme t:\n\
         \x20\x20slot floor -> @stonebrick\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n",
        "bedrock",
    );
    assert!(strings(&row, "buildable").is_empty(), "{row}");
    assert_eq!(
        refusals(&row),
        [
            ("1.21.0".to_owned(), "below_floor".to_owned()),
            ("1.21.40".to_owned(), "lowering_refused".to_owned()),
            ("1.21.60".to_owned(), "lowering_refused".to_owned()),
        ],
        "{row}",
    );
    assert_eq!(
        floors(&row["reason"]["versions"][0], "floors"),
        [("bedrock version>=1.21.40".to_owned(), 2, 1)],
        "{row}",
    );
    assert_eq!(
        row["reason"]["versions"][1]["findings"][0]["code"], "E_UNKNOWN_ID",
        "{row}",
    );
}

#[test]
fn a_row_with_a_buildable_target_carries_no_reason() {
    let axes = info_json("cottage.crn", "java,bedrock");
    for edition in ["java", "bedrock"] {
        let row = buildable_entry(&axes, edition);
        assert!(
            !strings(row, "buildable").is_empty(),
            "premise: {edition} builds the cottage: {row}",
        );
        assert!(
            row.get("reason").is_none(),
            "a reason beside a non-empty list is a reason for something that did not happen: {row}",
        );
    }
}

#[test]
fn naming_the_reason_does_not_move_the_exit_code_or_the_text_row() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("s.crn");
    std::fs::write(
        &src,
        "@cairn 2026.06\n\
         @requires bedrock version>=1.99\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\n\
         struct hut size=5x5\n\
         \x20\x20floor mat_slot=floor\n",
    )
    .expect("write source");
    let (code, stdout, stderr) = info_raw(&src, "bedrock");
    assert_eq!(code, Some(0), "info reports; the build refuses: {stderr}");
    assert!(
        stdout.contains(
            "buildable targets:       Bedrock: none (1.21.0, 1.21.40, 1.21.60 all refuse)"
        ),
        "the text row is unchanged: {stdout}",
    );
}

/// A source whose only material exists on one edition, written into `dir`.
fn one_slot_source(dir: &std::path::Path, name: &str, id: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "@cairn 2026.06\n\n\
             theme t:\n\
             \x20\x20slot floor -> @{id}\n\
             \x20\x20slot wall  -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\
             \x20\x20walls class=outer mat_slot=wall height=3\n\
             \x20\x20door  side=front at=center\n"
        ),
    )
    .expect("write probe source");
    path
}

/// Assert that a build of `path` fails on `id`, named at `at`, as unknown to that target.
fn assert_build_refuses_the_id(
    path: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
    id: &str,
    at: (u64, u64),
) {
    let run = compile(path, edition, target, out, None);
    let stderr = stderr_of(&run);
    assert_eq!(
        run.status.code(),
        Some(1),
        "premise: the build must fail; stderr={stderr}"
    );
    assert_stream(&stderr, &[(at.0, at.1, "error", "E_UNKNOWN_ID")]);
    assert!(
        stderr.contains(id),
        "premise: it must fail because {edition} {target} has no `{id}`; stderr={stderr}",
    );
}

#[test]
fn a_java_only_block_is_not_reported_as_bedrock_portable() {
    // `minecraft:oak_sign` is Bedrock's `standing_sign`, and has no properties to translate.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");

    assert_build_refuses_the_id(
        &src,
        "bedrock",
        "1.21.60",
        &tmp.path().join("out"),
        "minecraft:oak_sign",
        (4, 17),
    );

    let axes = info_json_at(&src, "java,bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        1,
        "the Java-only floor material must count as unsupported on Bedrock, got row {bedrock}",
    );
    let java = portability_entry(&axes, "java");
    assert_eq!(
        as_u64(java, "unsupported"),
        0,
        "Java declares this block; its row must stay clean, got {java}",
    );
}

#[test]
fn a_bedrock_only_block_is_not_reported_as_java_portable() {
    // Java is the base edition for states, not ids: a theme can put a Bedrock spelling in Java.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "bedrockonly.crn", "standing_sign");

    assert_build_refuses_the_id(
        &src,
        "java",
        "1.21.4",
        &tmp.path().join("out"),
        "minecraft:standing_sign",
        (4, 17),
    );

    let axes = info_json_at(&src, "java,bedrock");
    let java = portability_entry(&axes, "java");
    assert_eq!(
        as_u64(java, "unsupported"),
        1,
        "the Bedrock-only floor material must count as unsupported on Java, got row {java}",
    );
    assert_eq!(
        as_u64(portability_entry(&axes, "bedrock"), "unsupported"),
        0,
        "Bedrock declares this block; its row must stay clean",
    );
}

#[test]
fn a_block_only_part_of_the_range_declares_stays_portable() {
    // `stone_bricks` is `stonebrick` on Bedrock 1.21.0, and `info` lowers the default mapping.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "renamed.crn", "floor.stone.smooth");
    for target in ["1.21.0", "1.21.40", "1.21.60"] {
        let run = compile(&src, "bedrock", target, &tmp.path().join(target), None);
        let stderr = stderr_of(&run);
        assert_eq!(
            run.status.code(),
            Some(0),
            "premise: the material must build on every supported Bedrock target; stderr={stderr}",
        );
    }

    let axes = info_json_at(&src, "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        0,
        "a block the edition has, under whichever spelling the target uses, is not unsupported; \
         got row {bedrock}",
    );
}

/// Every stdout line `cairn info` prints.
fn stdout_rows(stdout: &str) -> Vec<&str> {
    stdout.lines().filter(|line| !line.is_empty()).collect()
}

#[test]
fn the_unsupported_figure_names_the_entry_and_the_reason() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");
    let (code, stdout, stderr) = info_raw(&src, "bedrock");
    assert_eq!(code, Some(0), "info reports; it does not refuse: {stderr}");
    assert!(
        stdout.contains("unsupported: 1"),
        "premise: the figure is the thing being explained, got: {stdout}",
    );
    assert!(
        stderr.contains("minecraft:oak_sign"),
        "the entry behind the figure must be named, got: {stderr}",
    );
    assert!(
        stderr.contains("no supported version of this edition declares that id"),
        "and the reason with it, since the three have three different repairs, got: {stderr}",
    );
    assert!(
        stderr.contains("it spells the block `minecraft:standing_sign`"),
        "the reason must end on what this edition calls the block, got: {stderr}",
    );
}

#[test]
fn naming_the_entry_does_not_move_the_rows() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let clean = one_slot_source(tmp.path(), "clean.crn", "cobblestone");
    let named = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");
    let (_, clean_out, _) = info_raw(&clean, "bedrock");
    let (_, named_out, named_err) = info_raw(&named, "bedrock");
    let labels: Vec<String> = stdout_rows(&clean_out)
        .iter()
        .map(|row| row.split(':').next().expect("a label").to_owned())
        .collect();
    assert_eq!(
        labels,
        [
            "registry compatibility",
            "edition portability",
            "buildable targets",
            "intended targets",
            "semantic-sensitive",
        ],
        "premise: these are the rows, got: {clean_out}",
    );
    assert_eq!(
        stdout_rows(&named_out).len(),
        labels.len(),
        "naming an entry must not add a row, got: {named_out}",
    );
    assert!(
        !named_out.contains("oak_sign"),
        "the id belongs on stderr with the other notes, got: {named_out}",
    );
    assert!(
        named_err.contains("oak_sign"),
        "premise: it is on stderr, got: {named_err}",
    );
}

#[test]
fn an_entry_is_named_even_when_no_target_was_weighed() {
    // A floor above every version skips them all, leaving the portability count the only report.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "floored.crn", "oak_sign");
    let body = std::fs::read_to_string(&src).expect("read the probe source back");
    let floored = body.replace(
        "@cairn 2026.06\n",
        "@cairn 2026.06\n@requires version>=1.99\n",
    );
    assert_ne!(floored, body, "the floor must actually have been added");
    std::fs::write(&src, floored).expect("write the floored source");
    let (code, stdout, stderr) = info_raw(&src, "bedrock");
    assert_eq!(code, Some(0), "stderr={stderr}");
    assert!(
        stdout.contains("unsupported: 1"),
        "premise: the entry is unsupported, got: {stdout}",
    );
    // Premise: the floor skipped every version, so nothing else named it.
    assert_stream(&stderr, &[]);
    assert!(
        stderr.contains("minecraft:oak_sign"),
        "the count is the only report left, so it has to name the entry: {stderr}",
    );
}

#[test]
fn the_json_row_names_exactly_what_its_count_counts() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");
    let axes = info_json_at(&src, "java,bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    let entries = bedrock["unsupported_entries"]
        .as_array()
        .expect("unsupported_entries is a JSON array");
    assert_eq!(
        entries.len() as u64,
        bedrock["unsupported"].as_u64().expect("a count"),
        "the list and the figure count the same entries, got: {bedrock}",
    );
    assert_eq!(entries[0]["id"], "minecraft:oak_sign");
    assert_eq!(entries[0]["reason"], "absent_from_edition");
    let aliases = entries[0]["aliases"]
        .as_array()
        .expect("the absent reason carries an aliases key");
    let alias = aliases
        .first()
        .and_then(serde_json::Value::as_str)
        .expect("Bedrock has this block under another name");
    let (_, _, stderr) = info_raw(&src, "bedrock");
    assert!(
        stderr.contains(&format!("it spells the block `{alias}`")),
        "the note and the JSON must offer the same id, got: {stderr}",
    );
    let java = portability_entry(&axes, "java");
    assert_eq!(java["unsupported"], 0);
    assert!(
        java["unsupported_entries"]
            .as_array()
            .expect("every row carries the key")
            .is_empty(),
        "Java has the block, so its list is empty rather than absent: {java}",
    );
    assert_eq!(java["degraded"], 0);
    assert!(
        java["degraded_entries"]
            .as_array()
            .expect("every row carries the key")
            .is_empty(),
        "Java loses nothing here, so its list is empty rather than absent: {java}",
    );
}

#[test]
fn the_degraded_figure_names_the_entry_and_what_it_lost() {
    let axes = info_json("roof-hip.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    let entries = bedrock["degraded_entries"]
        .as_array()
        .expect("degraded_entries is a JSON array");
    assert_eq!(
        entries.len() as u64,
        bedrock["degraded"].as_u64().expect("a count"),
        "the list and the figure count the same entries, got: {bedrock}",
    );
    assert!(
        entries.len() > 1,
        "premise: this example degrades more than one entry, got: {bedrock}",
    );
    // One block in four states: the state combination tells the entries apart.
    let ids: std::collections::BTreeSet<&str> = entries
        .iter()
        .map(|entry| entry["id"].as_str().expect("an id"))
        .collect();
    assert_eq!(ids.len(), 1, "premise: one id, several states: {bedrock}");
    let states: Vec<&str> = entries
        .iter()
        .map(|entry| entry["states"].as_str().expect("the entry's states"))
        .collect();
    let distinct: std::collections::BTreeSet<&&str> = states.iter().collect();
    assert_eq!(
        distinct.len(),
        states.len(),
        "each entry is a different state combination: {states:?}",
    );
    for entry in entries {
        let dropped = entry["dropped"]
            .as_array()
            .expect("an entry carries what it lost");
        assert!(!dropped.is_empty(), "got: {entry}");
        for one in dropped {
            assert_eq!(one["key"], "shape", "got: {entry}");
            let value = one["value"].as_str().expect("the value that was asked for");
            assert_ne!(
                value, "straight",
                "Bedrock can express `straight`, so it is not a loss: {entry}",
            );
            assert!(
                entry["states"]
                    .as_str()
                    .expect("states")
                    .contains(&format!("shape={value}")),
                "the dropped intent is one of the entry's own states: {entry}",
            );
        }
    }
}

#[test]
fn the_degraded_notes_and_the_build_share_one_wording() {
    const HEADER: &str = "note: what `degraded: ";
    let (code, stdout, stderr) = info_raw(&examples_dir().join("roof-hip.crn"), "bedrock");
    assert_eq!(code, Some(0), "info reports; it does not refuse: {stderr}");
    let mut lines = stderr.lines();
    let header = lines
        .by_ref()
        .find(|line| line.starts_with(HEADER))
        .unwrap_or_else(|| panic!("the block is introduced by its own header: {stderr}"));
    assert!(
        header.ends_with(" counts on bedrock:"),
        "the header names the edition whose figure it explains: {stderr}",
    );
    let figure: usize = header
        .trim_start_matches(HEADER)
        .split('`')
        .next()
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("the header carries the figure: {header}"));
    assert!(
        stdout.contains(&format!("degraded: {figure}")),
        "the row and the header report one figure, got: {stdout}",
    );
    assert!(
        figure > 1,
        "premise: this example degrades more than one entry, got: {stdout}",
    );
    // Only the lines under that header: `unsupported_notes` writes the same prefix.
    let named: Vec<&str> = lines.take_while(|line| line.starts_with("  ")).collect();
    assert_eq!(
        named.len(),
        figure,
        "one line per entry, counted against the header's own figure: {stderr}",
    );
    assert_eq!(
        named[0],
        format!(
            "  note: `minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]` — {}",
            degradation_detail(&DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            }),
        ),
        "got: {stderr}",
    );
    let warnings = degraded_warnings("roof-hip.crn");
    let mut from_build: Vec<(&str, Vec<&str>)> = Vec::new();
    for warning in &warnings {
        let (entry, detail) = warning
            .strip_prefix("warning[W_INTENT_DEGRADED]: struct::roof_hip: ")
            .and_then(|rest| rest.split_once(": "))
            .unwrap_or_else(|| panic!("a warning names its entry after the scope: {warning}"));
        // A warning per dropped intent, so an entry that drops several is named on a run of lines.
        match from_build.last_mut() {
            Some((named, details)) if *named == entry => details.push(detail),
            _ => from_build.push((entry, vec![detail])),
        }
    }
    let from_build: Vec<String> = from_build
        .iter()
        .map(|(entry, details)| format!("  note: `{entry}` — {}", details.join("; ")))
        .collect();
    assert_eq!(
        named, from_build,
        "info's notes against the build's warnings"
    );
    assert!(
        !stdout.contains("spruce_stairs"),
        "the names belong on stderr with the other notes, got: {stdout}",
    );
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let clean = one_slot_source(tmp.path(), "clean.crn", "cobblestone");
    let (_, clean_out, _) = info_raw(&clean, "bedrock");
    assert_eq!(
        stdout_rows(&stdout).len(),
        stdout_rows(&clean_out).len(),
        "naming an entry must not add a row, got: {stdout}",
    );
}

#[test]
fn a_source_info_refuses_prints_no_rows_at_all() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("unresolved.crn");
    std::fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n\n",
            "theme t:\n  slot floor -> @cobblestone\n\n",
            "struct hut size=5x5\n  floor mat_slot=floor\n",
            "  walls class=outer mat_slot=nosuchslot height=3\n",
        ),
    )
    .expect("write fixture");
    let (code, stdout, stderr) = info_raw(&src, "java,bedrock");
    assert_eq!(code, Some(1), "stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "got: {stdout}");
    // Premise: the run is refused for a finding.
    assert_stream(&stderr, &[(8, 3, "error", "E_UNRESOLVED_SLOT")]);
}

#[test]
fn an_unsupported_entry_is_a_figure_not_a_gate() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");
    let out = Command::new(cargo_bin())
        .args(["info", src.to_str().unwrap(), "--editions", "bedrock"])
        .output()
        .expect("run cairn");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("unsupported: 1"),
        "the text row must carry the same figure the JSON does, got: {stdout}",
    );
    assert!(
        stdout.contains("buildable targets:       Bedrock: none"),
        "the buildable row reports too, got: {stdout}",
    );
}

#[test]
fn a_walkway_path_material_reaches_the_portability_row() {
    // Every shipped example paves with `@gravel`, which both editions have.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("walkway.crn");
    std::fs::write(
        &src,
        "@cairn 2026.06\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\
         \x20\x20slot wall  -> @cobblestone\n\n\
         def hut size=3x3:\n\
         \x20\x20floor id=f mat_slot=floor\n\
         \x20\x20walls id=w class=outer mat_slot=wall height=2\n\
         \x20\x20door  id=entry side=front at=center\n\n\
         site duo:\n\
         \x20\x20place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap=2\n\
         \x20\x20connect a.entry to b.entry path=@oak_sign\n",
    )
    .expect("write walkway source");

    assert_build_refuses_the_id(
        &src,
        "bedrock",
        "1.21.60",
        &tmp.path().join("out"),
        "minecraft:oak_sign",
        (15, 35),
    );

    let axes = info_json_at(&src, "java,bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        1,
        "the walkway's paving material must reach the Bedrock row, got {bedrock}",
    );
    assert_eq!(
        as_u64(portability_entry(&axes, "java"), "unsupported"),
        0,
        "Java has the block; only the Bedrock row should move",
    );
}
