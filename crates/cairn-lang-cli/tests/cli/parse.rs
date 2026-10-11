//! `cairn parse <file>` end to end, against every shipped example, so
//! "the source parses" holds at the CLI surface and not only through the
//! library API. That a missing file exits 2 is pinned for every subcommand
//! at once in `contract/json.rs`.

use crate::common::{cairn, crn_examples, examples_dir};

#[test]
fn every_example_parses_to_a_json_module() {
    for path in crn_examples() {
        let out = cairn("parse", &[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{}: stderr={}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).expect("stdout utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|err| panic!("{}: stdout is not JSON: {err}", path.display()));
        assert!(
            parsed.get("items").is_some(),
            "{}: a parsed module carries its items, got {stdout}",
            path.display(),
        );
    }
}

#[test]
fn debug_format_runs_successfully() {
    // `--format debug` is a developer-facing escape hatch; its concrete shape
    // is whatever `{:?}` happens to produce and is not part of the CLI
    // contract. We only assert that the subcommand exits successfully and
    // writes non-empty UTF-8 to stdout, so the test does not couple to the
    // derived `Debug` representation of internal AST types.
    let path = examples_dir().join("cottage.crn");
    let out = cairn("parse", &[path.to_str().unwrap(), "--format", "debug"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        !stdout.trim().is_empty(),
        "debug stdout should not be empty"
    );
}
