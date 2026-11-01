//! `--format json` writes one JSON document to stdout for every readable source, failing ones
//! included.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::common::{assert_stream, cairn, examples_dir};

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
    assert_stream(&stderr, &[(2, 11, "error", "E_PARSE")]);
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
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.is_empty(),
        "the findings belong to the document, and nothing else had anything to say: {stderr:?}",
    );
}

#[test]
fn info_json_leaves_a_per_edition_warning_on_stderr() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("rebound.crn");
    fs::write(
        &path,
        "@cairn 2026.06\n\n\
         def kiosk size=5x5:\n\
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
    assert_stream(&stderr, &[(18, 3, "warning", "W_THEME_VARIANT_REBOUND")]);
    assert!(
        stderr.contains("note: reported for --editions java"),
        "under the note naming the edition that raised it: {stderr}",
    );
    assert!(
        !stderr.contains("--editions bedrock"),
        "and no note over an edition whose only finding went to the document: {stderr}",
    );
}

#[test]
fn info_json_carries_every_edition_the_walk_refused() {
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

/// Hand-written fixtures, one per refusal; a refused palette's document has no elements.
#[test]
fn every_refusal_a_source_can_reach_still_writes_a_document() {
    let tmp = TempDir::new().expect("tempdir");
    let refused: [(&str, PathBuf); 4] = [
        ("a source that does not parse", unparsable(&tmp)),
        ("an error in the edition-neutral pass", unresolved(&tmp)),
        (
            "an error only a per-edition pass sees",
            edition_specific(&tmp),
        ),
        ("a palette the pack refuses", refused_state(&tmp)),
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

/// A stair bound to a `facing` no stair has; the Bedrock portability walk is first to refuse it.
fn refused_state(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("refused_state.crn");
    fs::write(
        &path,
        "theme t:\n  slot step -> @oak_stairs[facing=up]\n\nstruct s size=3x3\n  floor mat_slot=step\n",
    )
    .expect("write");
    path
}

#[test]
fn an_authored_state_the_target_lacks_is_named_as_the_sources() {
    let tmp = TempDir::new().expect("tempdir");
    let path = refused_state(&tmp);
    let out = cairn(
        "info",
        &[path.to_str().unwrap(), "--editions", "java,bedrock"],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("`facing=up`"), "{stderr}");
    assert!(
        stderr.contains("a state literal on a stair in the source"),
        "{stderr}",
    );
}

#[test]
fn an_authored_state_bedrock_cannot_map_yet_is_counted_as_unsupported() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("log.crn");
    fs::write(
        &path,
        "theme t:\n  slot f -> @oak_log[axis=x]\n\nstruct s size=3x3\n  floor mat_slot=f\n",
    )
    .expect("write");
    let out = cairn(
        "info",
        &[path.to_str().unwrap(), "--editions", "java,bedrock"],
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(stdout.contains("unsupported: 1"), "{stdout}");
    assert!(
        stderr.contains("`minecraft:oak_log`") && stderr.contains("`axis=x` has no form here yet"),
        "{stderr}",
    );
}

#[test]
fn info_json_on_a_clean_source_is_still_the_report() {
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
fn a_check_of_a_source_that_cannot_be_read_writes_no_document() {
    // Exit 1 over `[]` is a run-level refusal (`spec/lint` "Machine-readable payload"), so an
    // unreadable file writes nothing. Non-UTF-8 is unreadable on every platform.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("not-utf8.crn");
    fs::write(&path, b"@cairn 2026.06\n\xff\xfe binary\n").expect("write");
    let out = cairn("check", &[path.to_str().unwrap(), "--format", "json"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        out.stdout.is_empty(),
        "no document, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    assert!(stderr.contains("cannot read"), "stderr={stderr}");
}

#[test]
fn parse_json_renders_a_parse_failure_as_a_document_of_its_own() {
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
        assert_stream(&stderr, &[(2, 11, "error", "E_PARSE")]);
    }
}

#[test]
fn a_dump_on_a_clean_source_is_still_the_dump() {
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

#[test]
fn a_dump_with_warnings_and_no_error_is_still_the_dump() {
    let tmp = TempDir::new().expect("tempdir");
    // A `def` with no `size=`: `W_DEF_NO_SIZE` and nothing of error severity.
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
    assert_stream(&stderr, &[(4, 1, "warning", "W_DEF_NO_SIZE")]);
}

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

/// The `@cairn` version note is the only note a parse failure carries.
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
