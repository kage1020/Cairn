//! `cargo eval`: runs the prompt set against a model and writes the run record, or compares two.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::Edition;
use clap::builder::{PossibleValuesParser, TypedValueParser as _};
use clap::{Args, Parser, Subcommand};
use eval::compiler::Compiler;
use eval::conversation::Target;
use eval::model::Model;
use eval::model::anthropic::Anthropic;
use eval::model::replay::Replay;
use eval::prompts::PromptSet;
use eval::record::{Backend, Outcome, PromptRecord, RunRecord, Verdict};
use eval::run::{Settings, run};
use tempfile::NamedTempFile;

/// The prompt set this checkout carries.
const PROMPTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/prompts");

#[derive(Parser)]
#[command(
    name = "cargo eval",
    about = "Measure the Cairn specification against a model, as `spec/evaluation` describes"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the prompt set and write the run record.
    Run(RunArgs),
    /// Print two run records side by side: a baseline, then a proposal.
    Compare {
        /// The baseline run's record.
        baseline: PathBuf,
        /// The proposal run's record.
        proposal: PathBuf,
    },
}

#[derive(Args)]
struct RunArgs {
    /// The model id to ask, which the record names.
    #[arg(long)]
    model: String,
    /// Answer from this replay file instead of the Anthropic API.
    #[arg(long)]
    replay: Option<PathBuf>,
    /// The edition every program is checked and compiled for.
    #[arg(
        long,
        value_parser = PossibleValuesParser::new(["java", "bedrock"])
            .try_map(|name| name.parse::<Edition>())
    )]
    edition: Edition,
    /// The version every program is checked and compiled for, as `cairn compile` takes it.
    #[arg(long)]
    target: String,
    /// The `max_tokens` of every request.
    #[arg(long, default_value_t = 16000)]
    max_tokens: u32,
    /// Run only this prompt, and the base of an edit; repeat it to run several.
    #[arg(long = "only", value_name = "ID")]
    only: Vec<String>,
    /// The prompt set's directory.
    #[arg(long, default_value = PROMPTS)]
    prompts: PathBuf,
    /// The `cairn` binary to check with, instead of building this checkout's.
    #[arg(long)]
    cairn: Option<PathBuf>,
    /// Write the record here instead of to stdout.
    #[arg(long)]
    out: Option<PathBuf>,
}

fn progress(record: &PromptRecord) {
    let attempts = record.attempts.len();
    let line = match &record.outcome {
        Outcome::Converged { .. } => format!("converged on attempt {attempts}"),
        Outcome::Failed => format!("failed after {attempts} attempts"),
        Outcome::CutShort => {
            let why = match record.attempts.last().map(|attempt| attempt.verdict) {
                Some(Verdict::Refused) => "the model declined to reply",
                _ => "the reply stopped at max_tokens",
            };
            format!("cut short at attempt {attempts}: {why}")
        }
        Outcome::Skipped => format!(
            "skipped: `{}` did not converge",
            record.base.as_deref().unwrap_or_default()
        ),
    };
    eprintln!("{}: {line}", record.id);
}

/// Where the record goes.
enum Out {
    /// Standard output.
    Stdout,
    /// A file made beside `path` before the run starts, so an unwritable directory is found
    /// before any request, and moved onto `path` once the record is written.
    File {
        staged: NamedTempFile,
        path: PathBuf,
    },
}

impl Out {
    fn new(path: Option<PathBuf>) -> Result<Self, String> {
        let Some(path) = path else {
            return Ok(Self::Stdout);
        };
        if path.is_dir() {
            return Err(format!(
                "`--out {}` is a directory; name the file to write the record to",
                path.display()
            ));
        }
        let dir = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = NamedTempFile::new_in(dir).map_err(|error| {
            format!(
                "cannot write the record to {}: {error}; name a file in a directory that exists \
                 and can be written",
                path.display()
            )
        })?;
        Ok(Self::File { staged, path })
    }

    fn write(self, record: &RunRecord) -> Result<(), String> {
        let json = serde_json::to_string_pretty(record)
            .map_err(|error| format!("cannot write the record: {error}"))?
            + "\n";
        match self {
            Self::Stdout => std::io::stdout()
                .write_all(json.as_bytes())
                .map_err(|error| format!("cannot write the record: {error}")),
            Self::File { mut staged, path } => {
                let failed =
                    |error: std::io::Error| format!("cannot write {}: {error}", path.display());
                staged.write_all(json.as_bytes()).map_err(failed)?;
                staged
                    .persist(&path)
                    .map(drop)
                    .map_err(|error| failed(error.error))
            }
        }
    }
}

fn run_prompts(args: RunArgs) -> Result<(), String> {
    let mut prompts = PromptSet::load(&args.prompts).map_err(|error| error.to_string())?;
    if !args.only.is_empty() {
        prompts = prompts
            .select(&args.only)
            .map_err(|error| error.to_string())?;
    }
    let out = Out::new(args.out)?;
    let (mut model, backend): (Box<dyn Model>, Backend) = match &args.replay {
        Some(path) => (
            Box::new(Replay::load(path).map_err(|error| error.to_string())?),
            Backend::Replay,
        ),
        None => (
            Box::new(
                Anthropic::from_env(args.model.clone(), args.max_tokens)
                    .map_err(|error| error.to_string())?,
            ),
            Backend::Api,
        ),
    };
    let target = Target {
        edition: args.edition,
        version: args.target,
    };
    let compiler = match args.cairn {
        Some(binary) => Compiler::new(binary, target),
        None => Compiler::from_checkout(target).map_err(|error| error.to_string())?,
    };
    let settings = Settings {
        model: args.model,
        backend,
        max_tokens: args.max_tokens,
    };
    let record = run(
        &settings,
        &prompts,
        model.as_mut(),
        &compiler,
        &mut progress,
    )
    .map_err(|error| error.to_string())?;
    out.write(&record)?;
    let metrics = &record.metrics;
    let (zero_shot, converged) = (
        &metrics.zero_shot_compile_rate,
        &metrics.fix_convergence_rate,
    );
    let cut_short = if converged.cut_short == 0 {
        String::new()
    } else {
        format!(", {} cut short", converged.cut_short)
    };
    eprintln!(
        "zero-shot {}/{}, converged {}/{}{cut_short}, edits measured {}/{}, {} requests",
        zero_shot.count,
        zero_shot.of,
        converged.count,
        converged.of,
        metrics.edit_stability.measured,
        metrics.edit_stability.of,
        metrics.requests
    );
    match &record.stopped {
        None => Ok(()),
        Some(stopped) => Err(format!(
            "the run stopped at prompt `{}`, attempt {}: {}; the record holds the prompts that \
             ended before it",
            stopped.prompt, stopped.attempt, stopped.reason
        )),
    }
}

fn read_record(path: &Path) -> Result<RunRecord, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    RunRecord::from_json(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn compare_records(baseline: &Path, proposal: &Path) -> Result<(), String> {
    let comparison = eval::compare::compare(&read_record(baseline)?, &read_record(proposal)?)
        .map_err(|error| error.to_string())?;
    print!("{comparison}");
    Ok(())
}
fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Run(args) => run_prompts(args),
        Command::Compare { baseline, proposal } => compare_records(&baseline, &proposal),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
