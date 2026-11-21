//! The loop and every metric, through a replayed run of the fixture set.

use std::collections::BTreeMap;

use cairn_lang_core::Edition;
use eval::compiler::Compiler;
use eval::conversation::{self, Target};
use eval::model::replay::Replay;
use eval::model::{Conversation, Message, Model, ModelError, Reply, Usage};
use eval::record::{RunRecord, Stopped};
use eval::run::{RunError, prompt_set_hash, run};
use eval::spec::SpecPages;
use serde_json::{Value, json};

use crate::common::{
    self, HUT_NO_MATERIAL, HUT_THEME_ONLY, HUT_WINDOW_PAST_WALL, MODEL, PAVILION, SHED,
    SHED_UNKNOWN_ARGUMENT, SHED_WITH_WINDOW, reply, settings,
};

/// A model that answers from a replay and keeps every conversation it was asked to continue.
struct Recording {
    replay: Replay,
    requests: Requests,
}

impl Model for Recording {
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError> {
        self.requests
            .push((conversation.id.to_owned(), conversation.messages.to_vec()));
        self.replay.reply(conversation)
    }
}

/// A model no run may ask.
struct Unasked;

impl Model for Unasked {
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError> {
        panic!("conversation `{}` was asked", conversation.id)
    }
}

/// The conversations a model was asked to continue, by prompt id.
type Requests = Vec<(String, Vec<Message>)>;

fn recorded_run() -> (RunRecord, Vec<String>, Requests) {
    let mut model = Recording {
        replay: Replay::parse(&common::replay_file()).expect("the replay parses"),
        requests: Vec::new(),
    };
    let mut ended = Vec::new();
    let record = run(
        &settings(),
        &common::prompts(),
        &mut model,
        &common::compiler(),
        &mut |prompt| ended.push(prompt.id.clone()),
    )
    .expect("the replayed run completes");
    (record, ended, model.requests)
}

fn attempt(reply: &str, verdict: &str, diagnostics: Value, feedback: Option<String>) -> Value {
    let mut attempt = json!({
        "reply": reply,
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0,
        },
        "verdict": verdict,
    });
    if diagnostics != json!([]) {
        attempt["diagnostics"] = diagnostics;
    }
    if let Some(feedback) = feedback {
        attempt["feedback"] = json!(feedback);
    }
    attempt
}

fn check_feedback(program: &str) -> String {
    conversation::report("check", &common::check_report(program), &common::target())
}

/// A rate as a record writes it.
fn rate(count: u64, of: u64, cut_short: u64) -> Value {
    #[expect(clippy::cast_precision_loss, reason = "a handful of prompts")]
    let rate = (of > 0).then(|| count as f64 / of as f64);
    json!({"count": count, "of": of, "cut_short": cut_short, "rate": rate})
}

/// Both rates of a topic.
fn topic(zero_shot: u64, converged: u64, of: u64) -> Value {
    json!({
        "zero_shot_compile_rate": rate(zero_shot, of, 0),
        "fix_convergence_rate": rate(converged, of, 0),
    })
}

/// The metrics of the fixture set's replayed run.
fn expected_metrics() -> Value {
    json!({
        "zero_shot_compile_rate": rate(1, 3, 0),
        "fix_convergence_rate": rate(2, 3, 0),
        "token_efficiency": {
            "programs": 2,
            "blocks": 73 + 98,
            "tokens": 36 + 41,
            "ratio": 171.0 / 77.0,
        },
        "edit_stability": {"measured": 1, "of": 2, "ast": 2.0 / 16.0, "nbt": 1.0 / 73.0},
        "topics": {
            "walls": topic(0, 1, 1),
            "openings": topic(0, 0, 1),
            "roofs": topic(1, 1, 1),
        },
        "requests": 8,
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0,
        },
    })
}

#[test]
fn a_replayed_run_records_every_attempt_and_metric() {
    let (record, _, _) = recorded_run();
    let error = |code: &str, line: u64, col: u64| json!([{"code": code, "severity": "error", "line": line, "col": col}]);
    let expected = json!({
        "format": 1,
        "compiler_version": cairn_lang_core::CAIRN_VERSION,
        "backend": "replay",
        "model": MODEL,
        "edition": "java",
        "target": "1.21.4",
        "max_tokens": 16000,
        "prompt_set": prompt_set_hash(&common::prompts()).to_string(),
        "spec_pages": SpecPages::embedded().hash().to_string(),
        "metrics": expected_metrics(),
        "prompts": [
            {
                "id": "hut",
                "topic": "openings",
                "outcome": "failed",
                "attempts": [
                    attempt(
                        "A hut needs walls before it can have windows.\n",
                        "no_program",
                        json!([]),
                        Some(conversation::NO_PROGRAM.to_owned()),
                    ),
                    attempt(
                        &reply("cairn", HUT_THEME_ONLY),
                        "empty",
                        json!([]),
                        Some(conversation::EMPTY.to_owned()),
                    ),
                    attempt(
                        &reply("cairn", HUT_NO_MATERIAL),
                        "check",
                        error("E_MISSING_MATERIAL", 5, 3),
                        Some(check_feedback(HUT_NO_MATERIAL)),
                    ),
                    attempt(
                        &reply("crn", HUT_WINDOW_PAST_WALL),
                        "check",
                        error("E_INVALID_MEMBER", 6, 3),
                        Some(check_feedback(HUT_WINDOW_PAST_WALL)),
                    ),
                ],
            },
            {
                "id": "hut-roof",
                "topic": "roofs",
                "base": "hut",
                "outcome": "skipped",
                "attempts": [],
            },
            {
                "id": "pavilion",
                "topic": "roofs",
                "outcome": "converged",
                "build": {"program": PAVILION, "tokens": 41, "blocks": 98},
                "attempts": [attempt(&reply("cairn", PAVILION), "pass", json!([]), None)],
            },
            {
                "id": "shed",
                "topic": "walls",
                "outcome": "converged",
                "build": {"program": SHED, "tokens": 36, "blocks": 73},
                "attempts": [
                    attempt(
                        &reply("cairn", SHED_UNKNOWN_ARGUMENT),
                        "check",
                        error("E_UNKNOWN_ARGUMENT", 7, 39),
                        Some(check_feedback(SHED_UNKNOWN_ARGUMENT)),
                    ),
                    attempt(&reply("cairn", SHED), "pass", json!([]), None),
                ],
            },
            {
                "id": "shed-window",
                "topic": "openings",
                "base": "shed",
                "outcome": "converged",
                "build": {"program": SHED_WITH_WINDOW, "tokens": 57, "blocks": 73},
                "edit": {
                    "ast": {"before": 7, "after": 9, "removed": 0, "added": 2, "ratio": 0.125},
                    "nbt": {"cells": 73, "changed": 1, "ratio": 1.0 / 73.0},
                },
                "attempts": [attempt(&reply("cairn", SHED_WITH_WINDOW), "pass", json!([]), None)],
            },
        ],
    });
    assert_eq!(
        serde_json::to_value(&record).expect("a record serializes"),
        expected
    );
}

#[test]
fn generation_prompts_end_before_any_edit_starts() {
    let (_, ended, _) = recorded_run();
    assert_eq!(
        ended,
        ["hut", "pavilion", "shed", "hut-roof", "shed-window"]
    );
}

#[test]
fn a_correction_carries_the_reply_before_it_unchanged_and_what_check_reported() {
    let (_, _, requests) = recorded_run();
    let prompts = common::prompts();
    let task = |id: &str| {
        let prompt = prompts.get(id).expect("the fixture holds the prompt");
        conversation::task(&prompt.prompt, &common::target())
    };
    let replayed = |text: String| Reply {
        content: json!([{"type": "text", "text": text}]),
        text,
        stop_reason: "end_turn".to_owned(),
        usage: Usage::default(),
    };
    let shed: Vec<&Vec<Message>> = requests
        .iter()
        .filter(|(id, _)| id == "shed")
        .map(|(_, messages)| messages)
        .collect();
    assert_eq!(
        shed,
        [
            &vec![Message::User(task("shed"))],
            &vec![
                Message::User(task("shed")),
                Message::Assistant(replayed(reply("cairn", SHED_UNKNOWN_ARGUMENT))),
                Message::User(check_feedback(SHED_UNKNOWN_ARGUMENT)),
            ],
        ]
    );
}

#[test]
fn an_edit_starts_from_the_program_its_base_converged_on() {
    let (_, _, requests) = recorded_run();
    let prompt = common::prompts()
        .get("shed-window")
        .expect("the fixture holds the edit")
        .clone();
    let edit: Vec<&Vec<Message>> = requests
        .iter()
        .filter(|(id, _)| id == "shed-window")
        .map(|(_, messages)| messages)
        .collect();
    assert_eq!(
        edit,
        [&vec![Message::User(conversation::edit_task(
            SHED,
            &prompt.prompt,
            &common::target()
        ))]]
    );
}

/// The fixture set's replay with `pavilion`'s conversation left out.
fn replay_without_pavilion() -> Replay {
    let file = common::replay_file();
    let start = file
        .find("[[conversation]]\nid = \"pavilion\"")
        .expect("pavilion is replayed");
    let end = start + file[start..].find("]\n\n").expect("its table ends") + 3;
    Replay::parse(&format!("{}{}", &file[..start], &file[end..])).expect("the replay parses")
}

#[test]
fn a_reply_the_model_does_not_give_stops_the_run_and_keeps_what_ended_before_it() {
    let mut ended = Vec::new();
    let record = run(
        &settings(),
        &common::prompts(),
        &mut replay_without_pavilion(),
        &common::compiler(),
        &mut |prompt| ended.push(prompt.id.clone()),
    )
    .expect("the run starts");
    assert_eq!(
        record.stopped,
        Some(Stopped {
            prompt: "pavilion".to_owned(),
            attempt: 1,
            reason: "the replay has no reply 1 for conversation `pavilion`".to_owned(),
            program: None,
        })
    );
    let kept: Vec<&str> = record
        .prompts
        .iter()
        .map(|prompt| prompt.id.as_str())
        .collect();
    assert_eq!((kept, ended), (vec!["hut"], vec!["hut".to_owned()]));
    assert_eq!(record.metrics.requests, 4);
}

/// A `cairn` that takes the target and reports its version, then panics on any program.
#[cfg(unix)]
fn panicking_cairn(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join("cairn");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         [ \"$1\" = --version ] && { echo 'cairn 0.0.0'; exit 0; }\n\
         [ -s main.crn ] || exit 0\n\
         echo \"thread 'main' panicked\" >&2\n\
         exit 101\n",
    )
    .expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in is made executable");
    path
}

#[cfg(unix)]
#[test]
fn a_compiler_that_fails_on_a_program_stops_the_run_and_keeps_the_program() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let compiler = Compiler::new(panicking_cairn(dir.path()), common::target());
    let mut model = Replay::parse(&common::replay_file()).expect("the replay parses");
    let record = run(
        &settings(),
        &common::prompts(),
        &mut model,
        &compiler,
        &mut |_| {},
    )
    .expect("the run starts");
    assert_eq!(
        (record.compiler_version.as_str(), record.prompts.len()),
        ("0.0.0", 0)
    );
    assert_eq!(
        record.stopped,
        Some(Stopped {
            prompt: "hut".to_owned(),
            attempt: 2,
            reason: "`cairn check --format json` ended with exit status: 101: thread 'main' \
                     panicked"
                .to_owned(),
            program: Some(HUT_THEME_ONLY.to_owned()),
        })
    );
}

/// A model that answers from a replay, but stops the first reply of some conversations with a
/// `stop_reason` that cuts them short.
struct CutShort {
    replay: Replay,
    stops: BTreeMap<&'static str, &'static str>,
}

impl Model for CutShort {
    fn reply(&mut self, conversation: &Conversation<'_>) -> Result<Reply, ModelError> {
        let mut reply = self.replay.reply(conversation)?;
        if let Some(stop) = self.stops.get(conversation.id) {
            (*stop).clone_into(&mut reply.stop_reason);
        }
        Ok(reply)
    }
}

#[test]
fn a_truncated_or_refused_reply_ends_its_conversation_and_no_rate_counts_it() {
    let mut model = CutShort {
        replay: Replay::parse(&common::replay_file()).expect("the replay parses"),
        stops: BTreeMap::from([("pavilion", "refusal"), ("shed", "max_tokens")]),
    };
    let record = run(
        &settings(),
        &common::prompts(),
        &mut model,
        &common::compiler(),
        &mut |_| {},
    )
    .expect("the run starts");
    let ended: Vec<(&str, Value, Vec<Value>)> = record
        .prompts
        .iter()
        .map(|prompt| {
            let json = serde_json::to_value(prompt).expect("a record serializes");
            let verdicts = prompt
                .attempts
                .iter()
                .map(|attempt| json!([attempt.verdict, attempt.feedback.is_some()]))
                .collect();
            (prompt.id.as_str(), json["outcome"].clone(), verdicts)
        })
        .collect();
    let no_program = json!(["no_program", true]);
    let check = json!(["check", true]);
    assert_eq!(
        ended,
        [
            (
                "hut",
                json!("failed"),
                vec![no_program, json!(["empty", true]), check.clone(), check]
            ),
            ("hut-roof", json!("skipped"), vec![]),
            (
                "pavilion",
                json!("cut_short"),
                vec![json!(["refused", false])]
            ),
            (
                "shed",
                json!("cut_short"),
                vec![json!(["truncated", false])]
            ),
            ("shed-window", json!("skipped"), vec![]),
        ]
    );
    let metrics = serde_json::to_value(&record.metrics).expect("the metrics serialize");
    assert_eq!(
        (
            &metrics["zero_shot_compile_rate"],
            &metrics["fix_convergence_rate"],
            &metrics["topics"]["roofs"],
        ),
        (
            &rate(0, 1, 2),
            &rate(0, 1, 2),
            &json!({
                "zero_shot_compile_rate": rate(0, 0, 1),
                "fix_convergence_rate": rate(0, 0, 1),
            }),
        )
    );
}

#[test]
fn a_target_the_compiler_does_not_ship_stops_the_run_before_any_request() {
    let compiler = Compiler::new(
        common::cairn().to_path_buf(),
        Target {
            edition: Edition::Java,
            version: "9.9".to_owned(),
        },
    );
    let error = run(
        &settings(),
        &common::prompts(),
        &mut Unasked,
        &compiler,
        &mut |_| {},
    )
    .expect_err("the target is refused");
    let RunError::Compiler(error) = error else {
        panic!("not a compiler error: {error}");
    };
    let message = error.to_string();
    assert!(
        message.starts_with("error: unsupported java target `9.9`. supported targets: "),
        "{message}"
    );
}
