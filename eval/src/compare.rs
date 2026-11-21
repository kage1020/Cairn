//! A proposal's evidence: its run beside a baseline run of the same prompts, model and
//! target, as `spec/evaluation` "Operating rule" asks a proposal to cite it.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::metrics::{mean, ratio};
use crate::record::{PromptRecord, Rate, RunRecord};

/// Why two records cannot be compared.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Refusal {
    /// A run stopped before every prompt ended, so its metrics cover only some of them.
    #[error(
        "the {run} run stopped at prompt `{prompt}`, attempt {attempt}, so its metrics cover only \
         the prompts before it; run it again to the end"
    )]
    Stopped {
        /// `baseline` or `proposal`.
        run: &'static str,
        /// The prompt it stopped at.
        prompt: String,
        /// The attempt it stopped at.
        attempt: u64,
    },
    /// The runs did not ask the same thing.
    #[error(
        "the records differ in {}, so the runs did not ask the same thing; run both through the \
         same backend, with the same prompt set, model, edition, target and max_tokens",
        fields.join(", ")
    )]
    Mismatch {
        /// The fields that differ.
        fields: Vec<&'static str>,
    },
}

/// Two runs side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    baseline: Side,
    proposal: Side,
    shared: String,
    zero_shot: (Rate, Rate),
    converged: (Rate, Rate),
    programs: u64,
    efficiency: (Option<f64>, Option<f64>),
    edits: u64,
    ast: (Option<f64>, Option<f64>),
    nbt: (Option<f64>, Option<f64>),
}

#[derive(Debug, Clone, PartialEq)]
struct Side {
    spec_pages: String,
    compiler: String,
}

impl Side {
    fn of(record: &RunRecord) -> Self {
        Self {
            spec_pages: record.spec_pages.to_string(),
            compiler: match &record.compiler_commit {
                Some(commit) => format!("{} at {commit}", record.compiler_version),
                None => record.compiler_version.clone(),
            },
        }
    }
}

fn by_id(record: &RunRecord) -> BTreeMap<&str, &PromptRecord> {
    record
        .prompts
        .iter()
        .map(|prompt| (prompt.id.as_str(), prompt))
        .collect()
}

/// `baseline` and `proposal` side by side, token efficiency and edit stability taken over the
/// prompts both runs converged on, so a prompt only one run built does not move either.
///
/// # Errors
///
/// [`Refusal::Stopped`] when either run stopped before it ended, and [`Refusal::Mismatch`] when
/// they differ in format, backend, prompt set, model, edition, target or `max_tokens`.
pub fn compare(baseline: &RunRecord, proposal: &RunRecord) -> Result<Comparison, Refusal> {
    for (run, record) in [("baseline", baseline), ("proposal", proposal)] {
        if let Some(stopped) = &record.stopped {
            return Err(Refusal::Stopped {
                run,
                prompt: stopped.prompt.clone(),
                attempt: stopped.attempt,
            });
        }
    }
    let differs = [
        ("format", baseline.format != proposal.format),
        ("backend", baseline.backend != proposal.backend),
        ("prompt_set", baseline.prompt_set != proposal.prompt_set),
        ("model", baseline.model != proposal.model),
        ("edition", baseline.edition != proposal.edition),
        ("target", baseline.target != proposal.target),
        ("max_tokens", baseline.max_tokens != proposal.max_tokens),
    ];
    let fields: Vec<&'static str> = differs
        .into_iter()
        .filter_map(|(field, differs)| differs.then_some(field))
        .collect();
    if !fields.is_empty() {
        return Err(Refusal::Mismatch { fields });
    }

    let (before, after) = (by_id(baseline), by_id(proposal));
    let both: Vec<(&PromptRecord, &PromptRecord)> = before
        .iter()
        .filter_map(|(id, base)| after.get(id).map(|other| (*base, *other)))
        .collect();
    let generation: Vec<_> = both
        .iter()
        .filter(|(a, _)| a.base.is_none())
        .filter_map(|(a, b)| a.outcome.build().zip(b.outcome.build()))
        .collect();
    let [blocks_before, tokens_before, blocks_after, tokens_after] =
        generation.iter().fold([0; 4], |[b0, t0, b1, t1], (a, b)| {
            [b0 + a.blocks, t0 + a.tokens, b1 + b.blocks, t1 + b.tokens]
        });
    let edits: Vec<_> = both
        .iter()
        .filter_map(|(a, b)| a.outcome.edit().zip(b.outcome.edit()))
        .collect();
    let metrics = (&baseline.metrics, &proposal.metrics);
    Ok(Comparison {
        baseline: Side::of(baseline),
        proposal: Side::of(proposal),
        shared: format!(
            "model {}, backend {}, prompt set {}, {} {}, max_tokens {}",
            baseline.model,
            baseline.backend,
            baseline.prompt_set,
            baseline.edition,
            baseline.target,
            baseline.max_tokens
        ),
        zero_shot: (
            metrics.0.zero_shot_compile_rate.clone(),
            metrics.1.zero_shot_compile_rate.clone(),
        ),
        converged: (
            metrics.0.fix_convergence_rate.clone(),
            metrics.1.fix_convergence_rate.clone(),
        ),
        programs: generation.len() as u64,
        efficiency: (
            ratio(blocks_before, tokens_before),
            ratio(blocks_after, tokens_after),
        ),
        edits: edits.len() as u64,
        ast: (
            mean(edits.iter().map(|(a, _)| a.ast.ratio)),
            mean(edits.iter().map(|(_, b)| b.ast.ratio)),
        ),
        nbt: (
            mean(edits.iter().map(|(a, _)| a.nbt.ratio)),
            mean(edits.iter().map(|(_, b)| b.nbt.ratio)),
        ),
    })
}

/// A rate's row: each run's count out of its prompts, the change, and how many each left out.
fn rate_row(name: &'static str, (a, b): &(Rate, Rate)) -> [String; 5] {
    let delta = if a.of == b.of {
        format!("{:+}", i128::from(b.count) - i128::from(a.count))
    } else {
        change((a.rate, b.rate))
    };
    let note = if a.cut_short == 0 && b.cut_short == 0 {
        String::new()
    } else {
        format!("{} and {} cut short", a.cut_short, b.cut_short)
    };
    [
        name.to_owned(),
        format!("{}/{}", a.count, a.of),
        format!("{}/{}", b.count, b.of),
        delta,
        note,
    ]
}

fn over(count: u64, one: &str, many: &str, rest: &str) -> String {
    format!(
        "over the {count} {} {rest}",
        if count == 1 { one } else { many }
    )
}

fn value(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.3}"))
}

fn change(pair: (Option<f64>, Option<f64>)) -> String {
    match pair {
        (Some(before), Some(after)) => format!("{:+.3}", after - before),
        _ => "-".to_owned(),
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = [
            rate_row("zero-shot compile rate", &self.zero_shot),
            rate_row("fix convergence rate", &self.converged),
            [
                "token efficiency".to_owned(),
                value(self.efficiency.0),
                value(self.efficiency.1),
                change(self.efficiency),
                over(self.programs, "program", "programs", "both runs built"),
            ],
            [
                "edit stability, AST".to_owned(),
                value(self.ast.0),
                value(self.ast.1),
                change(self.ast),
                over(self.edits, "edit", "edits", "both runs measured"),
            ],
            [
                "edit stability, NBT".to_owned(),
                value(self.nbt.0),
                value(self.nbt.1),
                change(self.nbt),
                String::new(),
            ],
        ];
        writeln!(
            f,
            "baseline: spec pages {}, compiler {}",
            self.baseline.spec_pages, self.baseline.compiler
        )?;
        writeln!(
            f,
            "proposal: spec pages {}, compiler {}",
            self.proposal.spec_pages, self.proposal.compiler
        )?;
        writeln!(f, "both: {}", self.shared)?;
        writeln!(f)?;
        writeln!(
            f,
            "{:<24}{:<10}{:<10}change",
            "metric", "baseline", "proposal"
        )?;
        for [metric, before, after, delta, note] in rows {
            let line = format!("{metric:<24}{before:<10}{after:<10}{delta:<8}{note}");
            writeln!(f, "{}", line.trim_end())?;
        }
        Ok(())
    }
}
