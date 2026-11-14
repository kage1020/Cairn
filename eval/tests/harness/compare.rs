//! A baseline run and a proposal run, side by side.

use cairn_lang_core::lock::HashHex;
use eval::compare::{Mismatch, compare};
use eval::metrics::summarize;
use eval::record::RunRecord;

use crate::common;

/// The replayed run as a proposal might have changed it: a new spec, `shed` passing on its first
/// attempt, and `pavilion` written in fewer tokens.
fn proposal(baseline: &RunRecord) -> RunRecord {
    let mut proposal = baseline.clone();
    proposal.spec_pages = HashHex::from_bytes(b"proposal");
    for prompt in &mut proposal.prompts {
        match prompt.id.as_str() {
            "shed" => {
                prompt.attempts.remove(0);
            }
            "pavilion" => prompt.build.as_mut().expect("pavilion built").tokens = 30,
            _ => {}
        }
    }
    proposal.metrics = summarize(&proposal.prompts);
    proposal
}

#[test]
fn a_comparison_pairs_the_prompts_both_runs_converged_on() {
    let baseline = common::replayed_run();
    let proposal = proposal(&baseline);
    let comparison = compare(&baseline, &proposal).expect("the runs asked the same thing");
    let expected = format!(
        "baseline: spec pages {}, compiler {version}\n\
         proposal: spec pages {}, compiler {version}\n\
         both: model {}, prompt set {}, java 1.21.4, max_tokens 16000\n\
         \n\
         metric                  baseline  proposal  change\n\
         zero-shot compile rate  1/3       2/3       +1\n\
         fix convergence rate    2/3       2/3       +0\n\
         token efficiency        2.221     2.591     +0.370  over the 2 programs both runs built\n\
         edit stability, AST     0.125     0.125     +0.000  over the 1 edit both runs measured\n\
         edit stability, blocks  0.014     0.014     +0.000\n",
        baseline.spec_pages,
        proposal.spec_pages,
        common::MODEL,
        baseline.prompt_set,
        version = cairn_lang_core::CAIRN_VERSION,
    );
    assert_eq!(comparison.to_string(), expected);
}

#[test]
fn runs_that_asked_different_things_are_not_compared() {
    let baseline = common::replayed_run();
    let mut other = baseline.clone();
    other.model = "other-placeholder".to_owned();
    other.target = "1.21".to_owned();
    let mismatch = compare(&baseline, &other).expect_err("the runs differ");
    assert_eq!(
        mismatch,
        Mismatch {
            fields: vec!["model", "target"],
        }
    );
    assert_eq!(
        mismatch.to_string(),
        "the records differ in model, target, so the runs did not ask the same thing; run both \
         with the same prompt set, model, edition, target and max_tokens"
    );
}
