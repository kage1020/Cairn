//! A baseline run and a proposal run, side by side.

use cairn_lang_core::Edition;
use cairn_lang_core::lock::HashHex;
use eval::compare::{Refusal, compare};
use eval::metrics::summarize;
use eval::record::{Attempt, Backend, Outcome, RecordError, RunRecord, Stopped, Verdict};

use crate::common;

/// `baseline` with `change` made to its prompts and its metrics measured again.
fn changed(
    baseline: &RunRecord,
    change: impl Fn(&str, &mut Outcome, &mut Vec<Attempt>),
) -> RunRecord {
    let mut proposal = baseline.clone();
    proposal.spec_pages = HashHex::from_bytes(b"proposal");
    for prompt in &mut proposal.prompts {
        change(&prompt.id, &mut prompt.outcome, &mut prompt.attempts);
    }
    proposal.metrics = summarize(&proposal.prompts);
    proposal
}

/// The table `compare` prints for `baseline` and `proposal`, after the lines naming them.
fn table(baseline: &RunRecord, proposal: &RunRecord) -> String {
    let printed = compare(baseline, proposal)
        .expect("the runs asked the same thing")
        .to_string();
    let header = format!(
        "baseline: spec pages {}, compiler {version}\n\
         proposal: spec pages {}, compiler {version}\n\
         both: model {}, backend replay, prompt set {}, java 1.21.4, max_tokens 16000\n\n",
        baseline.spec_pages,
        proposal.spec_pages,
        common::MODEL,
        baseline.prompt_set,
        version = cairn_lang_core::CAIRN_VERSION,
    );
    printed
        .strip_prefix(&header)
        .unwrap_or_else(|| panic!("the comparison names both runs:\n{printed}"))
        .to_owned()
}

#[test]
fn a_comparison_pairs_the_prompts_both_runs_converged_on() {
    let baseline = common::replayed_run();
    // `shed` passes on its first attempt, and `pavilion` is written in fewer tokens.
    let proposal = changed(&baseline, |id, outcome, attempts| match (id, outcome) {
        ("shed", _) => {
            attempts.remove(0);
        }
        ("pavilion", Outcome::Converged { build, .. }) => build.tokens = 30,
        _ => {}
    });
    assert_eq!(
        table(&baseline, &proposal),
        "metric                  baseline  proposal  change\n\
         zero-shot compile rate  1/3       2/3       +1\n\
         fix convergence rate    2/3       2/3       +0\n\
         token efficiency        2.221     2.591     +0.370  over the 2 programs both runs built\n\
         edit stability, AST     0.125     0.125     +0.000  over the 1 edit both runs measured\n\
         edit stability, NBT     0.014     0.014     +0.000\n"
    );
}

#[test]
fn a_prompt_only_one_run_converged_on_moves_neither_runs_token_efficiency() {
    let baseline = common::replayed_run();
    let proposal = changed(&baseline, |id, outcome, attempts| {
        if id == "pavilion" {
            *outcome = Outcome::Failed;
            attempts[0].verdict = Verdict::Check;
        }
    });
    // Over `shed` alone, 73 blocks in 36 tokens.
    assert_eq!(
        table(&baseline, &proposal),
        "metric                  baseline  proposal  change\n\
         zero-shot compile rate  1/3       0/3       -1\n\
         fix convergence rate    2/3       1/3       -1\n\
         token efficiency        2.028     2.028     +0.000  over the 1 program both runs built\n\
         edit stability, AST     0.125     0.125     +0.000  over the 1 edit both runs measured\n\
         edit stability, NBT     0.014     0.014     +0.000\n"
    );
}

#[test]
fn a_rate_over_prompts_cut_short_compares_rates() {
    let baseline = common::replayed_run();
    let proposal = changed(&baseline, |id, outcome, attempts| {
        if id == "hut" {
            *outcome = Outcome::CutShort;
            attempts.truncate(1);
            attempts[0].verdict = Verdict::Truncated;
        }
    });
    assert_eq!(
        table(&baseline, &proposal),
        "metric                  baseline  proposal  change\n\
         zero-shot compile rate  1/3       1/2       +0.167  0 and 1 cut short\n\
         fix convergence rate    2/3       2/2       +0.333  0 and 1 cut short\n\
         token efficiency        2.221     2.221     +0.000  over the 2 programs both runs built\n\
         edit stability, AST     0.125     0.125     +0.000  over the 1 edit both runs measured\n\
         edit stability, NBT     0.014     0.014     +0.000\n"
    );
}

#[test]
fn runs_that_asked_different_things_are_not_compared() {
    let baseline = common::replayed_run();
    let mut other = baseline.clone();
    other.backend = Backend::Api;
    other.prompt_set = HashHex::from_bytes(b"other prompts");
    other.model = "other-placeholder".to_owned();
    other.edition = Edition::Bedrock;
    other.target = "1.21".to_owned();
    other.max_tokens = 8000;
    let refusal = compare(&baseline, &other).expect_err("the runs differ");
    assert_eq!(
        refusal,
        Refusal::Mismatch {
            fields: vec![
                "backend",
                "prompt_set",
                "model",
                "edition",
                "target",
                "max_tokens"
            ],
        }
    );
    assert_eq!(
        refusal.to_string(),
        "the records differ in backend, prompt_set, model, edition, target, max_tokens, so the \
         runs did not ask the same thing; run both through the same backend, with the same prompt \
         set, model, edition, target and max_tokens"
    );
}

#[test]
fn a_run_that_stopped_is_not_compared() {
    let baseline = common::replayed_run();
    let mut stopped = baseline.clone();
    stopped.stopped = Some(Stopped {
        prompt: "shed".to_owned(),
        attempt: 2,
        reason: "no response from the API".to_owned(),
        program: None,
    });
    let refusal = compare(&baseline, &stopped).expect_err("the proposal stopped");
    assert_eq!(
        refusal.to_string(),
        "the proposal run stopped at prompt `shed`, attempt 2, so its metrics cover only the \
         prompts before it; run it again to the end"
    );
}

#[test]
fn a_record_reads_back_only_in_the_harness_format() {
    let record = common::replayed_run();
    let json = serde_json::to_value(&record).expect("a record serializes");
    assert_eq!(
        RunRecord::from_json(&json.to_string()).expect("the record reads back"),
        record
    );

    let mut future = json.clone();
    future["format"] = 2.into();
    let refused = RunRecord::from_json(&future.to_string()).expect_err("format 2 is refused");
    assert!(
        matches!(refused, RecordError::Format { found: 2 }),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "the record is of format 2, and this harness reads format 1; compare it with the harness \
         that wrote it, or run it again with this one"
    );

    let mut unknown = json;
    unknown["seed"] = 7.into();
    let refused = RunRecord::from_json(&unknown.to_string()).expect_err("`seed` is refused");
    assert_eq!(
        refused.to_string(),
        "not a run record: unknown field `seed`, expected one of `format`, `compiler_version`, \
         `compiler_commit`, `backend`, `model`, `edition`, `target`, `max_tokens`, `prompt_set`, \
         `spec_pages`, `metrics`, `prompts`, `stopped`"
    );
}
