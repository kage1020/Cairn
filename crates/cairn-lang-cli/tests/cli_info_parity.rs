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

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

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
    // AC3: Java is the base edition (spec versioning-editions §10.3), so
    // no state ever degrades there and a palette of blocks Java declares
    // classifies as portable throughout. Pin it on the two files carrying
    // the widest block variety.
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
    // matching the spec §10.4 self-correction triple (what is wrong /
    // what is valid / how to fix).
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

#[test]
fn an_unsupported_entry_is_a_figure_not_a_gate() {
    // `info` reports; it does not refuse. Spec §10.5's own example output
    // carries `unsupported: 1`, and a caller that wants the build to fail
    // runs the build. Exiting non-zero here would also make the count
    // unreadable from a shell pipeline that checks status first.
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
