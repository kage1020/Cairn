//! `--format json` writes one JSON document to stdout, for every input.
//!
//! The flag advertises a machine-readable product. It delivered one for
//! every source except the ones that fail — where stdout was empty and the
//! reason went to stderr as prose — so a consumer parsing stdout saw
//! nothing and had to guess from the exit code. These tests hold the
//! contract on the paths that used to fall through it.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

mod common;
use common::{cairn, examples_dir};

/// A source no lexer pass will take: `%` is in no token.
fn unparsable(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("bad.crn");
    fs::write(&path, "struct s size=3x3\n  floor a=%\n").expect("write");
    path
}

/// A source that parses and then fails the check pass.
fn unresolved(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("unresolved.crn");
    fs::write(
        &path,
        "theme t:\n  slot floor -> @oak_planks\n\nstruct s size=3x3\n  floor mat_slot=missing\n",
    )
    .expect("write");
    path
}

#[test]
fn check_json_renders_a_parse_failure_as_a_diagnostic() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("check", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let array = parsed.as_array().expect("check emits an array");
    assert_eq!(array.len(), 1, "one failure, one diagnostic: {stdout}");
    let d = &array[0];
    assert_eq!(d["code"], "E_PARSE");
    assert_eq!(d["severity"], "error");
    assert_eq!(d["line"], 2, "the failure is on the second line: {stdout}");
    assert!(
        d["primary"].as_str().is_some_and(|s| !s.is_empty()),
        "the diagnostic should say what is wrong: {stdout}",
    );
}

#[test]
fn check_text_renders_a_parse_failure_like_every_other_finding() {
    // The text form used to be a bare `error: file:pos: message` with no
    // code — the one finding a reader could not look up, and the one a
    // grep for `error[E_` did not match.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("check", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.is_empty(),
        "text diagnostics stay off stdout: {stdout}"
    );
    assert!(
        stderr.contains("error[E_PARSE]:"),
        "the failure should carry its code: {stderr}",
    );
    assert!(
        stderr.contains(&format!("{}:2:", path.display())),
        "and its position: {stderr}",
    );
}

#[test]
fn info_json_renders_a_parse_failure_as_a_document_of_its_own() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    // Distinguishable from the report by its keys: a report has axes and no
    // `diagnostics`, this has `diagnostics` and no axes.
    assert!(
        parsed.get("registry_compat").is_none(),
        "a failure document is not a report: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
}

#[test]
fn info_json_renders_a_check_failure_the_same_way() {
    // Not only the parse path: `info` wrote nothing to stdout for *any*
    // error-severity finding, so fixing the parse case alone would leave
    // the flag honest for one kind of failure and silent for the other.
    let tmp = TempDir::new().expect("tempdir");
    let path = unresolved(&tmp);
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E_UNRESOLVED_SLOT"),
        "the finding that stopped the report should be in it: {stdout}",
    );
}

/// A source whose only finding is one the strict per-edition pass sees.
///
/// The edition-neutral gate unions slot names across a theme's per-edition
/// variants, so a slot only one variant declares passes there and fails
/// inside the parity dry-run. Which pass raised the finding is the one
/// thing the caller did not ask about, so it cannot decide whether a
/// document is written.
fn edition_specific(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("split.crn");
    fs::write(
        &path,
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
    .expect("write");
    path
}

#[test]
fn info_json_renders_an_edition_specific_failure_the_same_way() {
    // The last `info` path that exited 1 with an empty stdout: the parity
    // dry-run returned a bare exit code, so `--format json` was never
    // consulted and the divergence the command exists to show was the one
    // shape a JSON consumer could not see.
    let tmp = TempDir::new().expect("tempdir");
    let path = edition_specific(&tmp);
    let out = cairn(
        "info",
        &[
            path.to_str().unwrap(),
            "--editions",
            "java,bedrock",
            "--format",
            "json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E_UNRESOLVED_SLOT"
            && d["primary"]
                .as_str()
                .is_some_and(|p| p.contains("theme `shop_bedrock`"))),
        "the finding only the bedrock pass sees should be in it: {stdout}",
    );
    // Not merely "the error is not on both streams": on an errors-only run
    // stderr is empty, and the stronger assertion is what catches the
    // edition header being hoisted out of the loop and printed over a
    // walk that held everything back.
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.is_empty(),
        "the findings belong to the document, and nothing else had anything to say: {stderr:?}",
    );
}

#[test]
fn info_json_leaves_a_per_edition_warning_on_stderr() {
    // The other half of the loop. A warning belongs to a row the caller is
    // about to discard, so it stays prose under the note naming its
    // edition while the error goes to the document — and the note prints
    // for the edition that has something left to head and not for the one
    // whose only finding was held back.
    //
    // `W_THEME_VARIANT_REBOUND` is raised only under a pinned edition, so
    // `theme=shop_bedrock` on the site is a warning the java pass sees and
    // the bedrock pass does not; the slot only `shop_java` declares is the
    // error the bedrock pass sees. The edition-neutral gate raises neither.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rebound.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\n\
         def kiosk class=house size=5x5:\n\
         \x20\x20walls id=walls class=outer mat_slot=wall height=3\n\n\
         theme shop_java:\n\
         \x20\x20slot wall           -> @wall.stone.cobble\n\
         \x20\x20slot floating_text  -> @sign.oak\n\n\
         theme shop_bedrock:\n\
         \x20\x20slot wall           -> @wall.stone.cobble\n\n\
         struct shop size=5x5\n\
         \x20\x20walls  class=outer mat_slot=wall height=3\n\
         \x20\x20window class=display side=front offset=2 y=2 size=1x1 mat_slot=floating_text\n\n\
         site market:\n\
         \x20\x20place id=stall use=kiosk theme=shop_bedrock at=origin\n",
    )
    .expect("write");
    let out = cairn(
        "info",
        &[
            path.to_str().unwrap(),
            "--editions",
            "java,bedrock",
            "--format",
            "json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let codes: Vec<&str> = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"))
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert_eq!(
        codes,
        ["E_UNRESOLVED_SLOT"],
        "the error, and only the error: {stdout}",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("W_THEME_VARIANT_REBOUND"),
        "the warning still reads as prose: {stderr}",
    );
    assert!(
        stderr.contains("note: reported for --editions java"),
        "under the note naming the edition that raised it: {stderr}",
    );
    assert!(
        !stderr.contains("--editions bedrock"),
        "and no note over an edition whose only finding went to the document: {stderr}",
    );
    assert!(
        !stderr.contains("E_UNRESOLVED_SLOT"),
        "the error is the document's alone: {stderr}",
    );
}

#[test]
fn info_json_carries_every_edition_the_walk_refused() {
    // The loop walks every requested edition before returning, so one bad
    // edition does not hide a second one's findings. Holding the errors
    // back for a document written after the walk has to keep that: a
    // document carrying only the first edition's finding would be the same
    // regression in a shape that parses.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("diverged.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\n\
         theme shop_java:\n\
         \x20\x20slot wall      -> @wall.stone.cobble\n\
         \x20\x20slot only_java -> @sign.oak\n\n\
         theme shop_bedrock:\n\
         \x20\x20slot wall         -> @wall.stone.cobble\n\
         \x20\x20slot only_bedrock -> @sign.oak\n\n\
         struct shop size=5x5\n\
         \x20\x20walls  class=outer mat_slot=wall height=3\n\
         \x20\x20window class=display side=front offset=2 y=2 size=1x1 mat_slot=only_java\n\
         \x20\x20window class=display side=back offset=2 y=2 size=1x1 mat_slot=only_bedrock\n",
    )
    .expect("write");
    let out = cairn(
        "info",
        &[
            path.to_str().unwrap(),
            "--editions",
            "java,bedrock",
            "--format",
            "json",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let refused: Vec<&str> = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"))
        .iter()
        .filter_map(|d| d["primary"].as_str())
        .collect();
    assert!(
        refused.iter().any(|p| p.contains("theme `shop_java`")),
        "the java pass's finding: {stdout}",
    );
    assert!(
        refused.iter().any(|p| p.contains("theme `shop_bedrock`")),
        "the bedrock pass's finding: {stdout}",
    );
}

#[test]
fn info_text_still_reports_an_edition_specific_failure_as_prose() {
    // The other half of the fix: `--format text` prints what it printed
    // before, under the note naming the edition that raised it, and writes
    // nothing to stdout. Holding the errors back in both formats would
    // have moved the prose to the end of the walk, away from its note.
    let tmp = TempDir::new().expect("tempdir");
    let path = edition_specific(&tmp);
    let out = cairn(
        "info",
        &[path.to_str().unwrap(), "--editions", "java,bedrock"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "text reports on stderr, got {:?}",
        String::from_utf8_lossy(&out.stdout),
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let edition_note = stderr
        .find("note: reported for --editions bedrock")
        .unwrap_or_else(|| panic!("the note naming the edition: {stderr}"));
    let finding = stderr
        .find("error[E_UNRESOLVED_SLOT]")
        .unwrap_or_else(|| panic!("the finding itself: {stderr}"));
    assert!(
        edition_note < finding,
        "the finding reads under the note that gives it its edition: {stderr}",
    );
}

/// The three refusals a `.crn` can reach, asked the one question together.
///
/// The per-path tests above each pin one failure's contents; this asks
/// all three for a parseable document in one place, so the promise reads
/// as a property of the command rather than three separate assertions.
/// What it does not do is catch a fourth path: the array below is three
/// hand-written fixtures, not an enumeration of `run_info`'s exits, and a
/// path none of them reaches is a path this stays green on.
///
/// The fourth refusal is not here and cannot be: a palette the pack
/// refuses has no `.crn` that reaches it — `PackView::lookup` answers
/// with a bare blockstate and the lexer refuses an authored `@id[k=v]` —
/// so the only way to raise one is to intern the entry into a lowering,
/// which `a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is`
/// does. It writes the document with no elements, since the leak has no
/// span in the source and no repair the author could make.
#[test]
fn every_refusal_a_source_can_reach_still_writes_a_document() {
    let tmp = TempDir::new().expect("tempdir");
    let refused: [(&str, PathBuf); 3] = [
        ("a source that does not parse", unparsable(&tmp)),
        ("an error in the edition-neutral pass", unresolved(&tmp)),
        (
            "an error only a per-edition pass sees",
            edition_specific(&tmp),
        ),
    ];
    for (what, path) in refused {
        let out = cairn(
            "info",
            &[
                path.to_str().unwrap(),
                "--editions",
                "java,bedrock",
                "--format",
                "json",
            ],
        );
        assert_eq!(out.status.code(), Some(1), "{what}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!("{what} should still write a document, got {stdout:?}: {err}")
        });
        assert!(
            parsed["diagnostics"].is_array(),
            "{what} should write the failure document, got {stdout}",
        );
    }
}

#[test]
fn info_json_on_a_clean_source_is_still_the_report() {
    // The failure document is additive: a source that has a report still
    // gets exactly the report, with no `diagnostics` key grafted on.
    let path = examples_dir().join("cottage.crn");
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.get("registry_compat").is_some(), "{stdout}");
    assert!(parsed.get("diagnostics").is_none(), "{stdout}");
}

#[test]
fn an_unreadable_source_is_still_told_apart_from_an_unparsable_one() {
    // A missing file is the caller's mistake and exits 2; a file that does
    // not parse is the source's and exits 1. Neither should have moved, and
    // every subcommand that reads a source draws the line in the same
    // place — each row carries the flags the subcommand insists on.
    let out_dir = TempDir::new().expect("out tempdir");
    let out_arg = out_dir.path().to_str().expect("utf-8 path");
    let subcommands: [(&str, &[&str]); 6] = [
        ("parse", &[]),
        ("check", &["--format", "json"]),
        ("info", &["--format", "json"]),
        ("lower", &[]),
        ("synth", &["--experimental-logic-synth"]),
        ("compile", &["--edition", "java", "--out", out_arg]),
    ];
    for (sub, flags) in subcommands {
        let mut args = vec!["definitely-not-a-file.crn"];
        args.extend_from_slice(flags);
        let missing = cairn(sub, &args);
        assert_eq!(
            missing.status.code(),
            Some(2),
            "{sub} should still exit 2 for a missing file, stderr={}",
            String::from_utf8_lossy(&missing.stderr),
        );
        let tmp = TempDir::new().expect("tempdir");
        let bad = unparsable(&tmp);
        let mut args = vec![bad.to_str().unwrap()];
        args.extend_from_slice(flags);
        let out = cairn(sub, &args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{sub} should still exit 1 for an unparsable file, stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn parse_json_renders_a_parse_failure_as_a_document_of_its_own() {
    // `parse`'s product is the AST, so a failure is not that dump with a
    // hole in it — it is the document `info` writes, told apart by its
    // keys.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("parse", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    assert!(
        parsed.get("items").is_none(),
        "a failure document is not an AST: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
    assert_eq!(diagnostics[0]["line"], 2);
}

#[test]
fn lower_json_renders_a_parse_failure_as_a_document_of_its_own() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    assert!(
        parsed.get("structures").is_none(),
        "a failure document is not an IR: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
}

#[test]
fn lower_json_renders_a_later_pass_failure_the_same_way() {
    // Not only the parse path. `lower` refuses to dump an IR built from a
    // source `check` rejects, and that refusal was the other way stdout
    // came out empty.
    let tmp = TempDir::new().expect("tempdir");
    let path = unresolved(&tmp);
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E_UNRESOLVED_SLOT"),
        "the finding that stopped the dump should be in it: {stdout}",
    );
}

#[test]
fn a_dump_in_any_other_format_still_reports_as_prose_on_stderr() {
    // The document belongs to `--format json` alone: `parse --format
    // debug` and `lower --format ascii` are read by a person, and a JSON
    // object on their stdout would be the regression this test guards
    // against.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let dumps: [(&str, &[&str]); 3] = [
        ("parse", &["--format", "debug"]),
        ("lower", &["--format", "ascii"]),
        ("lower", &["--format", "debug"]),
    ];
    for (sub, flags) in dumps {
        let mut args = vec![path.to_str().unwrap()];
        args.extend_from_slice(flags);
        let out = cairn(sub, &args);
        assert_eq!(out.status.code(), Some(1), "{sub} {flags:?}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert!(stdout.is_empty(), "{sub} {flags:?} wrote {stdout}");
        assert!(
            stderr.contains("error[E_PARSE]:"),
            "{sub} {flags:?} should carry the code: {stderr}",
        );
    }
}

#[test]
fn a_dump_on_a_clean_source_is_still_the_dump() {
    // The failure document is additive here too: a source that has a dump
    // gets exactly the dump, with no `diagnostics` key grafted on.
    let path = examples_dir().join("cottage.crn");
    for sub in ["parse", "lower"] {
        let out = cairn(sub, &[path.to_str().unwrap(), "--format", "json"]);
        assert!(
            out.status.success(),
            "{sub}: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        assert!(parsed.get("diagnostics").is_none(), "{sub}: {stdout}");
    }
}

/// A run with warnings and no error still writes the dump, not the
/// document.
///
/// This is the branch the failure gate now guards, and no test stood on
/// it: `report_diagnostics` moved behind the error check, so a source
/// whose findings are all warnings has to come out the other side with
/// the IR on stdout, the warnings on stderr, and exit 0. A regression
/// here writes `{"diagnostics": [...]}` for a build that succeeded.
#[test]
fn a_dump_with_warnings_and_no_error_is_still_the_dump() {
    let tmp = TempDir::new().expect("tempdir");
    // `place use=` a `def` that declares no `size=`: `W_DEF_NO_SIZE` and
    // nothing of error severity.
    let path = tmp.path().join("warned.crn");
    fs::write(
        &path,
        "theme t:\n  slot floor -> @oak_planks\n\ndef hut:\n  floor mat_slot=floor\n\n\
         site s:\n  place id=a use=hut theme=t at=origin\n",
    )
    .expect("write");
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    assert!(
        parsed.get("diagnostics").is_none(),
        "a run with a product writes the product: {stdout}",
    );
    assert!(
        parsed.get("structures").is_some(),
        "and the product is the IR: {stdout}",
    );
    assert!(
        stderr.contains("warning[W_DEF_NO_SIZE]:"),
        "the warnings go to stderr as prose: {stderr}",
    );
}

/// The failure document is the whole report: stderr carries no second
/// copy of it.
///
/// Without this, a `report_diagnostics` call left behind beside the
/// document would keep every other assertion in this file green while
/// writing the same findings twice, in two shapes, to two streams.
#[test]
fn the_failure_document_is_not_also_prose_on_stderr() {
    let tmp = TempDir::new().expect("tempdir");
    let bad = unparsable(&tmp);
    let unchecked = unresolved(&tmp);
    let runs: [(&str, &PathBuf); 3] = [("parse", &bad), ("lower", &bad), ("lower", &unchecked)];
    for (command, path) in runs {
        let out = cairn(command, &[path.to_str().unwrap(), "--format", "json"]);
        assert_eq!(out.status.code(), Some(1), "{command}");
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert!(
            stderr.is_empty(),
            "`{command} --format json` reports on stdout alone, got {stderr:?}",
        );
    }
}

/// A note reaches the document, so a consumer reading one finds it.
///
/// The `@cairn` version note is the only note a parse failure carries,
/// and nothing read `diagnostics[0].notes` out of the JSON at all — the
/// other fixtures here declare no header, so their notes are empty
/// whatever the serialiser does with them.
#[test]
fn a_notes_array_reaches_the_failure_document() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("future.crn");
    fs::write(&path, "@cairn 9999.12\nstruct s size=3x3\n  floor a=%\n").expect("write");
    let out = cairn("parse", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let notes = parsed["diagnostics"][0]["notes"]
        .as_array()
        .unwrap_or_else(|| panic!("the failure should carry its notes: {stdout}"));
    assert!(
        notes.iter().any(|n| n["message"]
            .as_str()
            .is_some_and(|m| m.contains("newer than this build"))),
        "the version gap the header explains: {stdout}",
    );
}
