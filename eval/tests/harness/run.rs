//! The loop and every metric, through a replayed run of the fixture set.

use eval::compiler::Compiler;
use eval::conversation::{self, Failure, Target};
use eval::model::replay::Replay;
use eval::model::{Conversation, Message, Model, ModelError, Reply, Usage};
use eval::record::RunRecord;
use eval::run::{RunError, Settings, prompt_set_hash, run};
use eval::spec::SpecPages;
use serde_json::{Value, json};

use crate::common::{
    self, HUT_NO_MATERIAL, HUT_THEME_ONLY, HUT_WINDOW_PAST_WALL, MODEL, PAVILION, SHED,
    SHED_UNKNOWN_ARGUMENT, SHED_WITH_WINDOW, reply,
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

fn settings() -> Settings {
    Settings {
        model: MODEL.to_owned(),
        max_tokens: 16000,
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
    conversation::feedback(
        Failure::Check(&common::check_report(program)),
        &common::target(),
    )
}

/// The metrics of the fixture set's replayed run.
fn expected_metrics() -> Value {
    json!({
        "zero_shot_compile_rate": {"passed": 1, "of": 3, "rate": 1.0 / 3.0},
        "fix_convergence_rate": {"passed": 2, "of": 3, "rate": 2.0 / 3.0},
        "token_efficiency": {
            "programs": 2,
            "blocks": 73 + 98,
            "tokens": 36 + 41,
            "ratio": 171.0 / 77.0,
        },
        "edit_stability": {"measured": 1, "of": 2, "ast": 2.0 / 16.0, "blocks": 1.0 / 73.0},
        "topics": {
            "walls": {"prompts": 1, "zero_shot": 0, "converged": 1},
            "openings": {"prompts": 1, "zero_shot": 0, "converged": 0},
            "roofs": {"prompts": 1, "zero_shot": 1, "converged": 1},
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
                "outcome": "passed",
                "attempts": [attempt(&reply("cairn", PAVILION), "pass", json!([]), None)],
                "build": {"program": PAVILION, "tokens": 41, "blocks": 98},
            },
            {
                "id": "shed",
                "topic": "walls",
                "outcome": "passed",
                "attempts": [
                    attempt(
                        &reply("cairn", SHED_UNKNOWN_ARGUMENT),
                        "check",
                        error("E_UNKNOWN_ARGUMENT", 7, 39),
                        Some(check_feedback(SHED_UNKNOWN_ARGUMENT)),
                    ),
                    attempt(&reply("cairn", SHED), "pass", json!([]), None),
                ],
                "build": {"program": SHED, "tokens": 36, "blocks": 73},
            },
            {
                "id": "shed-window",
                "topic": "openings",
                "base": "shed",
                "outcome": "passed",
                "attempts": [attempt(&reply("cairn", SHED_WITH_WINDOW), "pass", json!([]), None)],
                "build": {"program": SHED_WITH_WINDOW, "tokens": 57, "blocks": 73},
                "edit": {
                    "ast": {"before": 7, "after": 9, "removed": 0, "added": 2, "ratio": 0.125},
                    "blocks": {"cells": 73, "changed": 1, "ratio": 1.0 / 73.0},
                },
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

#[test]
fn a_reply_the_replay_lacks_stops_the_run() {
    let mut model = Replay::parse("conversation = []").expect("an empty replay parses");
    let error = run(
        &settings(),
        &common::prompts(),
        &mut model,
        &common::compiler(),
        &mut |_| {},
    )
    .expect_err("the replay answers nothing");
    assert_eq!(
        error.to_string(),
        "prompt `hut`: the replay has no reply 1 for conversation `hut`"
    );
}

#[test]
fn a_target_the_compiler_does_not_ship_stops_the_run_before_any_request() {
    let compiler = Compiler::new(
        common::cairn().to_path_buf(),
        Target {
            edition: "java".to_owned(),
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
