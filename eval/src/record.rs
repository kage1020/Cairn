//! The run record: what a run asked, of which model, against which specification and compiler,
//! and what came back, written as JSON.

use std::collections::BTreeMap;

use cairn_lang_core::Edition;
use cairn_lang_core::lock::HashHex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::Usage;
use crate::prompts::Topic;

/// The record's own format, which a change to its shape raises.
pub const FORMAT: u32 = 1;

/// One run of the prompt set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    /// [`FORMAT`] when the record was written.
    pub format: u32,
    /// The release the `cairn` binary reports with `--version`.
    pub compiler_version: String,
    /// The commit the harness built the `cairn` binary at, when it built it from its checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler_commit: Option<String>,
    /// What answered the run.
    pub backend: Backend,
    /// The model id the run asked, as `--model` named it.
    pub model: String,
    /// The edition every program was checked and compiled for.
    pub edition: Edition,
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
    /// Each prompt's conversation that ended, in id order.
    pub prompts: Vec<PromptRecord>,
    /// Where and why the run stopped before every prompt ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

/// Why a record cannot be read.
#[derive(Debug, Error)]
pub enum RecordError {
    /// The text is not a run record of any format.
    #[error("not a run record: {0}")]
    Invalid(serde_json::Error),
    /// The record is of a format this harness does not read.
    #[error(
        "the record is of format {found}, and this harness reads format {FORMAT}; compare it with \
         the harness that wrote it, or run it again with this one"
    )]
    Format {
        /// The record's `format`.
        found: u64,
    },
}

impl RunRecord {
    /// The record `text` holds.
    ///
    /// # Errors
    ///
    /// [`RecordError::Format`] when its `format` is not [`FORMAT`], and [`RecordError::Invalid`]
    /// when it is not a record.
    pub fn from_json(text: &str) -> Result<Self, RecordError> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(RecordError::Invalid)?;
        match value.get("format").and_then(serde_json::Value::as_u64) {
            Some(found) if found != u64::from(FORMAT) => Err(RecordError::Format { found }),
            _ => serde_json::from_value(value).map_err(RecordError::Invalid),
        }
    }
}

/// What answered a run's requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// The Anthropic Messages API.
    Api,
    /// Replies written down before, so the record's `model` is only the name it was given.
    Replay,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Api => "api",
            Self::Replay => "replay",
        })
    }
}

/// Where a run stopped, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stopped {
    /// The prompt whose conversation was going on.
    pub prompt: String,
    /// The attempt it stopped at, from 1.
    pub attempt: u64,
    /// Why the harness could not go on.
    pub reason: String,
    /// The program the harness could not check, when the reply held one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
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
    #[serde(flatten)]
    pub outcome: Outcome,
    /// Each attempt, in turn order.
    pub attempts: Vec<Attempt>,
}

/// How a prompt's conversation ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// An attempt passed.
    Converged {
        /// What the passing attempt built.
        build: Build,
        /// For an edit, how far it moved from its base.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edit: Option<EditDiff>,
    },
    /// No attempt passed.
    Failed,
    /// An attempt was truncated or refused, which ended the conversation.
    CutShort,
    /// An edit whose base did not converge, so it was not asked.
    Skipped,
}

impl Outcome {
    /// What the conversation built, when it converged.
    #[must_use]
    pub fn build(&self) -> Option<&Build> {
        match self {
            Self::Converged { build, .. } => Some(build),
            _ => None,
        }
    }

    /// How far an edit moved from its base, when it converged.
    #[must_use]
    pub fn edit(&self) -> Option<&EditDiff> {
        match self {
            Self::Converged { edit, .. } => edit.as_ref(),
            _ => None,
        }
    }
}

/// Why an attempt passed or did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// `cairn check` reported no error, `cairn compile` built it, and it builds a block.
    Pass,
    /// The reply holds no program.
    NoProgram,
    /// `cairn check` refused the program.
    Check,
    /// `cairn compile` refused a program `cairn check` passed.
    Compile,
    /// The program builds no block.
    Empty,
    /// The reply stopped at `max_tokens`.
    Truncated,
    /// The model declined to reply.
    Refused,
}

impl Verdict {
    /// Whether the attempt cut its conversation short.
    #[must_use]
    pub fn cuts_short(self) -> bool {
        matches!(self, Self::Truncated | Self::Refused)
    }
}

/// One reply and what the harness made of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct EditDiff {
    /// The AST diff.
    pub ast: AstDiff,
    /// The NBT diff.
    pub nbt: NbtDiff,
}

/// The AST nodes an edit removed and added, against the nodes of both programs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct NbtDiff {
    /// Cells that hold a block in either build.
    pub cells: u64,
    /// Of those, cells whose block differs between the builds.
    pub changed: u64,
    /// `changed / cells`.
    pub ratio: f64,
}

/// The primary metrics of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    /// Generation prompts whose first attempt passed.
    pub zero_shot_compile_rate: Rate,
    /// Generation prompts whose conversation converged.
    pub fix_convergence_rate: Rate,
    /// Blocks per DSL token over the converged generation prompts.
    pub token_efficiency: TokenEfficiency,
    /// The mean diffs of the edits measured.
    pub edit_stability: EditStability,
    /// The two rates over the generation prompts of each topic.
    pub topics: BTreeMap<Topic, TopicRates>,
    /// Requests sent, one per attempt.
    pub requests: u64,
    /// What the requests spent.
    pub usage: Usage,
}

/// A count of generation prompts out of the ones a rate is taken over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    /// Prompts counted.
    pub count: u64,
    /// Prompts the rate is taken over.
    pub of: u64,
    /// Prompts left out, cut short before the rate could judge them.
    pub cut_short: u64,
    /// `count / of`, with no value when there were none.
    pub rate: Option<f64>,
}

/// Expanded blocks over DSL tokens, summed over programs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// The mean AST and NBT diffs over the edits measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditStability {
    /// Edits whose base and own conversation both converged.
    pub measured: u64,
    /// Edit prompts there were.
    pub of: u64,
    /// The mean AST diff ratio, with no value when no edit was measured.
    pub ast: Option<f64>,
    /// The mean NBT diff ratio, with no value when no edit was measured.
    pub nbt: Option<f64>,
}

/// How the generation prompts of one topic did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopicRates {
    /// The topic's zero-shot compile rate.
    pub zero_shot_compile_rate: Rate,
    /// The topic's fix convergence rate.
    pub fix_convergence_rate: Rate,
}
