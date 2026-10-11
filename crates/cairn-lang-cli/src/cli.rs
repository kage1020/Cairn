//! The argument surface: the subcommands, their flags and the values those flags take.

use std::path::PathBuf;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::Edition;
use clap::{Parser, Subcommand, ValueEnum};

use crate::report::FailureReport;

/// `cairn` — Minecraft build DSL command-line interface.
#[derive(Parser)]
#[command(
    name = "cairn",
    version = CAIRN_VERSION,
    about = "Compile .crn build descriptions to Minecraft NBT and back",
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Lex and parse a .crn source file, printing the resulting AST.
    Parse {
        /// Path to the .crn file to parse.
        file: PathBuf,
        /// Output format for the AST.
        #[arg(long, value_enum, default_value_t = ParseFormat::Json)]
        format: ParseFormat,
    },
    /// Run the validation passes against a .crn source file and print its diagnostics.
    Check {
        /// Path to the .crn file to check.
        file: PathBuf,
        /// Edition to check against; omitted, slots resolve across both editions' theme variants.
        #[arg(long, value_enum)]
        edition: Option<EditionArg>,
        /// Minecraft version to check block ids against, resolved as `compile --target` is.
        // No `latest` default: it would refuse ids on a version nobody chose, the guess
        // `spec/versioning-editions` "Fail-loud and minimum-version inference" rules out.
        #[arg(long, requires = "edition")]
        target: Option<String>,
        /// Output format for the diagnostics.
        #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
        format: CheckFormat,
    },
    /// Report a .crn source file's version range, edition portability and semantic-sensitive
    /// members.
    Info {
        /// Path to the .crn file to inspect.
        file: PathBuf,
        /// Comma-separated editions to evaluate portability against.
        #[arg(long, value_delimiter = ',', default_values_t = vec!["java".to_owned(), "bedrock".to_owned()])]
        editions: Vec<String>,
        /// Output format for the report.
        #[arg(long, value_enum, default_value_t = InfoFormat::Text)]
        format: InfoFormat,
    },
    /// Lower a .crn source file all the way to the block-array IR and print the result.
    Lower {
        /// Path to the .crn file to lower.
        file: PathBuf,
        /// Output format for the lowered block-array IR.
        #[arg(long, value_enum, default_value_t = LowerFormat::Ascii)]
        format: LowerFormat,
    },
    /// Compile a .crn source file to its structure artifacts and write its lockfile.
    Compile {
        /// Path to the .crn file to compile.
        file: PathBuf,
        /// Target edition.
        #[arg(long, value_enum)]
        edition: EditionArg,
        /// Minecraft version to compile for, resolved against the edition's data table.
        #[arg(long, default_value = "latest")]
        target: String,
        /// Output directory for the structure files; defaults to the source file's directory.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Lockfile path; defaults to `<source>.lock` next to the source.
        #[arg(long)]
        lock: Option<PathBuf>,
    },
    /// Experimental: print one stage of a .crn source file's redstone pipeline as JSON.
    ///
    /// The `edition`, `placement`, `route`, `delay`, and `crossing` stages need `--edition`; the
    /// earlier stages are edition-neutral.
    Synth {
        /// Path to the .crn file to synthesise.
        file: PathBuf,
        /// Confirm that this subcommand's output is internal and may change at any time.
        #[arg(long)]
        experimental_logic_synth: bool,
        /// Which pipeline stage to print.
        #[arg(long, value_enum, default_value_t = SynthStage::Logic)]
        stage: SynthStage,
        /// Required when `--stage edition`, `--stage placement`, `--stage route`, `--stage delay`,
        /// or `--stage crossing` is set, and refused for `logic` and `netlist`.
        #[arg(long, value_enum)]
        edition: Option<EditionArg>,
    },
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum SynthStage {
    /// Edition-neutral Logic IR DAG.
    Logic,
    /// Netlist IR: Logical Cell selection over the Logic IR.
    Netlist,
    /// Edition Netlist IR: Edition Cell selection over the Netlist IR.
    Edition,
    /// Placement IR: coordinate assignment over the Edition Netlist IR.
    Placement,
    /// Routed Placement IR: Steiner routing, filling each cell's `wire_length`.
    Route,
    /// Delayed Placement IR: delay insertion, filling each cell's `local_delay_ticks`.
    Delay,
    /// Legalized Placement IR: crossing legalization, filling each cell's `buffer_coords`.
    Crossing,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum ParseFormat {
    /// Pretty JSON (default).
    Json,
    /// Rust `{:#?}` debug formatting (developer-facing).
    Debug,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum CheckFormat {
    /// gcc-style one-diagnostic-per-line for humans (default).
    Text,
    /// Pretty JSON list, for tools.
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum InfoFormat {
    /// Multi-line report for humans.
    Text,
    /// Pretty JSON serialisation of `VersionAxes`, for tools.
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum EditionArg {
    /// Java Edition: gzip `.nbt` structures.
    Java,
    /// Bedrock Edition: uncompressed little-endian `.mcstructure` files.
    Bedrock,
}

impl From<EditionArg> for Edition {
    fn from(arg: EditionArg) -> Self {
        match arg {
            EditionArg::Java => Edition::Java,
            EditionArg::Bedrock => Edition::Bedrock,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum LowerFormat {
    /// Per-structure ASCII Y-slices plus a palette listing (default).
    Ascii,
    /// Pretty JSON serialisation of `BlockArrayIr`, for tools.
    Json,
    /// Rust `{:#?}` debug formatting (developer-facing).
    Debug,
}

impl ParseFormat {
    pub(crate) fn failure_report(self) -> FailureReport {
        match self {
            Self::Json => FailureReport::Json,
            Self::Debug => FailureReport::Text,
        }
    }
}

impl InfoFormat {
    pub(crate) fn failure_report(self) -> FailureReport {
        match self {
            Self::Json => FailureReport::Json,
            Self::Text => FailureReport::Text,
        }
    }
}

impl LowerFormat {
    pub(crate) fn failure_report(self) -> FailureReport {
        match self {
            Self::Json => FailureReport::Json,
            Self::Ascii | Self::Debug => FailureReport::Text,
        }
    }
}
