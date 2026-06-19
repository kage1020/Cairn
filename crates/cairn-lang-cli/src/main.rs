//! Cairn command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{VersionAxes, compute_axes, resolve};
use cairn_lang_core::{Severity, check, lower, parse};
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
    /// Run syntactic validation passes against a .crn source file. Exits 0
    /// when nothing is reported, 1 when any `Error`-severity diagnostic is
    /// emitted (or the file fails to parse), 2 when the file cannot be
    /// located.
    Check {
        /// Path to the .crn file to check.
        file: PathBuf,
        /// Output format for the diagnostics.
        #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
        format: CheckFormat,
    },
    /// Report the three version axes (registry-compatible range, edition
    /// portability, semantic-sensitive members) for a .crn source file.
    /// Exits 0 on success, 1 on parse failure or any other I/O error
    /// (permission denied, non-UTF-8 contents), 2 when the file cannot be
    /// located, and rejects an empty `--editions` value with exit 2.
    Info {
        /// Path to the .crn file to inspect.
        file: PathBuf,
        /// Comma-separated editions to evaluate portability against. Each
        /// edition produces one entry in the output's `edition portability`
        /// section.
        #[arg(long, value_delimiter = ',', default_values_t = vec!["java".to_owned(), "bedrock".to_owned()])]
        editions: Vec<String>,
        /// Output format for the report.
        #[arg(long, value_enum, default_value_t = InfoFormat::Text)]
        format: InfoFormat,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    /// Pretty JSON (default; matches future programmatic consumers).
    Json,
    /// Rust `{:#?}` debug formatting (developer-facing).
    Debug,
}

#[derive(Copy, Clone, ValueEnum)]
enum CheckFormat {
    /// gcc-style one-diagnostic-per-line for humans (default).
    Text,
    /// Pretty JSON list, for tools.
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
enum InfoFormat {
    /// Multi-line human report mirroring `spec/versioning-editions.md` §10.5.
    Text,
    /// Pretty JSON serialisation of `VersionAxes`, for tools.
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Parse { file, format }) => run_parse(&file, format),
        Some(Command::Check { file, format }) => run_check(&file, format),
        Some(Command::Info {
            file,
            editions,
            format,
        }) => run_info(&file, &editions, format),
        None => {
            eprintln!("error: a subcommand is required (try `cairn --help`)");
            ExitCode::from(2)
        }
    }
}

fn run_parse(file: &Path, format: Format) -> ExitCode {
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

fn run_check(file: &Path, format: CheckFormat) -> ExitCode {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: cannot read `{}`: {err}", file.display());
            return match err.kind() {
                std::io::ErrorKind::NotFound => ExitCode::from(2),
                _ => ExitCode::from(1),
            };
        }
    };
    // A parse failure pre-empts any check pass — the AST/IR has to be
    // well-formed before invariant-collecting can run. Surface it under the
    // same exit code as a check-level error so a CI pipeline gating on
    // `cairn check` does not silently pass a file that the parser rejected.
    let module = match parse(&source) {
        Ok(m) => m,
        Err(err) => {
            eprintln!(
                "error: {}:{}: {}",
                file.display(),
                err.position(),
                err.user_message(),
            );
            return ExitCode::from(1);
        }
    };
    let ir = lower(&module);
    let diagnostics = check(&module, &ir);
    let has_error = diagnostics.iter().any(|d| d.severity == Severity::Error);
    // Build the line-start index once and reuse it for every diagnostic /
    // note position lookup. Without this we'd re-walk the entire source for
    // each position computation, which gets expensive when a single file
    // produces many diagnostics (e.g. a registry pack ingest run).
    let lines = LineStarts::new(&source);

    match format {
        CheckFormat::Text => {
            for d in &diagnostics {
                let pos = lines.position(&source, d.span.start);
                println!(
                    "{}:{}: {}[{}]: {}",
                    file.display(),
                    pos,
                    d.severity.as_str(),
                    d.code.as_str(),
                    d.primary,
                );
                for note in &d.notes {
                    if let Some(span) = note.span.as_ref() {
                        let note_pos = lines.position(&source, span.start);
                        println!("{}:{}:   note: {}", file.display(), note_pos, note.message);
                    } else {
                        // Informational note with no distinct secondary
                        // location — indent without a file:L:C prefix so the
                        // output doesn't read as a second pointer at the
                        // primary span.
                        println!("  note: {}", note.message);
                    }
                }
            }
        }
        CheckFormat::Json => {
            // Render to the `RenderedDiagnostic` form so the JSON output
            // carries `line` / `col` / `end_line` / `end_col` — without
            // this the `--format json` contract for downstream tooling
            // would ship only `code` / `severity` / `primary` / `notes`,
            // with no source position at all.
            let rendered: Vec<_> = diagnostics
                .iter()
                .map(|d| d.render(&source, &lines))
                .collect();
            match serde_json::to_string_pretty(&rendered) {
                Ok(json) => println!("{json}"),
                Err(err) => {
                    eprintln!("error: failed to serialise diagnostics as JSON: {err}");
                    return ExitCode::from(1);
                }
            }
        }
    }

    if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_info(file: &Path, editions: &[String], format: InfoFormat) -> ExitCode {
    // Reject empty edition entries early so they cannot leak into the
    // output. `--editions ""` and `--editions java,,bedrock` both produce
    // empty strings under the comma value-delimiter, which would render
    // as `: 1.20 .. latest` rows or `"edition":""` JSON.
    if editions.iter().any(|e| e.trim().is_empty()) {
        eprintln!("error: --editions value must not contain empty entries");
        return ExitCode::from(2);
    }

    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: cannot read `{}`: {err}", file.display());
            return match err.kind() {
                std::io::ErrorKind::NotFound => ExitCode::from(2),
                _ => ExitCode::from(1),
            };
        }
    };
    let module = match parse(&source) {
        Ok(m) => m,
        Err(err) => {
            eprintln!(
                "error: {}:{}: {}",
                file.display(),
                err.position(),
                err.user_message(),
            );
            return ExitCode::from(1);
        }
    };
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let axes = compute_axes(&module, &ir, &resolution, editions);

    match format {
        InfoFormat::Text => {
            print_text(&axes);
            ExitCode::SUCCESS
        }
        InfoFormat::Json => match serde_json::to_string_pretty(&axes) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: failed to serialise version axes as JSON: {err}");
                ExitCode::from(1)
            }
        },
    }
}

fn print_text(axes: &VersionAxes) {
    // Axis 1: the registry-compatible range is currently edition-agnostic
    // — `RegistryRange` holds a single `min/max` pair. The output renders
    // it as one entry to match. When the registry pack lands (2026.12.0)
    // and the range becomes per-edition, this is the line that grows a
    // per-edition list to mirror axis 2.
    println!(
        "registry compatibility:  {} .. {}",
        axes.registry_compat.min, axes.registry_compat.max,
    );

    let portability_line = if axes.edition_portability.is_empty() {
        String::from("(no editions requested)")
    } else {
        axes.edition_portability
            .iter()
            .map(|ep| {
                format!(
                    "{}: portable: {}  degraded: {}  unsupported: {}",
                    capitalise(&ep.edition),
                    ep.portable,
                    ep.degraded,
                    ep.unsupported,
                )
            })
            .collect::<Vec<_>>()
            .join("   ")
    };
    println!("edition portability:     {portability_line}");

    let semantic_line = if axes.semantic_sensitive.is_empty() {
        String::from("(none)")
    } else {
        axes.semantic_sensitive
            .iter()
            .map(|f| format!("{}({} @{})", f.member, f.reason, f.boundary_version))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("semantic-sensitive:      {semantic_line}");
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
