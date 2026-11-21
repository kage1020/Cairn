//! A run: every prompt of a set through generation, the lint step and up to three corrections,
//! then every edit through the same loop from its base's converged program.

use std::collections::BTreeMap;
use std::path::Path;

use cairn_lang_core::lock::HashHex;
use serde::Serialize;
use thiserror::Error;

use crate::compiler::{Cells, Checked, Compiler, CompilerError, Evaluation};
use crate::conversation::{self, EMPTY, INSTRUCTIONS, NO_PROGRAM};
use crate::metrics;
use crate::model::{Conversation, Message, Model, Reply};
use crate::prompts::{Prompt, PromptSet};
use crate::record::{
    Attempt, Backend, Build, EditDiff, FORMAT, Outcome, PromptRecord, RunRecord, Stopped, Verdict,
};
use crate::spec::SpecPages;

/// The corrections a conversation gets after its first attempt.
pub const CORRECTIONS: usize = 3;

/// What a run asks and of which model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The model id the record names.
    pub model: String,
    /// What answers the requests.
    pub backend: Backend,
    /// The `max_tokens` every request carries.
    pub max_tokens: u32,
}

/// Why a run could not start.
#[derive(Debug, Error)]
pub enum RunError {
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
    tokens: u64,
    cells: Cells,
}

impl Converged {
    fn build(&self) -> Build {
        Build {
            program: self.program.clone(),
            tokens: self.tokens,
            blocks: self.cells.len() as u64,
        }
    }
}

/// How a conversation ended.
enum Ended {
    Converged(Converged),
    Failed,
    CutShort,
}

/// An attempt recording `reply` and what it came to.
fn attempt(reply: &Reply, verdict: Verdict, evaluation: Option<&Evaluation>) -> Attempt {
    Attempt {
        reply: reply.text.clone(),
        stop_reason: reply.stop_reason.clone(),
        usage: reply.usage,
        verdict,
        diagnostics: evaluation
            .map(|evaluation| evaluation.diagnostics.clone())
            .unwrap_or_default(),
        feedback: None,
    }
}

/// The conversations of one run, asked of one model and checked with one compiler.
struct Runner<'a> {
    model: &'a mut dyn Model,
    compiler: &'a Compiler,
    system: String,
    scratch: &'a Path,
}

impl Runner<'_> {
    /// The attempts of the conversation `first` opens and how it ended, or where the run stopped.
    fn converse(&mut self, id: &str, first: String) -> Result<(Vec<Attempt>, Ended), Stopped> {
        let target = self.compiler.target().clone();
        let mut messages = vec![Message::User(first)];
        let mut attempts = Vec::new();
        for turn in 0..=CORRECTIONS {
            let stopped = |reason: String, program: Option<&String>| Stopped {
                prompt: id.to_owned(),
                attempt: turn as u64 + 1,
                reason,
                program: program.cloned(),
            };
            let reply = self
                .model
                .reply(&Conversation {
                    id,
                    system: &self.system,
                    messages: &messages,
                })
                .map_err(|error| stopped(error.to_string(), None))?;
            let cut = match reply.stop_reason.as_str() {
                "max_tokens" => Some(Verdict::Truncated),
                "refusal" => Some(Verdict::Refused),
                _ => None,
            };
            if let Some(verdict) = cut {
                attempts.push(attempt(&reply, verdict, None));
                return Ok((attempts, Ended::CutShort));
            }
            let Some(program) = conversation::extract_program(&reply.text) else {
                attempts.push(Attempt {
                    feedback: Some(NO_PROGRAM.to_owned()),
                    ..attempt(&reply, Verdict::NoProgram, None)
                });
                messages.push(Message::Assistant(reply));
                messages.push(Message::User(NO_PROGRAM.to_owned()));
                continue;
            };
            let dir = self.scratch.join(id).join(turn.to_string());
            let evaluation = self
                .compiler
                .evaluate(&program, &dir)
                .map_err(|error| stopped(error.to_string(), Some(&program)))?;
            let (verdict, feedback) = match &evaluation.checked {
                Checked::Pass(cells) => {
                    let tokens = metrics::tokens(&program).map_err(|error| {
                        stopped(
                            format!(
                                "`cairn check` passed a program the harness's own lexer refuses: \
                                 {error}; check with the `cairn` of the harness's checkout"
                            ),
                            Some(&program),
                        )
                    })?;
                    attempts.push(attempt(&reply, Verdict::Pass, Some(&evaluation)));
                    let cells = cells.clone();
                    return Ok((
                        attempts,
                        Ended::Converged(Converged {
                            program,
                            tokens,
                            cells,
                        }),
                    ));
                }
                Checked::Check(printed) => (
                    Verdict::Check,
                    conversation::report("check", printed, &target),
                ),
                Checked::Compile(printed) => (
                    Verdict::Compile,
                    conversation::report("compile", printed, &target),
                ),
                Checked::Empty => (Verdict::Empty, EMPTY.to_owned()),
            };
            attempts.push(Attempt {
                feedback: Some(feedback.clone()),
                ..attempt(&reply, verdict, Some(&evaluation))
            });
            messages.push(Message::Assistant(reply));
            messages.push(Message::User(feedback));
        }
        Ok((attempts, Ended::Failed))
    }
}

/// A record of `prompt`'s conversation.
fn record(prompt: &Prompt, outcome: Outcome, attempts: Vec<Attempt>) -> PromptRecord {
    PromptRecord {
        id: prompt.id.clone(),
        topic: prompt.topic,
        base: prompt.base.clone(),
        outcome,
        attempts,
    }
}

/// Asks every prompt of `prompts` in turn, generation before edits, adding each record to
/// `records` as it ends.
fn ask(
    runner: &mut Runner<'_>,
    prompts: &PromptSet,
    records: &mut BTreeMap<String, PromptRecord>,
    progress: &mut dyn FnMut(&PromptRecord),
) -> Result<(), Stopped> {
    let target = runner.compiler.target().clone();
    let mut converged = BTreeMap::new();
    for prompt in prompts.prompts().iter().filter(|p| p.base.is_none()) {
        let first = conversation::task(&prompt.prompt, &target);
        let (attempts, ended) = runner.converse(&prompt.id, first)?;
        let outcome = match ended {
            Ended::Converged(passed) => {
                let build = passed.build();
                converged.insert(prompt.id.clone(), passed);
                Outcome::Converged { build, edit: None }
            }
            Ended::Failed => Outcome::Failed,
            Ended::CutShort => Outcome::CutShort,
        };
        let record = record(prompt, outcome, attempts);
        progress(&record);
        records.insert(prompt.id.clone(), record);
    }
    let edits = prompts
        .prompts()
        .iter()
        .filter_map(|prompt| Some((prompt, prompt.base.as_deref()?)));
    for (prompt, base) in edits {
        let record = match converged.get(base) {
            None => record(prompt, Outcome::Skipped, Vec::new()),
            Some(before) => {
                let first = conversation::edit_task(&before.program, &prompt.prompt, &target);
                let (attempts, ended) = runner.converse(&prompt.id, first)?;
                let outcome = match ended {
                    Ended::Converged(after) => {
                        let ast = metrics::ast_diff(&before.program, &after.program).map_err(
                            |error| Stopped {
                                prompt: prompt.id.clone(),
                                attempt: attempts.len() as u64,
                                reason: format!(
                                    "`cairn check` passed a program the harness's own parser \
                                     refuses: {error}; check with the `cairn` of the harness's \
                                     checkout"
                                ),
                                program: Some(after.program.clone()),
                            },
                        )?;
                        Outcome::Converged {
                            build: after.build(),
                            edit: Some(EditDiff {
                                ast,
                                nbt: metrics::nbt_diff(&before.cells, &after.cells),
                            }),
                        }
                    }
                    Ended::Failed => Outcome::Failed,
                    Ended::CutShort => Outcome::CutShort,
                };
                record(prompt, outcome, attempts)
            }
        };
        progress(&record);
        records.insert(prompt.id.clone(), record);
    }
    Ok(())
}

/// Runs `prompts` against `model` and `compiler`, calling `progress` as each prompt ends; a run
/// stopped partway still returns its record, with `stopped` saying where and why.
///
/// # Errors
///
/// [`RunError`] when the compiler refuses the target or cannot be used, or no scratch directory
/// can be made, before any request is sent.
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
    let compiler_version = compiler.version()?;
    let mut runner = Runner {
        model,
        compiler,
        system: conversation::system(&pages),
        scratch: scratch.path(),
    };
    let mut records = BTreeMap::new();
    let stopped = ask(&mut runner, prompts, &mut records, progress).err();
    let prompts_run: Vec<PromptRecord> = records.into_values().collect();
    let target = compiler.target().clone();
    Ok(RunRecord {
        format: FORMAT,
        compiler_version,
        compiler_commit: compiler.commit().map(str::to_owned),
        backend: settings.backend,
        model: settings.model.clone(),
        edition: target.edition,
        target: target.version,
        max_tokens: settings.max_tokens,
        prompt_set: prompt_set_hash(prompts),
        spec_pages: pages.hash(),
        metrics: metrics::summarize(&prompts_run),
        prompts: prompts_run,
        stopped,
    })
}
