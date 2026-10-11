//! `cairn info --editions`: each edition's portability figures and
//! buildable targets are what `cairn compile` does with the same source
//! for that edition. An entry it degrades or refuses is counted and named,
//! a target it accepts is listed, and a refused version carries the reason
//! the compile would give.

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

/// Run `cairn info --format json` and hand back the document, after
/// checking that each per-edition axis has one entry per distinct edition
/// in `editions`.
///
/// [`buildable_entry`] and [`portability_entry`] find an entry by name and
/// cannot see a second one.
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
/// `--lock` goes inside `out`: the default lands beside the source, in
/// `examples/` for most callers, and `report_previous_target` reading it
/// back would make calls order-dependent.
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
    // themed-tower interns non-`straight` stair `shape` at least
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
fn cottage_bedrock_has_no_unsupported_entries() {
    // cottage carries only bare blocks + gable stairs whose
    // `shape=straight` is Bedrock's default (lossless drop). No family
    // outside the mapped set should appear.
    let axes = info_json("cottage.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        0,
        "cottage on Bedrock must not report unsupported entries",
    );
    // A real measurement: a gable-corner promotion, say, would inflate it.
    let degraded = as_u64(bedrock, "degraded");
    assert!(
        degraded <= 1,
        "cottage on Bedrock: unexpectedly high degraded count {degraded} — the gable roof should stay all-straight",
    );
}

#[test]
fn the_java_axis_is_pure_portable_for_a_source_java_can_build() {
    // Java is the base edition (`spec/versioning-editions`
    // "Backend = data tables"), so no state degrades there. "Pure portable"
    // is still a property of these sources: an id Java lacks is
    // unsupported on Java too
    // (`a_bedrock_only_block_is_not_reported_as_java_portable`).
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

/// Compile a copy of `examples/<example>` for Bedrock in a temp dir, so the
/// lockfile lands beside the copy rather than in `examples/`, and return
/// the `W_INTENT_DEGRADED` lines it printed.
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
    // `portability_for_bedrock` and `build_mcstructure_tag` share
    // `translate_states`, so a degradation added to one path alone shows
    // up here.
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
fn each_degraded_entry_gets_its_own_warning_naming_the_entry_info_names() {
    // `roof-hip`'s four degraded entries are four states of one id, so each
    // warning names the Java state, as `info`'s note does.
    let axes = info_json("roof-hip.crn", "bedrock");
    let entries = portability_entry(&axes, "bedrock")["degraded_entries"]
        .as_array()
        .expect("degraded_entries is a JSON array");
    assert!(entries.len() > 1, "premise: several entries degrade");

    let warnings = degraded_warnings("roof-hip.crn");
    let distinct: std::collections::BTreeSet<&String> = warnings.iter().collect();
    assert_eq!(
        distinct.len(),
        warnings.len(),
        "a warning repeats: {warnings:?}"
    );
    assert_eq!(
        warnings.len(),
        entries.len(),
        "one warning per entry: {warnings:?}"
    );
    for entry in entries {
        let named = format!(
            "`{}[{}]`",
            entry["id"].as_str().expect("an id"),
            entry["states"].as_str().expect("states"),
        );
        assert_eq!(
            warnings.iter().filter(|line| line.contains(&named)).count(),
            1,
            "exactly one warning names {named}: {warnings:?}",
        );
    }
}

#[test]
fn an_edition_specific_resolve_failure_is_reported_not_counted_away() {
    // `info` gates on the edition-neutral pass, which unions slot names
    // across per-edition theme variants, so a slot only one variant
    // declares fails only in the parity dry-run's strict pass. Unreported,
    // it would read as a smaller `portable` count beside `degraded: 0
    // unsupported: 0`.
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
/// range spells differently is not missing from it: two entries can each
/// answer yes while no single version declares both.
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

    // The counters answer their own question: both blocks exist on
    // Bedrock, on different parts of its range.
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

    // Matched as a whole line: `none` alone is in every run's output, under
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
    // A bare `none` is not a report.
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

/// The range-wide palette's id sets never overlap for this source, yet one
/// target builds it: unpinned, every material takes its default mapping,
/// and `@floor.stone.smooth` is respelled at 1.21.0.
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

/// The claim under the row: the set is what `compile --target` accepts,
/// asserted against the command rather than a second copy of the rule.
///
/// Fixtures, not the corpus: every shipped example builds at every
/// supported target, so a filter reporting every version would pass there.
/// Each source below refuses on a different gate, and the clean one fails
/// a filter that refuses everything.
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
        // A floor no edition's table can place answers for every version
        // at once (`weigh_versions` returns before it lowers anything), so
        // `compile --target` has to agree on every version.
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

/// An empty `buildable` has four causes, and a consumer reading the JSON
/// without stderr has to tell which to edit.
///
/// This one is the edition's rather than any release's (a floor naming no
/// release of the edition is weighed against none of them), so it is
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

/// The release that also refuses an id says so in the same run as the
/// unplaceable floor, not after the floor is repaired.
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

/// The floor that *is* placeable, and refuses every release: a different
/// repair (raise the target, or build the other edition), so a different
/// tag.
///
/// Two floors at different heights, the only shape in which a version's
/// `floors` differs from the edition's: `1.21.0` is under both and the two
/// after it only under the second.
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
    // An inherited floor is repaired in the part that declares it. The
    // header floor carries no `declared_by`.
    let header = &row["reason"]["versions"][0]["floors"][0];
    let inherited = &row["reason"]["versions"][0]["floors"][1];
    assert!(header.get("declared_by").is_none(), "{row}");
    assert_eq!(inherited["declared_by"]["keyword"], "def", "{row}");
    assert_eq!(inherited["declared_by"]["name"], "cottage", "{row}");
}

/// The same two floors as prose. Listing every version some floor refused
/// under *each* floor would have the header line read "1.21.40 is below
/// `version>=1.21.40`" — which is not true of the version, and sends the
/// reader to a line that does not refuse it.
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

/// A floor and a dropped scope both refuse every version. With one slot
/// per version and the floor reported first, the author would repair the
/// `@requires` line and only then learn about the member — the same
/// one-cause-per-run loop as the unplaceable floor.
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

/// The fourth: the pinned lowering raised errors, usually a material or an
/// id, differing per version. The findings ride along in the shape
/// `spec/lint` "Machine-readable payload" gives them.
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
    // The id `1.21.0` refuses is not the id `1.21.40` refuses, so one tag
    // for the edition would have to pick one.
    let later = &row["reason"]["versions"][1]["findings"][0];
    assert_eq!(later["data"]["id"], "minecraft:stonebrick", "{row}");
}

/// Two causes in one run, which is why the answer is per version: the
/// floor refuses the oldest release and an id the two after it.
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

/// Assert that a build of `path` refuses `id`, named at `at`, as unknown to
/// that target.
///
/// Any diagnostic exits 1, so the premise is that the build fails *on this
/// id*.
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
    // `minecraft:oak_sign` is a Java spelling; Bedrock calls the block
    // `standing_sign`. It carries no properties, so `translate_states`
    // translates it cleanly and only the id check keeps it out of
    // `portable`.
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
    // Java is the base edition for *states*, not for ids: a theme resolving
    // a slot to a Bedrock spelling puts one into a Java palette.
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
    // `stone_bricks` is `stonebrick` on Bedrock 1.21.0, and a per-version
    // override gives each target its spelling. `info` lowers the default
    // mapping, so the axis asks whether the edition has the block at all,
    // not whether every version declares that spelling.
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

/// Every stdout line `cairn info` prints, so a test can say the rows did
/// not move rather than that one substring survived.
fn stdout_rows(stdout: &str) -> Vec<&str> {
    stdout.lines().filter(|line| !line.is_empty()).collect()
}

#[test]
fn the_unsupported_figure_names_the_entry_and_the_reason() {
    // `unsupported: 1` is one integer over three failures with three
    // different repairs.
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
    // The rows are the stdout contract, so the names go to stderr beside
    // every other `note:`.
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
    // `buildable targets` names an absent id only for a version it
    // lowers. A floor above every version skips them all, leaving the
    // portability count as the only report.
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
    // Against the row's own figure rather than a literal, so the two
    // cannot be right about different things.
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
    // The same of the figure beside it, so the two halves of the additive
    // claim fail together: a `skip_serializing_if` added to one list would
    // otherwise break its consumers with the other list's test still green.
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
    // Without the names, finding `roof-hip`'s four means compiling and
    // reading `W_INTENT_DEGRADED`, the run `info` exists to be read before.
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
fn the_degraded_notes_and_the_build_share_one_wording() {
    // The note and the build's `W_INTENT_DEGRADED` come off one function,
    // checked against it rather than a literal.
    const HEADER: &str = "note: what `degraded: ";
    let (code, stdout, stderr) = info_raw(&examples_dir().join("roof-hip.crn"), "bedrock");
    assert_eq!(code, Some(0), "info reports; it does not refuse: {stderr}");
    // The header carries the figure it is explaining, the way the
    // `unsupported` block does, and every count below is read off it
    // rather than written twice.
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
    // Only the lines under that header: `unsupported_notes` writes the
    // same `  note: ` prefix into the same stream from the same loop, so
    // counting across the whole of stderr would let three degraded beside
    // one unsupported pass for four.
    let named: Vec<&str> = lines.take_while(|line| line.starts_with("  ")).collect();
    assert_eq!(
        named.len(),
        figure,
        "one line per entry, counted against the header's own figure: {stderr}",
    );
    // The whole line, against the writer: `degradation_detail` is where
    // the clause lives, and the build frames that same clause with the
    // block named.
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
    // And none of it reaches stdout, where the rows are the contract. The
    // row count comes off a clean run rather than a literal, so the two
    // cannot be right about different numbers of rows.
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
    // The four rows are a guarantee about a run that finishes: a finding
    // the strict per-edition pass raises returns before `print_text`, so
    // stdout is empty rather than short one row.
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
    // `info` reports; it does not refuse. The example output in the
    // three-answers section of `spec/versioning-editions` carries
    // `unsupported: 1`, and a caller that wants the build to fail runs the
    // build.
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
    // A walkway's strip is lowered into its own `BlockArray`, and
    // `non_air_entries` never looks at a key, so only a source that lays a
    // walkway shows the strip reaching the counters. Every shipped example
    // paves with `@gravel`, which both editions have.
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
