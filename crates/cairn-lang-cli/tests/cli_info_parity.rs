//! Parity table AC pins for `cairn info --editions ...`.
//!
//! The `edition_portability` axis grew from a hard-coded
//! `degraded: 0, unsupported: 0` fill to a real per-edition classification
//! driven by `cairn-lang-formats::portability`. These tests hold the
//! contract from the CLI side end-to-end so a future refactor of the
//! dry-run wiring can't silently re-introduce the zero-fill.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;
use common::{cargo_bin, examples_dir};

fn info_json(file: &str, editions: &str) -> Value {
    info_json_at(&examples_dir().join(file), editions)
}

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
    serde_json::from_str(&stdout).expect("valid JSON")
}

/// Run `cairn info` and hand back the exit code with both streams, for the
/// cases where the command is expected to refuse.
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

/// A JSON string array as owned strings, so a failure prints the versions
/// rather than a `Value`.
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
/// `--lock` is pointed inside `out` rather than left to default. Without
/// it the lockfile lands next to the source, which for a source under
/// `examples/` means writing into the working tree — and `report_previous_target`
/// reads that file, so calls would communicate through it and become
/// order-dependent.
fn compile_accepts(
    path: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
) -> bool {
    Command::new(cargo_bin())
        .args([
            "compile",
            path.to_str().unwrap(),
            "--edition",
            edition,
            "--target",
            target,
            "--out",
            out.to_str().unwrap(),
            "--lock",
            out.join("build.crn.lock").to_str().unwrap(),
        ])
        .output()
        .expect("run cairn")
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
fn ac1_themed_tower_bedrock_reports_at_least_one_degraded_palette_entry() {
    // AC1: themed-tower interns non-`straight` stair `shape` at least
    // once (the eave with `shape=outer_left`); Bedrock has no `shape`
    // state so that palette entry counts as degraded.
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
fn ac2_cottage_bedrock_has_no_unsupported_entries() {
    // AC2: cottage carries only bare blocks + gable stairs whose
    // `shape=straight` is Bedrock's default (lossless drop). No family
    // outside the mapped set should appear.
    let axes = info_json("cottage.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        0,
        "cottage on Bedrock must not report unsupported entries",
    );
    // `degraded` is a real measurement — pin exactly whatever the current
    // roof lowering produces so a future regression that inflates it
    // (e.g. a gable-corner promotion) trips this AC on purpose.
    let degraded = as_u64(bedrock, "degraded");
    assert!(
        degraded <= 1,
        "cottage on Bedrock: unexpectedly high degraded count {degraded} — the gable roof should stay all-straight",
    );
}

#[test]
fn ac3_the_java_axis_is_pure_portable_for_a_source_java_can_build() {
    // AC3: Java is the base edition (`spec/versioning-editions`
    // "Backend = data tables"), so no state ever degrades there and a
    // palette of blocks Java declares classifies as portable throughout.
    // Pin it on the two files carrying the widest block variety.
    //
    // "Pure portable" is a property of these sources, not of the Java
    // axis: an id Java has never had counts as unsupported on Java too,
    // which `a_bedrock_only_block_is_not_reported_as_java_portable`
    // exercises below.
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

#[test]
fn info_degraded_count_matches_compile_intent_degraded_warnings() {
    // Info↔compile cross-consistency: `portability_for_bedrock` and
    // `build_mcstructure_tag` share `translate_states` as the source of
    // truth. If `cairn info` reports `degraded >= 1` on Bedrock, then
    // `cairn compile --edition bedrock` on the same file must emit at
    // least one `W_INTENT_DEGRADED` warning. Without this pin, the two
    // sides could drift silently — e.g. an internal refactor that adds
    // a degradation source to `translate_states` in the compile path but
    // not the info path (or vice versa) would let the parity table
    // undercount vs. what the writer actually degrades.
    let src_repo = examples_dir().join("themed-tower.crn");
    // 1. Info-side degraded count.
    let axes = info_json("themed-tower.crn", "bedrock");
    let degraded = as_u64(portability_entry(&axes, "bedrock"), "degraded");
    assert!(
        degraded >= 1,
        "expected themed-tower Bedrock degraded >= 1 for cross-consistency check, got {degraded}",
    );

    // 2. Compile-side warning count. Copy the source into a tempdir so
    //    the lockfile doesn't land next to the repo copy.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let copied = tmp.path().join("themed-tower.crn");
    std::fs::copy(&src_repo, &copied).expect("copy themed-tower");
    let out_dir = tempfile::TempDir::new().expect("out tempdir");
    let compile = Command::new(cargo_bin())
        .args([
            "compile",
            copied.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    assert!(
        compile.status.success(),
        "compile failed; stderr={}",
        String::from_utf8_lossy(&compile.stderr),
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    let warning_count = stderr.matches("W_INTENT_DEGRADED").count();
    assert!(
        warning_count >= 1,
        "info reported degraded >= 1 but compile emitted no W_INTENT_DEGRADED; \
         info and compile paths must share `translate_states` as source of truth; stderr={stderr}",
    );
}

#[test]
fn ac4_unknown_edition_rejected_with_exit_two() {
    // AC4: `--editions foo` fails before dry-run lowering runs (which
    // would otherwise silently produce a zero-fill row that a caller
    // couldn't distinguish from a real portable-only result).
    let path = examples_dir().join("cottage.crn");
    let out = Command::new(cargo_bin())
        .args(["info", path.to_str().unwrap(), "--editions", "foo"])
        .output()
        .expect("run cairn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for unknown edition, got {:?}; stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The message threads the offending value + the closed valid set,
    // matching the self-correction triple in `spec/versioning-editions`
    // "Fail-loud and minimum-version inference" (what is wrong / what is
    // valid / how to fix).
    assert!(
        stderr.contains("foo"),
        "stderr should name the offending value, got: {stderr}",
    );
    assert!(
        stderr.contains("java, bedrock"),
        "stderr should list valid editions, got: {stderr}",
    );
}

#[test]
fn ac5_an_edition_specific_resolve_failure_is_reported_not_counted_away() {
    // `info` gates on the edition-neutral pass, which unions slot names
    // across per-edition theme variants. A slot only one variant declares
    // therefore resolves there and fails only in the strict per-edition
    // pass the parity dry-run runs.
    //
    // Reporting nothing made the failure indistinguishable from a design
    // choice: the member that could not resolve just lowered a smaller
    // `portable` count for that edition, with `degraded: 0
    // unsupported: 0` beside it. The one command whose job is showing
    // per-edition divergence was the one that could not show it.
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

    // The edition-neutral gate sees a slot declared by one variant, so it
    // passes — this is the premise that makes the case interesting.
    let check = Command::new(cargo_bin())
        .args(["check", path])
        .output()
        .expect("run cairn");
    assert_eq!(check.status.code(), Some(0), "premise: check accepts it");

    // Bedrock cannot resolve it, and `compile` says so.
    let compile = Command::new(cargo_bin())
        .args([
            "compile",
            path,
            "--edition",
            "bedrock",
            "--out",
            tmp.path().join("out").to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    assert_eq!(
        compile.status.code(),
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
    assert!(
        stderr.contains("E_UNRESOLVED_SLOT") && stderr.contains("bedrock"),
        "the report must name the code and the edition it belongs to; stderr={stderr}",
    );
    assert!(
        info.stdout.is_empty(),
        "no portability table should be printed when a figure could not be computed",
    );
    // The per-version pass runs against the same resolution, so it
    // re-derives this finding once per target. Reporting it four times
    // would blame the versions for something that is not about them.
    assert_eq!(
        stderr.matches("E_UNRESOLVED_SLOT").count(),
        1,
        "a finding the range-wide pass already reported is not repeated per \
         target; stderr={stderr}",
    );
}
// -- buildable targets ----------------------------------------------------

/// The portability row asks of the *edition*, so a block one part of the
/// range spells differently is not missing from it. Two entries can each
/// answer yes while no single version declares both, and the report was
/// clean for a source every supported target refuses.
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

    // The two counters still answer their own question, and it is not this
    // one: both blocks exist on Bedrock, each on a different part of its
    // range.
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

    // The premise the test name rests on, asked of the command that
    // decides it rather than assumed.
    let out = tmp.path().join("out");
    for version in ["1.21.0", "1.21.40", "1.21.60"] {
        assert!(
            !compile_accepts(&src, "bedrock", version, &out.join(version)),
            "premise: bedrock {version} should refuse this source",
        );
    }

    // And the text row says it rather than leaving it to the JSON. Matched
    // as a whole line: `none` on its own is in every run's output, under
    // `semantic-sensitive`.
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
    // The reason is printed too, since nothing else in the run would show
    // it and a bare `none` is not a report.
    assert!(
        stderr.contains("E_UNKNOWN_ID") && stderr.contains("pale_moss_block"),
        "the refusing ids should reach the user: {stderr}",
    );
}

/// The intersection of the range-wide palette's id sets is empty for this
/// source, and one target builds it: with no target pinned every material
/// takes its default mapping, and `@floor.stone.smooth` is respelled at
/// 1.21.0. This is the case that separates the sound answer from the cheap
/// one.
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

/// A version is refused for an error, not for a finding. A source that
/// warns at every target still builds at every target.
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
         \x20\x20door  side=front at=center\n",
    )
    .expect("write source");

    let axes = info_json_at(&src, "bedrock");
    let entry = buildable_entry(&axes, "bedrock");
    assert_eq!(
        strings(entry, "buildable"),
        strings(entry, "considered"),
        "a door with no walls to carve into is a warning, not a refusal",
    );
}

/// Nothing shipped is unbuildable, so the row is full for every example on
/// both editions and `info` still exits 0.
#[test]
fn every_example_lists_every_supported_target() {
    for (name, _) in examples() {
        let path = examples_dir().join(&name);
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

/// The claim under the row: the set is what `compile --target` accepts.
/// Asserted against the command rather than against a second copy of the
/// rule, which is the only way the two cannot drift together.
///
/// Fixture-driven rather than corpus-driven. Every shipped example
/// declares `@requires version>=1.20`, below every supported target on
/// both editions; none has a scope that fails to lower; and every id they
/// use is in both packs' base table. So all 24 corpus pairs collapse to
/// `buildable == considered == accepted`, and a filter that reported every
/// version unconditionally would pass. Each source below refuses on a
/// different gate, and the clean one is there so a filter that refuses
/// everything fails too.
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
        // A default mapping the earliest target respells, beside the
        // respelling written out.
        (
            "mix.crn",
            "theme t:\n\
             \x20\x20slot floor -> @floor.stone.smooth\n\
             \x20\x20slot wall  -> @stonebrick\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\
             \x20\x20walls class=outer mat_slot=wall height=3\n",
        ),
        // A floor above part of the supported range. `E_VERSION_CAP` is
        // per version, so this is the axis itself.
        (
            "floor.crn",
            "@requires version>=1.21.40\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A scope that does not lower. Every code that drops one is
        // Warning severity, so the edition-neutral gate lets it through
        // and only `E_PARTIAL_BUILD` refuses.
        (
            "partial.crn",
            "theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n\n\
             struct nosize\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // A floor no edition's table can place. It is the one gate that
        // answers for every version at once — `weigh_versions` returns
        // before it lowers anything — so the row is empty for a reason
        // the loop below never reaches, and `compile --target` has to
        // agree on every version rather than on none of them.
        (
            "unorderable.crn",
            "@requires version>=24w14a\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // The same shape one edition *can* place, so the two editions
        // disagree in one run: Java orders `1.21.4` exactly and Bedrock
        // has no release by that name.
        (
            "cross_edition.crn",
            "@requires version>=1.21.4\n\n\
             theme t:\n\
             \x20\x20slot floor -> @cobblestone\n\n\
             struct hut size=5x5\n\
             \x20\x20floor mat_slot=floor\n",
        ),
        // And one that builds everywhere, so "refuse everything" fails.
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

/// Ascending release order, not the order of rows in the pack's JSON,
/// which `DataVersionTable` documents as informational. The row shows the
/// list to a reader, so the display order has to be decided here.
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

/// Write `source` to a fresh temp dir and read its `buildable_targets` row
/// for `edition`, with the temp dir kept alive for the caller.
fn buildable_row(source: &str, edition: &str) -> (tempfile::TempDir, Value) {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("s.crn");
    std::fs::write(&src, source).expect("write source");
    let axes = info_json_at(&src, edition);
    let row = buildable_entry(&axes, edition).clone();
    (tmp, row)
}

/// The per-version list as `(version, refusal tag)`, in the row's order.
///
/// The version travels with the tag because the list is a subsequence of
/// `considered` rather than a parallel array: a run whose answer is
/// edition-wide leaves versions out of it, so a test reading tags by
/// position alone would pass against the wrong releases.
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

/// An empty `buildable` had four causes and named none of them: a consumer
/// reading the JSON and discarding stderr saw `[]` and could not tell
/// which of four different things to edit.
///
/// This one is the edition's rather than any release's — a floor naming no
/// release of the edition can be weighed against none of them — so it is
/// reported once, beside the per-version list rather than inside it.
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
    // Nothing else is wrong with this source, so the floor is the whole
    // answer and no release has one of its own.
    assert!(
        row["reason"].get("versions").is_none(),
        "a version with nothing against it contributes no entry: {row}",
    );
}

/// The floor an edition cannot place answers for the edition, but it is
/// not the only thing a file can have wrong, and the release that also
/// refuses an id has to say so in the same run. Otherwise the author
/// repairs the floor, runs again, and meets the ids for the first time —
/// the loop this row exists to close.
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
    // `1.21.60` declares the block and is left out: the floor is the only
    // thing against it, and that is already said above. This is why the
    // list joins on `version` rather than lining up with `considered`.
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

/// The floor that *is* placeable, and refuses every release. Different
/// news and a different repair — raise the target, or build the other
/// edition — so it reads as a different tag.
///
/// Two floors at different heights, because that is the only shape in
/// which a version's `floors` differs from the edition's: `1.21.0` is
/// under both and the two after it only under the second. With one floor
/// the per-version list and the edition-wide set are the same bytes, and
/// a test using it would pass against either.
#[test]
fn a_floor_above_every_release_names_the_line_under_each_version() {
    let (_tmp, row) = buildable_row(
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.40\n\n\
         def cottage class=house size=9x7:\n\
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
    // The oldest release is under both lines; the two after it are under
    // the def's alone, and being sent to a line that does not refuse them
    // is a repair that changes nothing.
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
    // A floor a build inherited from a part is repaired in that part, and
    // "this file requires ..." is the reading the composite rule exists to
    // correct. The header floor above carries no `declared_by` at all.
    let header = &row["reason"]["versions"][0]["floors"][0];
    let inherited = &row["reason"]["versions"][0]["floors"][1];
    assert!(header.get("declared_by").is_none(), "{row}");
    assert_eq!(inherited["declared_by"]["keyword"], "def", "{row}");
    assert_eq!(inherited["declared_by"]["name"], "cottage", "{row}");
}

/// The same two floors as prose. `report_version_notes` used to list every
/// version some floor refused under *each* floor, so the header line read
/// "1.21.40 is below `version>=1.21.40`" — which is not true of the
/// version, and sends the reader to a line that does not refuse it.
#[test]
fn a_floor_is_credited_only_with_the_versions_it_refuses() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("s.crn");
    std::fs::write(
        &src,
        "@cairn 2026.06\n\
         @requires bedrock version>=1.21.40\n\n\
         def cottage class=house size=9x7:\n\
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

/// The third cause: a scope the source declares produced no voxels, so
/// every version is refused before its id table is consulted. The repair
/// is the member, and nothing about the versions would change it — so it
/// is the edition's answer rather than a tag repeated once per release.
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

/// A floor used to eat the dropped scope: both refuse every version, the
/// payload had one slot per version, and the floor arm was reached first.
/// So the author repaired the `@requires` line and only then learned about
/// the member — the same one-cause-per-run loop as the unplaceable floor.
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

/// The fourth: the pinned lowering raised errors, which is usually a
/// material or an id and differs per version. The findings ride along in
/// the shape `spec/lint` "Machine-readable payload" gives them, because
/// naming the version without naming what it refused leaves the reader
/// exactly where the bare `[]` did.
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
    // The id `1.21.0` refuses is not the id `1.21.40` refuses. One tag for
    // the edition would have had to pick one of them to report.
    let later = &row["reason"]["versions"][1]["findings"][0];
    assert_eq!(later["data"]["id"], "minecraft:stonebrick", "{row}");
}

/// Two causes in one run, which is the whole reason the per-version answer
/// is per version: the floor refuses the oldest release and an id refuses
/// the two after it. A single tag for the edition would report one and
/// hide the other, and a reader who fixed only that one would still have
/// no buildable target.
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

/// The key is absent on the ordinary run, so the addition costs a
/// consumer of a report that has a buildable target nothing at all.
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

/// `info` reports and does not refuse, whatever the row says. The reason
/// is a payload on a successful run, not a diagnostic — naming the cause
/// is not the same as taking a position on it.
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

/// Every `.crn` under `examples/`, as `(file name, source)`, with a guard
/// against a loop over nothing.
fn examples() -> Vec<(String, String)> {
    let entries = std::fs::read_dir(examples_dir()).expect("read examples dir");
    let mut found: Vec<(String, String)> = entries
        .map(|entry| entry.expect("read an entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("read example");
            (
                path.file_name().expect("named").to_string_lossy().into(),
                source,
            )
        })
        .collect();
    found.sort();
    assert!(
        found.len() >= 5,
        "found only {} examples, which is not the shipped set",
        found.len(),
    );
    found
}

/// A source whose only material exists on one edition, written out to
/// `tmp` and returned as its path.
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

/// Build `path` for one target, returning `(exit code, stderr)`.
fn compile_for(
    path: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
) -> (Option<i32>, String) {
    let result = Command::new(cargo_bin())
        .args([
            "compile",
            path.to_str().unwrap(),
            "--edition",
            edition,
            "--target",
            target,
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    (
        result.status.code(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    )
}

/// Assert that a build of `path` refuses `id` as unknown to that target.
///
/// The premise these tests rest on is not "the build failed" — any
/// diagnostic exits 1, including ones about something else entirely — but
/// "the build failed *on this id*". Naming the code and the id is what
/// makes the portability row a claim `compile` actually contradicts.
fn assert_build_refuses_the_id(
    path: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
    id: &str,
) {
    let (code, stderr) = compile_for(path, edition, target, out);
    assert_eq!(
        code,
        Some(1),
        "premise: the build must fail; stderr={stderr}"
    );
    assert!(
        stderr.contains("E_UNKNOWN_ID") && stderr.contains(id),
        "premise: it must fail because {edition} {target} has no `{id}`; stderr={stderr}",
    );
}

#[test]
fn a_java_only_block_is_not_reported_as_bedrock_portable() {
    // `minecraft:oak_sign` is a Java spelling; Bedrock calls that block
    // `standing_sign` and has never had this id. It carries no properties,
    // so `translate_states` returns a clean translation for it and the
    // Bedrock row read `portable` — the one command whose job is answering
    // "will this port?" answering yes about a block the edition does not
    // have.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "javaonly.crn", "oak_sign");

    assert_build_refuses_the_id(
        &src,
        "bedrock",
        "1.21.60",
        &tmp.path().join("out"),
        "minecraft:oak_sign",
    );

    let axes = info_json_at(&src, "java,bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        1,
        "the Java-only floor material must count as unsupported on Bedrock, got row {bedrock}",
    );
    // Java has the block, so its own row stays clean — the count is a
    // statement about the edition, not a blanket refusal of the source.
    let java = portability_entry(&axes, "java");
    assert_eq!(
        as_u64(java, "unsupported"),
        0,
        "Java declares this block; its row must stay clean, got {java}",
    );
}

#[test]
fn a_bedrock_only_block_is_not_reported_as_java_portable() {
    // The mirror. Java is the base edition for *states*, which is why it
    // never degrades — it is not a base edition for ids, and a theme
    // resolving a slot to a Bedrock spelling puts one into a Java palette
    // exactly the way the case above puts a Java spelling into a Bedrock
    // one.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "bedrockonly.crn", "standing_sign");

    assert_build_refuses_the_id(
        &src,
        "java",
        "1.21.4",
        &tmp.path().join("out"),
        "minecraft:standing_sign",
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
    // `stone_bricks` does not exist on Bedrock 1.21.0 — it is `stonebrick`
    // there — and the pack carries a per-version override so each target
    // gets its own spelling. `info` pins no version and therefore lowers
    // the default one, so an id axis asking "does *every* supported
    // version declare this" would report the shipped mapping as
    // unsupported on an edition that builds it cleanly on all three
    // targets. The axis asks "does the edition have the block at all".
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = one_slot_source(tmp.path(), "renamed.crn", "floor.stone.smooth");
    for target in ["1.21.0", "1.21.40", "1.21.60"] {
        let (code, stderr) = compile_for(&src, "bedrock", target, &tmp.path().join(target));
        assert_eq!(
            code,
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

/// Every stdout line `cairn info` prints, so a test can say the rows did
/// not move rather than that one substring survived.
fn stdout_rows(stdout: &str) -> Vec<&str> {
    stdout.lines().filter(|line| !line.is_empty()).collect()
}

#[test]
fn the_unsupported_figure_names_the_entry_and_the_reason() {
    // `unsupported: 1` is one integer over three failures with three
    // different repairs. Everything needed to say which entry and which
    // failure is in hand where the count is incremented, so the command
    // says it.
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
    // This fixture's block is one Bedrock has under another name, so the
    // reason ends on the edit rather than on the dead end: the row is
    // there to be acted on.
    assert!(
        stderr.contains("it spells the block `minecraft:standing_sign`"),
        "the reason must end on what this edition calls the block, got: {stderr}",
    );
}

#[test]
fn naming_the_entry_does_not_move_the_rows() {
    // The rows are the stdout contract — `--format json`'s text twin, and
    // what a reader greps. The names go to stderr beside every other
    // `note:` this command prints, so nothing that consumes the rows has
    // to change.
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
    // The `buildable targets` row already prints `E_UNKNOWN_ID` per
    // version, which names an absent id in the ordinary case — but only
    // when a version was lowered at all. A floor above every supported
    // version skips all of them, and then the portability count is the
    // only thing left saying anything is wrong. It has to say what.
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
    assert!(
        !stderr.contains("E_UNKNOWN_ID"),
        "premise: the floor skipped every version, so nothing else named it: {stderr}",
    );
    assert!(
        stderr.contains("minecraft:oak_sign"),
        "the count is the only report left, so it has to name the entry: {stderr}",
    );
}

#[test]
fn the_json_row_names_exactly_what_its_count_counts() {
    // A tool reading `--format json` gets the same answer the text does.
    // The list is asserted against the row's own figure rather than
    // against a literal, so the two cannot be right about different things.
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
    // `aliases` is the flattened payload of that reason, and it holds the
    // same strings the stderr note offers — one answer rendered twice, not
    // two lookups that happen to agree.
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
}

#[test]
fn the_degraded_figure_names_the_entry_and_what_it_lost() {
    // The other integer over entries the command can name. `roof-hip` is
    // a shipped example reporting `degraded: 4`, and before this the only
    // way to find the four was to compile and read `W_INTENT_DEGRADED` —
    // which is the run `info` exists to be read before.
    let axes = info_json("roof-hip.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    let entries = bedrock["degraded_entries"]
        .as_array()
        .expect("degraded_entries is a JSON array");
    // Against the row's own figure rather than a literal, so the two
    // cannot be right about different things.
    assert_eq!(
        entries.len() as u64,
        bedrock["degraded"].as_u64().expect("a count"),
        "the list and the figure count the same entries, got: {bedrock}",
    );
    assert!(
        entries.len() > 1,
        "premise: this example degrades more than one entry, got: {bedrock}",
    );
    // Every one of them is the same block, which is the case a list keyed
    // by id alone could not report: it is the state combination that
    // degrades, so that is what tells the entries apart.
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
    // The pieces, not a sentence: a consumer reads which property was
    // dropped without parsing English out of the field beside it.
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
fn the_degraded_notes_say_what_the_build_would_say() {
    // Two moments, one reader, one wording: the note here and the build's
    // `W_INTENT_DEGRADED` come off the same function, so `info` cannot
    // start describing a loss differently from the compile that hits it.
    let (code, stdout, stderr) = info_raw(&examples_dir().join("roof-hip.crn"), "bedrock");
    assert_eq!(code, Some(0), "info reports; it does not refuse: {stderr}");
    assert!(
        stdout.contains("degraded: 4"),
        "premise: the figure is the thing being explained, got: {stdout}",
    );
    let named: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("  note: `minecraft:"))
        .collect();
    assert_eq!(
        named.len(),
        4,
        "one line per entry, so the lines can be counted against the figure: {stderr}",
    );
    assert!(
        named[0].contains("shape=outer_left has no Bedrock state"),
        "the sentence the build prints for the same entry: {stderr}",
    );
    assert!(
        named[0].contains("minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]"),
        "the line's subject is the palette entry, states and all: {stderr}",
    );
    // The header carries the figure it is explaining, the way the
    // `unsupported` block does.
    assert!(
        stderr.contains("note: what `degraded: 4` counts on bedrock:"),
        "got: {stderr}",
    );
    // And none of it reaches stdout, where the rows are the contract.
    assert!(
        !stdout.contains("spruce_stairs"),
        "the names belong on stderr with the other notes, got: {stdout}",
    );
    assert_eq!(
        stdout_rows(&stdout).len(),
        5,
        "naming an entry must not add a row, got: {stdout}",
    );
}

#[test]
fn a_source_info_refuses_prints_no_rows_at_all() {
    // The four rows are a guarantee about a run that finishes, not about
    // every invocation: a finding the strict per-edition pass raises
    // returns before `print_text`, and stdout is then empty rather than
    // short one row. The spec says so, and the two tests either side of
    // this one compare sources that parse and resolve, so neither could
    // notice.
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
    assert!(
        stderr.contains("E_UNRESOLVED_SLOT"),
        "premise: the run is refused for a finding, got: {stderr}",
    );
}

#[test]
fn an_unsupported_entry_is_a_figure_not_a_gate() {
    // `info` reports; it does not refuse. The example output in the
    // three-answers section of `spec/versioning-editions` carries
    // `unsupported: 1`, and a caller that wants the build to fail runs the
    // build. Exiting non-zero here would also make the count unreadable
    // from a shell pipeline that checks status first.
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
    // The buildable row does not refuse either, and for this source the two
    // rows agree from different directions: Bedrock has no `oak_sign` at
    // all, so the entry is unsupported *and* no target builds it.
    assert!(
        stdout.contains("buildable targets:       Bedrock: none"),
        "the buildable row reports too, got: {stdout}",
    );
}

#[test]
fn a_walkway_path_material_reaches_the_portability_row() {
    // A walkway's `path=@ID` resolves through the registry exactly the way
    // a member's `mat_slot=` does, and its strip is lowered into its own
    // `BlockArray`. Whether that array reaches the counters is a fact about
    // the lowering, not about the fold — `non_air_entries` never looks at a
    // key — so only a source that really lays a walkway can establish it.
    //
    // Every shipped example paves with `@gravel`, which both editions have,
    // so nothing else here could ever raise the count.
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
