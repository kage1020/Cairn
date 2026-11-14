//! The run record: what a run asked, of which model, against which specification and compiler,
//! and what came back, written as JSON.

use std::collections::BTreeMap;

use cairn_lang_core::lock::HashHex;
use serde::{Deserialize, Serialize};

use crate::model::Usage;
use crate::prompts::Topic;

/// The record's own format, which a change to its shape raises.
pub const FORMAT: u32 = 1;

/// One run of the prompt set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// [`FORMAT`] when the record was written.
    pub format: u32,
    /// The Cairn release the checkout is.
    pub compiler_version: String,
    /// The model id the run asked, as `--model` named it.
    pub model: String,
    /// The edition every program was checked and compiled for.
    pub edition: String,
    /// The version every program was checked and compiled for, as `--target` named it.
    pub target: String,
    /// The `max_tokens` of every request.
    pub max_tokens: u32,
    /// The sha256 over the harness's instructions and the prompts the run asked.
    pub prompt_set: HashHex,
    /// The sha256 over the specification pages the system prompt carried.
    pub spec_pages: HashHex,
    /// The primary metrics over the prompts below.
    pub metrics: Metrics,
    /// Each prompt's conversation, in id order.
    pub prompts: Vec<PromptRecord>,
}

/// How a prompt's conversation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// An attempt passed.
    Passed,
    /// No attempt passed.
    Failed,
    /// An edit whose base did not converge, so it was not asked.
    Skipped,
}

/// One prompt's conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptRecord {
    /// The prompt's id.
    pub id: String,
    /// What the prompt exercises.
    pub topic: Topic,
    /// For an edit, the prompt whose program it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// How the conversation ended.
    pub outcome: Outcome,
    /// Each attempt, in turn order.
    pub attempts: Vec<Attempt>,
    /// What the passing attempt built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<Build>,
    /// For an edit that passed, how far it moved from its base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<EditDiff>,
}

/// Why an attempt passed or did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Checked and compiled clean, and builds at least one block.
    Pass,
    /// The reply holds no program.
    NoProgram,
    /// `cairn check` refused the program.
    Check,
    /// `cairn compile` refused a program `cairn check` passed.
    Compile,
    /// The program builds no block.
    Empty,
}

/// One reply and what the harness made of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    /// The reply's text.
    pub reply: String,
    /// Why the model stopped.
    pub stop_reason: String,
    /// What the request spent.
    pub usage: Usage,
    /// What the attempt came to.
    pub verdict: Verdict,
    /// What `cairn check --format json` reported on the program, warnings included.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticRecord>,
    /// What the harness told the model next, on an attempt that did not pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

/// One diagnostic `cairn check` reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRecord {
    /// The diagnostic code.
    pub code: String,
    /// `error` or `warning`.
    pub severity: String,
    /// The 1-based line it points at.
    pub line: u64,
    /// The 1-based column it points at.
    pub col: u64,
}

/// What a passing program is and builds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The program.
    pub program: String,
    /// Its DSL tokens.
    pub tokens: u64,
    /// The blocks it expands to.
    pub blocks: u64,
}

/// How far an edit's program and build moved from its base's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditDiff {
    /// The AST diff.
    pub ast: AstDiff,
    /// The block diff.
    pub blocks: BlockDiff,
}

/// The AST nodes an edit removed and added, against the nodes of both programs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AstDiff {
    /// The base program's nodes.
    pub before: u64,
    /// The edited program's nodes.
    pub after: u64,
    /// Nodes of the base the edit does not keep.
    pub removed: u64,
    /// Nodes of the edit the base does not have.
    pub added: u64,
    /// `(removed + added) / (before + after)`.
    pub ratio: f64,
}

/// The cells an edit changed, against the cells either build fills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockDiff {
    /// Cells that hold a block in either build.
    pub cells: u64,
    /// Of those, cells whose block differs between the builds.
    pub changed: u64,
    /// `changed / cells`.
    pub ratio: f64,
}

/// The primary metrics of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Generation prompts whose first attempt passed.
    pub zero_shot_compile_rate: Rate,
    /// Generation prompts whose conversation converged.
    pub fix_convergence_rate: Rate,
    /// Blocks per DSL token over the converged generation prompts.
    pub token_efficiency: TokenEfficiency,
    /// The mean diffs of the edits measured.
    pub edit_stability: EditStability,
    /// The generation prompts of each topic.
    pub topics: BTreeMap<Topic, TopicCounts>,
    /// Requests sent, one per attempt.
    pub requests: u64,
    /// What the requests spent.
    pub usage: Usage,
}

/// A count of prompts out of a total.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rate {
    /// Prompts counted.
    pub passed: u64,
    /// Prompts there were.
    pub of: u64,
    /// `passed / of`, with no value when there were none.
    pub rate: Option<f64>,
}

/// Expanded blocks over DSL tokens, summed over programs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenEfficiency {
    /// Programs summed.
    pub programs: u64,
    /// Their blocks.
    pub blocks: u64,
    /// Their DSL tokens.
    pub tokens: u64,
    /// `blocks / tokens`, with no value when there were no programs.
    pub ratio: Option<f64>,
}

/// The mean AST and block diffs over the edits measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditStability {
    /// Edits whose base and own conversation both converged.
    pub measured: u64,
    /// Edit prompts there were.
    pub of: u64,
    /// The mean AST diff ratio, with no value when no edit was measured.
    pub ast: Option<f64>,
    /// The mean block diff ratio, with no value when no edit was measured.
    pub blocks: Option<f64>,
}

/// How the generation prompts of one topic did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicCounts {
    /// Generation prompts of the topic.
    pub prompts: u64,
    /// Of those, the ones whose first attempt passed.
    pub zero_shot: u64,
    /// Of those, the ones whose conversation converged.
    pub converged: u64,
}
