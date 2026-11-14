//! `cargo eval`: runs the prompt set against a model and writes the run record, or compares two.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use eval::compiler::Compiler;
use eval::conversation::Target;
use eval::model::Model;
use eval::model::anthropic::Anthropic;
use eval::model::replay::Replay;
use eval::prompts::PromptSet;
use eval::record::{Outcome, PromptRecord, RunRecord};
use eval::run::{Settings, run};

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
    #[arg(long, value_parser = ["java", "bedrock"])]
    edition: String,
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
    let line = match record.outcome {
        Outcome::Passed => format!("passed on attempt {attempts}"),
        Outcome::Failed => format!("failed after {attempts} attempts"),
        Outcome::Skipped => format!(
            "skipped: `{}` did not converge",
            record.base.as_deref().unwrap_or_default()
        ),
    };
    eprintln!("{}: {line}", record.id);
}

fn run_prompts(args: RunArgs) -> Result<(), String> {
    let mut prompts = PromptSet::load(&args.prompts).map_err(|error| error.to_string())?;
    if !args.only.is_empty() {
        prompts = prompts
            .select(&args.only)
            .map_err(|error| error.to_string())?;
    }
    let mut model: Box<dyn Model> = match &args.replay {
        Some(path) => Box::new(Replay::load(path).map_err(|error| error.to_string())?),
        None => Box::new(
            Anthropic::from_env(args.model.clone(), args.max_tokens)
                .map_err(|error| error.to_string())?,
        ),
    };
    let binary = match args.cairn {
        Some(binary) => binary,
        None => Compiler::build().map_err(|error| error.to_string())?,
    };
    let compiler = Compiler::new(
        binary,
        Target {
            edition: args.edition,
            version: args.target,
        },
    );
    let settings = Settings {
        model: args.model,
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
    let json = serde_json::to_string_pretty(&record).expect("a run record serializes") + "\n";
    match args.out {
        Some(path) => fs::write(&path, json)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?,
        None => std::io::stdout()
            .write_all(json.as_bytes())
            .map_err(|error| format!("cannot write the record: {error}"))?,
    }
    let metrics = &record.metrics;
    eprintln!(
        "zero-shot {}/{}, converged {}/{}, edits measured {}/{}, {} requests",
        metrics.zero_shot_compile_rate.passed,
        metrics.zero_shot_compile_rate.of,
        metrics.fix_convergence_rate.passed,
        metrics.fix_convergence_rate.of,
        metrics.edit_stability.measured,
        metrics.edit_stability.of,
        metrics.requests
    );
    Ok(())
}

fn read_record(path: &PathBuf) -> Result<RunRecord, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is not a run record: {error}", path.display()))
}

fn compare_records(baseline: &PathBuf, proposal: &PathBuf) -> Result<(), String> {
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
