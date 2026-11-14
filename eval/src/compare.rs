//! A proposal's evidence: its run beside a baseline run of the same prompts, model and
//! target, as `spec/evaluation` "Operating rule" asks a proposal to cite it.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::record::{Outcome, PromptRecord, RunRecord};

/// Why two records cannot be compared.
#[derive(Debug, Error, PartialEq, Eq)]
#[error(
    "the records differ in {}, so the runs did not ask the same thing; run both with the same \
     prompt set, model, edition, target and max_tokens",
    fields.join(", ")
)]
pub struct Mismatch {
    /// The fields that differ.
    pub fields: Vec<&'static str>,
}

/// Two runs side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    baseline: Side,
    proposal: Side,
    shared: String,
    prompts: u64,
    zero_shot: (u64, u64),
    converged: (u64, u64),
    programs: u64,
    efficiency: (Option<f64>, Option<f64>),
    edits: u64,
    ast: (Option<f64>, Option<f64>),
    blocks: (Option<f64>, Option<f64>),
}

#[derive(Debug, Clone, PartialEq)]
struct Side {
    spec_pages: String,
    compiler_version: String,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of blocks, tokens or edits is far below 2^52"
)]
fn quotient(part: u64, whole: u64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of edits is far below 2^52"
)]
fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
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
/// [`Mismatch`] when the runs asked different prompts, of a different model, or for a different
/// target.
pub fn compare(baseline: &RunRecord, proposal: &RunRecord) -> Result<Comparison, Mismatch> {
    let mut fields = Vec::new();
    if baseline.format != proposal.format {
        fields.push("format");
    }
    if baseline.prompt_set != proposal.prompt_set {
        fields.push("prompt_set");
    }
    if baseline.model != proposal.model {
        fields.push("model");
    }
    if baseline.edition != proposal.edition {
        fields.push("edition");
    }
    if baseline.target != proposal.target {
        fields.push("target");
    }
    if baseline.max_tokens != proposal.max_tokens {
        fields.push("max_tokens");
    }
    if !fields.is_empty() {
        return Err(Mismatch { fields });
    }

    let (before, after) = (by_id(baseline), by_id(proposal));
    let both: Vec<(&PromptRecord, &PromptRecord)> = before
        .iter()
        .filter_map(|(id, base)| after.get(id).map(|other| (*base, *other)))
        .filter(|(base, other)| base.outcome == Outcome::Passed && other.outcome == Outcome::Passed)
        .collect();
    let generation: Vec<_> = both
        .iter()
        .filter(|(a, _)| a.base.is_none())
        .filter_map(|(a, b)| a.build.as_ref().zip(b.build.as_ref()))
        .collect();
    let [blocks_before, tokens_before, blocks_after, tokens_after] =
        generation.iter().fold([0; 4], |[b0, t0, b1, t1], (a, b)| {
            [b0 + a.blocks, t0 + a.tokens, b1 + b.blocks, t1 + b.tokens]
        });
    let edits: Vec<_> = both
        .iter()
        .filter_map(|(a, b)| a.edit.as_ref().zip(b.edit.as_ref()))
        .collect();
    let metrics = (&baseline.metrics, &proposal.metrics);
    Ok(Comparison {
        baseline: Side {
            spec_pages: baseline.spec_pages.to_string(),
            compiler_version: baseline.compiler_version.clone(),
        },
        proposal: Side {
            spec_pages: proposal.spec_pages.to_string(),
            compiler_version: proposal.compiler_version.clone(),
        },
        shared: format!(
            "model {}, prompt set {}, {} {}, max_tokens {}",
            baseline.model,
            baseline.prompt_set,
            baseline.edition,
            baseline.target,
            baseline.max_tokens
        ),
        prompts: metrics.0.zero_shot_compile_rate.of,
        zero_shot: (
            metrics.0.zero_shot_compile_rate.passed,
            metrics.1.zero_shot_compile_rate.passed,
        ),
        converged: (
            metrics.0.fix_convergence_rate.passed,
            metrics.1.fix_convergence_rate.passed,
        ),
        programs: generation.len() as u64,
        efficiency: (
            quotient(blocks_before, tokens_before),
            quotient(blocks_after, tokens_after),
        ),
        edits: edits.len() as u64,
        ast: (
            mean(edits.iter().map(|(a, _)| a.ast.ratio)),
            mean(edits.iter().map(|(_, b)| b.ast.ratio)),
        ),
        blocks: (
            mean(edits.iter().map(|(a, _)| a.blocks.ratio)),
            mean(edits.iter().map(|(_, b)| b.blocks.ratio)),
        ),
    })
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
            (
                "zero-shot compile rate",
                format!("{}/{}", self.zero_shot.0, self.prompts),
                format!("{}/{}", self.zero_shot.1, self.prompts),
                format!(
                    "{:+}",
                    i128::from(self.zero_shot.1) - i128::from(self.zero_shot.0)
                ),
                String::new(),
            ),
            (
                "fix convergence rate",
                format!("{}/{}", self.converged.0, self.prompts),
                format!("{}/{}", self.converged.1, self.prompts),
                format!(
                    "{:+}",
                    i128::from(self.converged.1) - i128::from(self.converged.0)
                ),
                String::new(),
            ),
            (
                "token efficiency",
                value(self.efficiency.0),
                value(self.efficiency.1),
                change(self.efficiency),
                over(self.programs, "program", "programs", "both runs built"),
            ),
            (
                "edit stability, AST",
                value(self.ast.0),
                value(self.ast.1),
                change(self.ast),
                over(self.edits, "edit", "edits", "both runs measured"),
            ),
            (
                "edit stability, blocks",
                value(self.blocks.0),
                value(self.blocks.1),
                change(self.blocks),
                String::new(),
            ),
        ];
        writeln!(
            f,
            "baseline: spec pages {}, compiler {}",
            self.baseline.spec_pages, self.baseline.compiler_version
        )?;
        writeln!(
            f,
            "proposal: spec pages {}, compiler {}",
            self.proposal.spec_pages, self.proposal.compiler_version
        )?;
        writeln!(f, "both: {}", self.shared)?;
        writeln!(f)?;
        writeln!(
            f,
            "{:<24}{:<10}{:<10}change",
            "metric", "baseline", "proposal"
        )?;
        for (metric, before, after, delta, note) in rows {
            let line = format!("{metric:<24}{before:<10}{after:<10}{delta:<8}{note}");
            writeln!(f, "{}", line.trim_end())?;
        }
        Ok(())
    }
}
