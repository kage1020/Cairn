//! A run: every prompt of a set through generation, the lint step and up to three corrections,
//! then every edit through the same loop from its base's converged program.

use std::collections::BTreeMap;
use std::path::Path;

use cairn_lang_core::lock::HashHex;
use serde::Serialize;
use thiserror::Error;

use crate::compiler::{Cells, Compiler, CompilerError, Outcome as Checked};
use crate::conversation::{self, Failure, INSTRUCTIONS};
use crate::metrics;
use crate::model::{Conversation, Message, Model, ModelError};
use crate::prompts::{Prompt, PromptSet};
use crate::record::{Attempt, Build, EditDiff, FORMAT, Outcome, PromptRecord, RunRecord, Verdict};
use crate::spec::SpecPages;

/// The corrections a conversation gets after its first attempt.
pub const CORRECTIONS: usize = 3;

/// What a run asks and of which model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The model id the record names.
    pub model: String,
    /// The `max_tokens` every request carries.
    pub max_tokens: u32,
}

/// Why a run stopped before its record was written.
#[derive(Debug, Error)]
pub enum RunError {
    /// The model gave no reply.
    #[error("prompt `{id}`: {error}")]
    Model {
        /// The prompt being asked.
        id: String,
        /// Why no reply came.
        error: ModelError,
    },
    /// The compiler could not be used.
    #[error("{0}")]
    Compiler(#[from] CompilerError),
    /// The scratch directory could not be made.
    #[error("cannot make a scratch directory: {0}")]
    Scratch(std::io::Error),
}

/// The sha256 over [`INSTRUCTIONS`] and `prompts`, which a record carries as `prompt_set`.
///
/// # Panics
///
/// Never: a prompt set always serializes to JSON.
#[must_use]
pub fn prompt_set_hash(prompts: &PromptSet) -> HashHex {
    #[derive(Serialize)]
    struct Hashed<'a> {
        instructions: &'a [&'a str],
        prompts: &'a [Prompt],
    }
    let hashed = Hashed {
        instructions: &INSTRUCTIONS,
        prompts: prompts.prompts(),
    };
    HashHex::from_bytes(&serde_json::to_vec(&hashed).expect("a prompt set serializes"))
}

/// A converged program and what it builds.
struct Converged {
    program: String,
    cells: Cells,
}

/// The conversations of one run, asked of one model and checked with one compiler.
struct Runner<'a> {
    model: &'a mut dyn Model,
    compiler: &'a Compiler,
    system: String,
    scratch: &'a Path,
}

impl Runner<'_> {
    /// The attempts of the conversation `first` opens, and the program that passed, if one did.
    fn converse(
        &mut self,
        id: &str,
        first: String,
    ) -> Result<(Vec<Attempt>, Option<Converged>), RunError> {
        let target = self.compiler.target().clone();
        let mut messages = vec![Message::User(first)];
        let mut attempts = Vec::new();
        for turn in 0..=CORRECTIONS {
            let reply = self
                .model
                .reply(&Conversation {
                    id,
                    system: &self.system,
                    messages: &messages,
                })
                .map_err(|error| RunError::Model {
                    id: id.to_owned(),
                    error,
                })?;
            let program = conversation::extract_program(&reply.text);
            let evaluation = match &program {
                Some(program) => Some(
                    self.compiler
                        .evaluate(program, &self.scratch.join(id).join(turn.to_string()))?,
                ),
                None => None,
            };
            let (verdict, failure) = match evaluation.as_ref().map(|e| &e.outcome) {
                None => (Verdict::NoProgram, Some(Failure::NoProgram)),
                Some(Checked::Check(report)) => (Verdict::Check, Some(Failure::Check(report))),
                Some(Checked::Compile(report)) => {
                    (Verdict::Compile, Some(Failure::Compile(report)))
                }
                Some(Checked::Empty) => (Verdict::Empty, Some(Failure::Empty)),
                Some(Checked::Pass(_)) => (Verdict::Pass, None),
            };
            let feedback = failure.map(|failure| conversation::feedback(failure, &target));
            attempts.push(Attempt {
                reply: reply.text.clone(),
                stop_reason: reply.stop_reason.clone(),
                usage: reply.usage,
                verdict,
                diagnostics: evaluation
                    .as_ref()
                    .map(|e| e.diagnostics.clone())
                    .unwrap_or_default(),
                feedback: feedback.clone(),
            });
            if let (Some(program), Some(Checked::Pass(cells))) =
                (program, evaluation.map(|e| e.outcome))
            {
                return Ok((attempts, Some(Converged { program, cells })));
            }
            messages.push(Message::Assistant(reply));
            messages.push(Message::User(
                feedback.expect("an attempt that fails has feedback"),
            ));
        }
        Ok((attempts, None))
    }
}

/// A record of what the prompt's conversation came to.
fn record(prompt: &Prompt, attempts: Vec<Attempt>, converged: Option<&Converged>) -> PromptRecord {
    PromptRecord {
        id: prompt.id.clone(),
        topic: prompt.topic,
        base: prompt.base.clone(),
        outcome: if converged.is_some() {
            Outcome::Passed
        } else {
            Outcome::Failed
        },
        attempts,
        build: converged.map(|converged| Build {
            program: converged.program.clone(),
            tokens: metrics::tokens(&converged.program).expect("a program that passed lexes"),
            blocks: converged.cells.len() as u64,
        }),
        edit: None,
    }
}

/// Runs `prompts` against `model` and `compiler`, calling `progress` as each prompt ends.
///
/// # Errors
///
/// [`RunError`] when the model gives no reply or the compiler cannot be used.
///
/// # Panics
///
/// When a program this checkout's `cairn check` passed does not lex or parse in-process.
pub fn run(
    settings: &Settings,
    prompts: &PromptSet,
    model: &mut dyn Model,
    compiler: &Compiler,
    progress: &mut dyn FnMut(&PromptRecord),
) -> Result<RunRecord, RunError> {
    let pages = SpecPages::embedded();
    let scratch = tempfile::tempdir().map_err(RunError::Scratch)?;
    compiler.validate(scratch.path())?;
    let target = compiler.target().clone();
    let mut runner = Runner {
        model,
        compiler,
        system: conversation::system(&pages),
        scratch: scratch.path(),
    };

    let mut records = BTreeMap::new();
    let mut converged = BTreeMap::new();
    for prompt in prompts.prompts().iter().filter(|p| p.base.is_none()) {
        let first = conversation::task(&prompt.prompt, &target);
        let (attempts, passed) = runner.converse(&prompt.id, first)?;
        let record = record(prompt, attempts, passed.as_ref());
        progress(&record);
        records.insert(prompt.id.clone(), record);
        if let Some(passed) = passed {
            converged.insert(prompt.id.clone(), passed);
        }
    }
    let edits = prompts
        .prompts()
        .iter()
        .filter_map(|prompt| Some((prompt, prompt.base.as_deref()?)));
    for (prompt, base) in edits {
        let record = match converged.get(base) {
            None => PromptRecord {
                id: prompt.id.clone(),
                topic: prompt.topic,
                base: prompt.base.clone(),
                outcome: Outcome::Skipped,
                attempts: Vec::new(),
                build: None,
                edit: None,
            },
            Some(before) => {
                let first = conversation::edit_task(&before.program, &prompt.prompt, &target);
                let (attempts, after) = runner.converse(&prompt.id, first)?;
                let mut record = record(prompt, attempts, after.as_ref());
                record.edit = after.map(|after| EditDiff {
                    ast: metrics::ast_diff(&before.program, &after.program)
                        .expect("programs that passed parse"),
                    blocks: metrics::block_diff(&before.cells, &after.cells),
                });
                record
            }
        };
        progress(&record);
        records.insert(prompt.id.clone(), record);
    }

    let prompts_run: Vec<PromptRecord> = records.into_values().collect();
    Ok(RunRecord {
        format: FORMAT,
        compiler_version: cairn_lang_core::CAIRN_VERSION.to_owned(),
        model: settings.model.clone(),
        edition: target.edition,
        target: target.version,
        max_tokens: settings.max_tokens,
        prompt_set: prompt_set_hash(prompts),
        spec_pages: pages.hash(),
        metrics: metrics::summarize(&prompts_run),
        prompts: prompts_run,
    })
}
