//! Cairn command-line entry point.

use std::path::PathBuf;
use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::parse;
use clap::{Parser, Subcommand, ValueEnum};

/// `cairn` — Minecraft build DSL command-line interface.
#[derive(Parser)]
#[command(
    name = "cairn",
    version = CAIRN_VERSION,
    about = "Compile .crn build descriptions to Minecraft NBT and back",
)]
struct Cli {
    /// Subcommand to dispatch.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Lex and parse a .crn source file, printing the resulting AST.
    Parse {
        /// Path to the .crn file to parse.
        file: PathBuf,
        /// Output format for the AST.
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    /// Pretty JSON (default; matches future programmatic consumers).
    Json,
    /// Rust `{:#?}` debug formatting (developer-facing).
    Debug,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Parse { file, format }) => run_parse(&file, format),
        None => {
            eprintln!("error: a subcommand is required (try `cairn --help`)");
            ExitCode::from(2)
        }
    }
}

fn run_parse(file: &std::path::Path, format: Format) -> ExitCode {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: cannot read `{}`: {err}", file.display());
            // `NotFound` is a user-input mistake (wrong path) → exit 2;
            // everything else (permission denied, non-UTF-8 file contents,
            // I/O failure) signals a build/system problem → exit 1.
            return match err.kind() {
                std::io::ErrorKind::NotFound => ExitCode::from(2),
                _ => ExitCode::from(1),
            };
        }
    };
    let module = match parse(&source) {
        Ok(m) => m,
        Err(err) => {
            // gcc/clang style `file:line:col:` so editors can jump.
            let position = err.position();
            eprintln!(
                "error: {}:{}: {}",
                file.display(),
                position,
                err.user_message(),
            );
            return ExitCode::from(1);
        }
    };
    match format {
        Format::Json => match serde_json::to_string_pretty(&module) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: failed to serialise AST as JSON: {err}");
                ExitCode::from(1)
            }
        },
        Format::Debug => {
            println!("{module:#?}");
            ExitCode::SUCCESS
        }
    }
}
