//! A source whose artifacts share a file name is refused by every command
//! that predicts a build, not only by the build.
//!
//! The finding is `E_OUTPUT_NAME_COLLISION`, raised with the other site
//! findings, so `cairn check` with or without `--target`, `cairn info` and
//! `cairn compile` all see it before anything lowers.

use std::process::Command;

mod common;
use common::{Fixture, cargo_bin, compile_as};

/// The source the report opens with: a `struct hut` and a `place id=hut`
/// both write `hut.nbt`.
const COLLIDE: &str = "@cairn 2026.06

theme t:
  slot floor -> @oak_planks

struct hut size=3x3
  floor mat_slot=floor

def house size=3x3:
  floor mat_slot=floor

site s:
  place id=hut use=house theme=t at=origin
";

const CODE: &str = "E_OUTPUT_NAME_COLLISION";

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(cargo_bin())
        .args(args)
        .output()
        .expect("run cairn");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn check_refuses_with_and_without_a_target() {
    let fixture = Fixture::new("cli-output-name", "check", COLLIDE);
    let source = fixture.source();
    let source = source.to_str().unwrap();
    for args in [
        vec!["check", source],
        vec!["check", source, "--edition", "java", "--target", "1.21.4"],
        vec![
            "check",
            source,
            "--edition",
            "bedrock",
            "--target",
            "1.21.60",
        ],
    ] {
        let (code, _, stderr) = run(&args);
        assert_eq!(code, Some(1), "`cairn {}`: stderr={stderr}", args.join(" "));
        assert!(
            stderr.contains(&format!("error[{CODE}]")),
            "`cairn {}` must name the code: {stderr}",
            args.join(" "),
        );
    }

    let (code, stdout, _) = run(&["check", source, "--format", "json"]);
    assert_eq!(code, Some(1));
    let findings: serde_json::Value = serde_json::from_str(&stdout).expect("a JSON array");
    assert!(
        findings
            .as_array()
            .expect("an array")
            .iter()
            .any(|d| d["code"] == CODE && d["severity"] == "error"),
        "the JSON array must carry the code for a consumer to match on: {stdout}",
    );
}

#[test]
fn info_certifies_no_version() {
    let fixture = Fixture::new("cli-output-name", "info", COLLIDE);
    let source = fixture.source();
    let (code, stdout, stderr) = run(&[
        "info",
        source.to_str().unwrap(),
        "--editions",
        "java,bedrock",
    ]);
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(stderr.contains(&format!("error[{CODE}]")), "{stderr}");
    assert!(
        !stdout.contains("buildable targets"),
        "a source every target refuses must not get a buildable row: {stdout}",
    );
}

#[test]
fn compile_refuses_with_the_code_and_writes_nothing() {
    for (edition, target) in [("java", "1.21.4"), ("bedrock", "1.21.60")] {
        let fixture = Fixture::new("cli-output-name", edition, COLLIDE);
        let out = compile_as(&fixture, edition, target);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{edition}: stderr={stderr}");
        assert!(
            stderr.contains(&format!("error[{CODE}]")),
            "{edition}: {stderr}"
        );
        assert_eq!(
            fixture.artifacts(),
            Vec::<String>::new(),
            "{edition}: nothing is written"
        );
    }
}
