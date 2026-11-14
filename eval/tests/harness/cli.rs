//! `cargo eval` itself: what only the command line does.

use std::fs;
use std::process::{Command, Output};

use eval::compare::compare;
use eval::record::RunRecord;

use crate::common::{self, MODEL};

fn harness() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_eval"));
    command.env_remove("ANTHROPIC_API_KEY");
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("the harness prints UTF-8")
}

fn replayed(dir: &std::path::Path) -> Output {
    let replay = dir.join("replay.toml");
    fs::write(&replay, common::replay_file()).expect("the replay is written");
    harness()
        .args([
            "run",
            "--model",
            MODEL,
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ])
        .arg("--replay")
        .arg(&replay)
        .arg("--prompts")
        .arg(common::fixture("prompts"))
        .arg("--cairn")
        .arg(common::cairn())
        .arg("--out")
        .arg(dir.join("record.json"))
        .output()
        .expect("the harness runs")
}

#[test]
fn run_writes_the_record_and_reports_each_prompt_as_it_ends() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let output = replayed(dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stderr),
        "hut: failed after 4 attempts\n\
         pavilion: passed on attempt 1\n\
         shed: passed on attempt 2\n\
         hut-roof: skipped: `hut` did not converge\n\
         shed-window: passed on attempt 1\n\
         zero-shot 1/3, converged 2/3, edits measured 1/2, 8 requests\n"
    );
    assert_eq!(text(&output.stdout), "");
    let written: RunRecord = serde_json::from_str(
        &fs::read_to_string(dir.path().join("record.json")).expect("the record is written"),
    )
    .expect("the record reads back");
    assert_eq!(written, common::replayed_run());
}

#[test]
fn run_names_no_model_or_target_of_its_own() {
    let output = harness()
        .args(["run", "--replay", "replay.toml"])
        .output()
        .expect("the harness runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).starts_with(
            "error: the following required arguments were not provided:\n  --model <MODEL>\n  \
             --edition <EDITION>\n  --target <TARGET>\n"
        ),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn run_without_a_key_or_a_replay_asks_for_a_key_before_any_request() {
    let output = harness()
        .args([
            "run",
            "--model",
            MODEL,
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ])
        .output()
        .expect("the harness runs");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stderr),
        "error: ANTHROPIC_API_KEY is not set; set it to an Anthropic API key, or pass `--replay` \
         to answer from recorded replies\n"
    );
    assert_eq!(text(&output.stdout), "");
}

#[test]
fn compare_prints_what_the_library_compares() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let baseline = common::replayed_run();
    let mut proposal = baseline.clone();
    proposal.compiler_version = "proposal".to_owned();
    for (name, record) in [("baseline.json", &baseline), ("proposal.json", &proposal)] {
        fs::write(
            dir.path().join(name),
            serde_json::to_string(record).expect("a record serializes"),
        )
        .expect("the record is written");
    }
    let output = harness()
        .arg("compare")
        .arg(dir.path().join("baseline.json"))
        .arg(dir.path().join("proposal.json"))
        .output()
        .expect("the harness runs");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        compare(&baseline, &proposal)
            .expect("the runs asked the same thing")
            .to_string()
    );
}

#[test]
fn compare_refuses_runs_of_different_models() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let baseline = common::replayed_run();
    let mut proposal = baseline.clone();
    proposal.model = "other-placeholder".to_owned();
    for (name, record) in [("baseline.json", &baseline), ("proposal.json", &proposal)] {
        fs::write(
            dir.path().join(name),
            serde_json::to_string(record).expect("a record serializes"),
        )
        .expect("the record is written");
    }
    let output = harness()
        .arg("compare")
        .arg(dir.path().join("baseline.json"))
        .arg(dir.path().join("proposal.json"))
        .output()
        .expect("the harness runs");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stderr),
        "error: the records differ in model, so the runs did not ask the same thing; run both \
         with the same prompt set, model, edition, target and max_tokens\n"
    );
    assert_eq!(text(&output.stdout), "");
}
