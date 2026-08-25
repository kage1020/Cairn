//! Cairn command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::{DiagnosticNote as Note, LineStarts};
use cairn_lang_core::lock::{
    HashHex, LOCK_SCHEMA_VERSION, LockEdition, LockError, LockInputs, LockPlacement, LockTarget,
    LockWalkway, Lockfile, hash_resolved_ir, hash_source,
};
use cairn_lang_core::resolve::{
    BuildableTargets, EditionReport, UnsupportedEntry, UnsupportedReason, VersionAxes,
    VersionFloor, compare_versions, compute_axes, declared_version_floor, resolve,
};
use cairn_lang_core::{Edition, Severity, check, lower, parse};
use cairn_lang_formats::bedrock_structure::{ParityNote, build_mcstructure_tag, write_mcstructure};
use cairn_lang_formats::data_version::{
    BedrockTarget, JavaTarget, resolve_bedrock_target, resolve_java_target,
};
use cairn_lang_formats::java_structure::{
    Compound, OutputExt, build_structure_tag, output_filename, write_compound_gzip,
};
use cairn_lang_formats::portability::{portability_for_bedrock, portability_for_java};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};
use cairn_lang_redstone::{
    PlacementStage, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};
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
    ///
    /// This command does not run block-array lowering, so no lowering-stage
    /// finding reaches it — `E_UNKNOWN_ID` and `E_UNKNOWN_ABSTRACT_TOKEN`
    /// among them. `cairn compile` runs both stages and is the gate that
    /// sees every code.
    Check {
        /// Path to the .crn file to check.
        file: PathBuf,
        /// Optional edition pin. When set, per-edition theme variants
        /// (spec versioning-editions §10.7) are resolved for the picked
        /// edition specifically — a `mat_slot=X` reference to a slot only
        /// the *other* variant declares fires `E_UNRESOLVED_SLOT`. When
        /// omitted, the resolver unions slot names across both variants
        /// of one logical theme so the file passes `check` regardless of
        /// which edition it ends up compiling for.
        #[arg(long, value_enum)]
        edition: Option<EditionArg>,
        /// Output format for the diagnostics.
        #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
        format: CheckFormat,
    },
    /// Report the three version axes (registry-compatible range, edition
    /// portability, semantic-sensitive members) for a .crn source file.
    /// Exits 0 on success; 1 on a parse failure, on any `Error`-severity
    /// diagnostic (the check passes run here, and a range derived from a
    /// file `cairn check` rejects is a confident wrong answer), or on any
    /// other I/O error (permission denied, non-UTF-8 contents); 2 when the
    /// file cannot be located, and rejects an empty `--editions` value with
    /// exit 2.
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
    /// Lower a .crn source file all the way to the block-array IR and print
    /// the result. A debugging surface for the universal voxel pivot;
    /// `cairn compile` writes the same IR out as a Java `.nbt` artifact.
    /// Lowering warnings (deferred members, themeless scopes, abstract
    /// tokens) print to stderr but do not affect the exit code. Exits 0 on
    /// success, 1 on a parse failure, on any `Error`-severity diagnostic
    /// (`E_UNKNOWN_ABSTRACT_TOKEN` among them; `E_UNKNOWN_ID` is not, since
    /// nothing here pins a target to check ids against), or on an I/O
    /// error, and 2 when the file cannot be located.
    Lower {
        /// Path to the .crn file to lower.
        file: PathBuf,
        /// Output format for the lowered block-array IR.
        #[arg(long, value_enum, default_value_t = LowerFormat::Ascii)]
        format: LowerFormat,
    },
    /// Compile a .crn source file to its edition+version-pinned structure
    /// artifact set and write a lockfile next to the source. `--edition
    /// java` writes gzip `.nbt` structures; `--edition bedrock` writes
    /// uncompressed `.mcstructure` files, translating the blockstate
    /// families it knows into Bedrock `states`; a property it cannot
    /// translate is a hard error, and intent it can only approximate (stair
    /// `shape`) is dropped with a `W_INTENT_DEGRADED` warning rather than
    /// silently. This is also the only command that checks block ids against a
    /// registry (`E_UNKNOWN_ID`): `--target` pins the one version there is
    /// an answer for.
    /// Exits 0 on success, 1 on parse, lowering, or I/O failure (including
    /// an unsupported `--target` or a Bedrock property with no `states`
    /// translation), and 2
    /// when the source file cannot be located.
    Compile {
        /// Path to the .crn file to compile.
        file: PathBuf,
        /// Target edition. Required by spec §4.2 (`--target` alone is
        /// forbidden).
        #[arg(long, value_enum)]
        edition: EditionArg,
        /// Minecraft version string. Resolved against the backend's data
        /// table; opaque label per spec §10.1. `latest` aliases the newest
        /// version the backend knows about.
        #[arg(long, default_value = "latest")]
        target: String,
        /// Output directory for the generated `.nbt` files. Created if
        /// missing. Defaults to the source file's parent directory.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Lockfile path. Defaults to `<source>.lock` next to the source
        /// (so `cottage.crn` → `cottage.crn.lock`), keeping per-source
        /// locks unambiguous when several `.crn` files share an output
        /// directory.
        #[arg(long)]
        lock: Option<PathBuf>,
    },
    /// Lower a .crn source file's `logic` bindings, sensors, and actuators
    /// through the redstone pipeline and print an intermediate stage as
    /// JSON. `--stage logic` (default) prints the edition-neutral Logic IR
    /// DAG; `--stage netlist` prints the Netlist IR of Logical Cells + nets
    /// derived from that DAG; `--stage edition` picks the target-edition
    /// realisation of each cell and prints the Edition Netlist IR;
    /// `--stage placement` lays those edition-tagged cells out inside
    /// each scope's `circuit region=` reservation and prints the
    /// Placement IR; `--stage route` runs Steiner routing over the
    /// Placement IR and prints the routed layout with every cell's
    /// `wire_length` populated; `--stage delay` runs delay insertion
    /// over the routed IR and fills every cell's `delay_ticks` with
    /// the sum of the cell's base delay and each implicit buffer
    /// repeater's `BUFFER_REPEATER_TICKS` contribution over every
    /// driving net's segment beyond the `DUST_ATTENUATION_LIMIT`;
    /// `--stage crossing` runs crossing legalization over the delayed
    /// IR, reports every pair of nets sharing a wire coord — with
    /// `W_WIRE_CROSSING`, or with `E_CROSSING_CONGESTION` when
    /// `void=<N>` is under 2 and the reservation has no y-layer above
    /// the plane a lift could ever go on (v1 lifts no wire either
    /// way; the height decides whether raising `void=` could change
    /// that, not whether the two signals merge), and fills every
    /// cell's `buffer_coords` with the coord of the buffer repeater
    /// each driver segment passes through (escaping to a
    /// `RouteLayer::Bridge` y-layer whenever the plane candidate
    /// collides with a cell / pad / another net's wire; a repeater
    /// this net already placed is recorded rather than escaped
    /// around, so one block can be named by several segments). Every cell of the four
    /// Placement IR stages carries a
    /// `"stage"` key echoing the flag value that produced the dump
    /// (`placement` / `route` / `delay` / `crossing`), so a consumer
    /// reads the stage off the output instead of inferring it from
    /// which optional keys are present. The `--edition <java|bedrock>`
    /// flag is required in the `edition`, `placement`, `route`,
    /// `delay`, and `crossing` modes and refused otherwise (the
    /// earlier stages are edition-neutral by contract).
    /// **Internal / experimental** — the shape of the output is not
    /// covered by the stable compatibility tier and may change at any time
    /// as the route / simulator stages land. Requires
    /// `--experimental-logic-synth` so a caller cannot end up depending on
    /// it accidentally.
    ///
    /// Exits 0 when the requested stage produced a well-formed IR
    /// (warnings still allowed), 1 on parse failure, I/O error, or any
    /// Error-severity synth diagnostic, and 2 when the file cannot be
    /// located.
    Synth {
        /// Path to the .crn file to synthesise.
        file: PathBuf,
        /// Opt-in flag confirming the caller understands this surface is
        /// internal. Required until the redstone pipeline reaches a stable
        /// tier; without it the subcommand exits 2 with a hint.
        #[arg(long)]
        experimental_logic_synth: bool,
        /// Which pipeline stage to print. Adding stages here as they
        /// land is preferred over a new subcommand per stage — it keeps
        /// the internal surface area (and `--help` output) contained.
        /// For the four Placement IR stages the value chosen here is
        /// echoed back as every cell's `"stage"` key, so the flag and
        /// the dump share one vocabulary
        /// ([`cairn_lang_redstone::PlacementStage`]).
        #[arg(long, value_enum, default_value_t = SynthStage::Logic)]
        stage: SynthStage,
        /// Target edition for the Edition Netlist IR / Placement IR /
        /// routed Placement IR / delayed Placement IR / legalized
        /// Placement IR. Required when `--stage edition`,
        /// `--stage placement`, `--stage route`, `--stage delay`, or
        /// `--stage crossing` is set; refused for `logic` / `netlist`,
        /// which are edition-neutral by contract.
        #[arg(long, value_enum)]
        edition: Option<EditionArg>,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum SynthStage {
    /// Edition-neutral Logic IR DAG produced by `synthesize`.
    Logic,
    /// Netlist IR: Logical Cell selection over the Logic IR DAG. Still
    /// carries no delay per `spec/redstone` "Time model" / "Connection to
    /// the IR and phases".
    Netlist,
    /// Edition Netlist IR: Edition Cell selection over the Netlist IR
    /// against `--edition`. The middle tier of `spec/redstone` §14.6's
    /// three-tier cell library. Still carries no delay.
    Edition,
    /// Placement IR: 1D coordinate assignment over the Edition Netlist
    /// IR against `--edition`. Stage 1 of `spec/redstone` §14.5's
    /// place-and-route pipeline. `wire_length` and `delay_ticks` are
    /// reserved as `Option`s and stay `None` until the routing and
    /// delay-insertion follow-up passes land.
    Placement,
    /// Routed Placement IR: Steiner routing over the Placement IR
    /// against `--edition`. Stage 2 of `spec/redstone` §14.5's
    /// place-and-route pipeline. Fills every cell's `wire_length`
    /// with the sum, over the nets driving it, of the routed length
    /// from that net's source into the cell; `delay_ticks` stays
    /// `None` until the delay-insertion pass (stage 3) runs.
    Route,
    /// Delayed Placement IR: delay insertion over the routed Placement
    /// IR against `--edition`. Stage 3 of `spec/redstone` §14.5's
    /// place-and-route pipeline. Fills every cell's `delay_ticks`
    /// with the sum of the cell's physical base delay
    /// ([`cairn_lang_redstone::EditionCell::base_delay_ticks`]) and
    /// each implicit buffer repeater's
    /// [`cairn_lang_redstone::BUFFER_REPEATER_TICKS`] contribution
    /// implied by each driving net's segment beyond
    /// [`cairn_lang_redstone::DUST_ATTENUATION_LIMIT`]; refuses with
    /// `E_ATTENUATION_LIMIT` when a segment exceeds the v1 sanity cap
    /// [`cairn_lang_redstone::MAX_ATTENUATION_SEGMENT`], the threshold
    /// past which a stage-4 crossing-legalization escape becomes
    /// unavoidable.
    Delay,
    /// Legalized Placement IR: crossing legalization over the delayed
    /// Placement IR against `--edition`. Stage 4 of `spec/redstone`
    /// §14.5's place-and-route pipeline. Detects wire coords two
    /// distinct nets share — on the ground plane or on a bridge layer
    /// they both climbed to — and reports one finding per pair, as
    /// `W_WIRE_CROSSING` or, when `void=<N>` is under 2 and the
    /// `circuit region=<label> void=<N>` reservation has no y-layer
    /// above the plane at all, as `E_CROSSING_CONGESTION`. The test is
    /// whether such a layer exists, not how many crossings would share
    /// it. It also materialises the coord of every implicit buffer
    /// repeater
    /// the delay pass counted into every cell's `buffer_coords`, one
    /// entry per driver segment that passes through it — so a block
    /// serving several segments is named once per segment and a
    /// consumer counting blocks deduplicates by coord. A buffer whose
    /// plane candidate collides with a cell / pad / another net's wire
    /// escapes to the first free `RouteLayer::Bridge` y-layer inside
    /// the `void=<N>` budget; a repeater this net has already placed
    /// on the candidate, or lifted off it, is recorded instead. If
    /// every bridge y-layer at that `(x, z)` is taken, refuses with
    /// `E_BUFFER_COORD_COLLISION`. v1 does not lift the wire
    /// crossing itself onto `Bridge` — the routed wire path is not
    /// carried on the IR, so an escape record would have nowhere to
    /// attach, and nothing downstream reads the crossing set. A scope
    /// with nothing to legalize emits no `buffer_coords` at all
    /// (the empty vector serde-skips); the `"stage": "crossing"` tag
    /// on every cell, not the presence of that key, is what marks the
    /// dump as having been through this pass.
    Crossing,
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

#[derive(Copy, Clone, ValueEnum)]
enum EditionArg {
    /// Java Edition. Emits a gzip-compressed vanilla `.nbt` structure.
    Java,
    /// Bedrock Edition. Emits an uncompressed little-endian `.mcstructure`;
    /// the stair family's blockstate is mapped to Bedrock `states` and
    /// unrepresentable intent (stair `shape`) degrades with a
    /// `W_INTENT_DEGRADED` warning.
    Bedrock,
}

impl EditionArg {
    fn as_lock_edition(self) -> LockEdition {
        match self {
            EditionArg::Java => LockEdition::Java,
            EditionArg::Bedrock => LockEdition::Bedrock,
        }
    }

    /// Convert to the core-crate [`Edition`] marker so the resolver's
    /// per-edition theme-variant selection sees the same value the compile
    /// backend does.
    fn as_edition(self) -> Edition {
        match self {
            EditionArg::Java => Edition::Java,
            EditionArg::Bedrock => Edition::Bedrock,
        }
    }

    /// The built-in registry pack this edition compiles against, whose
    /// version table is the closed set of `--target` values.
    fn registry_pack(self) -> &'static RegistryPack {
        match self {
            EditionArg::Java => builtin_java(),
            EditionArg::Bedrock => builtin_bedrock(),
        }
    }

    /// Lowercase name, matching the `--edition` spelling and the label
    /// `UnsupportedTarget` uses, so one message never calls it two things.
    fn as_str(self) -> &'static str {
        match self {
            EditionArg::Java => "java",
            EditionArg::Bedrock => "bedrock",
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum LowerFormat {
    /// Per-structure ASCII Y-slice plus a palette listing (default;
    /// easiest way to eyeball whether the walls came out right).
    Ascii,
    /// Pretty JSON serialisation of `BlockArrayIr`, for tools.
    Json,
    /// Rust `{:#?}` debug formatting (developer-facing).
    Debug,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Parse { file, format }) => run_parse(&file, format),
        Some(Command::Check {
            file,
            edition,
            format,
        }) => run_check(&file, edition, format),
        Some(Command::Info {
            file,
            editions,
            format,
        }) => run_info(&file, &editions, format),
        Some(Command::Lower { file, format }) => run_lower(&file, format),
        Some(Command::Synth {
            file,
            experimental_logic_synth,
            stage,
            edition,
        }) => run_synth(&file, experimental_logic_synth, stage, edition),
        Some(Command::Compile {
            file,
            edition,
            target,
            out,
            lock,
        }) => run_compile(&file, edition, &target, out.as_deref(), lock.as_deref()),
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

fn run_check(file: &Path, edition: Option<EditionArg>, format: CheckFormat) -> ExitCode {
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
    let diagnostics = check(&module, &ir, edition.map(EditionArg::as_edition));
    let has_error = diagnostics.iter().any(|d| d.severity() == Severity::Error);
    // Build the line-start index once and reuse it for every diagnostic /
    // note position lookup. Without this we'd re-walk the entire source for
    // each position computation, which gets expensive when a single file
    // produces many diagnostics (e.g. a registry pack ingest run).
    let lines = LineStarts::new(&source);

    match format {
        // Diagnostics are the report, not the product. Every other
        // subcommand sends theirs to stderr, and `check` sending its text
        // form to stdout meant `cairn check f.crn > out` swallowed them
        // while still exiting 1 — a CI step that captured stdout for
        // something else saw a bare exit code and no reason.
        //
        // `--format json` stays on stdout: that one *is* the product, and a
        // consumer redirects it deliberately.
        CheckFormat::Text => {
            for d in &diagnostics {
                let pos = lines.position(&source, d.span.start);
                eprintln!(
                    "{}:{}: {}[{}]: {}",
                    file.display(),
                    pos,
                    d.severity().as_str(),
                    d.code.as_str(),
                    d.primary,
                );
                report_notes(file, &source, &lines, &d.notes);
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
    // Reject unknown edition names before the (expensive) dry-run lowering.
    // The parity table's contract is "every entry is a real portability
    // figure"; letting an unknown edition through would either silently
    // produce zeros or a Java-flavoured fallback the caller couldn't
    // distinguish from a real portable-only classification.
    for e in editions {
        if let Err(err) = e.parse::<Edition>() {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
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

    // Surface the check pass (syntactic + resolver) and lowering
    // diagnostics before running the parity dry-run — the same contract
    // `run_check` / `run_lower` / `run_compile` honor. Without this, a
    // `.crn` carrying an `E_UNRESOLVED_SLOT` (or any other Error-severity
    // finding) would still get `cairn info` exit 0 with a portability row
    // computed against a partially unresolved IR, which is a poor CI gate.
    //
    // This gate is edition-neutral — `resolve(ir, None)` unions slot names
    // across per-edition variants — so a file whose only "problem" is that
    // one variant declares a slot the other does not still passes here.
    // The strict per-edition pass runs inside the dry-run below, which
    // reports whatever only it can see.
    let resolution = resolve(&ir, None);
    // `info` reports across every target in the pack's range, so there is
    // no single version to check ids against and the view carries no id
    // table. Guessing one would refuse ids that are fine on the version
    // the author compiles for; `cairn compile --target` is where the
    // question has an answer.
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().view(None)));
    let combined = build_diagnostics(
        &module,
        &ir,
        None,
        std::mem::take(&mut block_ir.diagnostics),
    );

    let lines = LineStarts::new(&source);
    let mut has_error = false;
    for d in &combined {
        let pos = lines.position(&source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity().as_str(),
            d.code.as_str(),
            d.primary,
        );
        report_notes(file, &source, &lines, &d.notes);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }
    if has_error {
        return ExitCode::from(1);
    }

    let floor = declared_version_floor(&module);
    let rows = match edition_rows(
        file,
        &source,
        &lines,
        &ir,
        editions,
        floor.as_ref(),
        &combined,
    ) {
        Ok(rows) => rows,
        Err(code) => return code,
    };

    let axes = compute_axes(&module, &ir, &resolution, rows);

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

/// One dry-run lower per requested edition, plus one per supported version
/// of it, returning the two per-edition rows.
///
/// The resolver's per-edition theme-variant selection can produce a
/// different palette per edition (the whole point of spec §10.7 hierarchy
/// #2), so a single shared block-array IR would misrepresent the parity
/// axis. Nothing is written to disk — the lowering stops at the in-memory
/// `BlockArrayIr` that `portability_for_*` inspects.
///
/// A finding only the strict per-edition pass produces is reported here and
/// turns into exit 1. Without that, a source `cairn compile --edition
/// bedrock` refuses with `E_UNRESOLVED_SLOT` was described as
/// `degraded: 0  unsupported: 0`, with the member that failed to resolve
/// visible only as a smaller `portable` count — indistinguishable from
/// "this edition simply has fewer structures". A parity report that cannot
/// show a parity failure is worse than none.
///
/// The version loop is here for the same reason one level down.
/// Portability asks of the edition, and two palette entries declared by
/// disjoint sets of versions each answer yes while no single version has
/// both. Asking each version in turn is the only sound answer, and it
/// cannot be approximated by intersecting the range-wide palette's id
/// sets: with no target pinned every material takes its *default*
/// mapping, so a token the target respells is compared as the wrong id.
/// A theme binding `@floor.stone.smooth` (default `stone_bricks`,
/// respelled `stonebrick` at Bedrock 1.21.0) beside a literal
/// `@stonebrick` has an empty intersection and builds on 1.21.0.
///
/// A version counts as buildable when it passes the gates
/// [`run_compile`] applies to the *source*: the pinned lowering raises no
/// error, the `@requires` floor is at or below it, and every scope the
/// source declares lowered. The last two do not depend on the version's id
/// table, but they decide whether a build happens, and a row that named a
/// target `compile` refuses would be the same defect this one exists to
/// remove. The gates after those are about the filesystem — an output
/// directory, a free lockfile path — and belong to the command that writes.
///
/// The loop reports and does not refuse. An entry no version of the
/// edition has is already a figure rather than a gate here — spec §10.5's
/// own sample output carries `unsupported: 1` — and a caller that wants a
/// refusal runs the build. What this row adds is the fact the counters
/// cannot carry: that the versions disagree about different entries.
///
/// Reporting and refusing are different, though, and a row that says
/// `none` without saying why is not a report. Each pinned lowering's
/// findings are printed under the version that raised them, because
/// nothing else in the run will ever show them; the floor and the dropped
/// scopes get one line each per edition instead, since their reason is the
/// same for every version and is already on screen — the compat row for
/// the first, the scope's own warning for the second.
///
/// The cost is one lowering per version per edition rather than one per
/// edition — three versions per edition in the built-in packs — and each
/// reuses the edition's single `resolve`, which is the expensive half.
///
/// `already_reported` is the edition-neutral stream the caller has printed;
/// only diagnostics absent from it are reported, keyed by code and span, so
/// the shared findings are not repeated once per edition.
fn edition_rows(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    ir: &cairn_lang_core::intent::IntentModule,
    editions: &[String],
    floor: Option<&VersionFloor>,
    already_reported: &[cairn_lang_core::check::Diagnostic],
) -> Result<Vec<EditionReport>, ExitCode> {
    let already: std::collections::HashSet<(&str, usize, usize)> = already_reported
        .iter()
        .map(|d| (d.code.as_str(), d.span.start, d.span.end))
        .collect();
    let mut rows: Vec<EditionReport> = Vec::with_capacity(editions.len());
    let mut edition_specific_error = false;

    for e in editions {
        let edition: Edition = e.parse().expect("validated by the caller");
        let resolution = resolve(ir, Some(edition));
        let pack = match edition {
            Edition::Java => builtin_java(),
            Edition::Bedrock => builtin_bedrock(),
        };
        // Same reason as the pass above: no single version, so the lowering
        // gets no id table and raises no `E_UNKNOWN_ID`. The portability
        // fold below still reads the pack's tables — it asks the wider
        // question "does this edition have the block at all", which the
        // whole range can answer.
        let block_ir = lower_to_block_array(ir, &resolution, Some(&pack.view(None)));

        let only_here: Vec<_> = resolution
            .diagnostics
            .iter()
            .chain(block_ir.diagnostics.iter())
            .filter(|d| !already.contains(&(d.code.as_str(), d.span.start, d.span.end)))
            .cloned()
            .collect();
        if !only_here.is_empty() {
            eprintln!("note: reported for --editions {}", edition.as_str());
            if report_core_diagnostics(file, source, lines, &only_here) {
                edition_specific_error = true;
            }
        }

        let portability = match edition {
            Edition::Java => portability_for_java(&block_ir, &pack.blocks),
            Edition::Bedrock => portability_for_bedrock(&block_ir, &pack.blocks),
        };
        for note in unsupported_notes(edition, portability.unsupported()) {
            eprintln!("{note}");
        }
        let dropped = dropped_scopes(&resolution, &block_ir);
        if !dropped.is_empty() {
            eprintln!(
                "note: no {} target can build this source: {} produced no voxels, and a partial \
                 build is not certified",
                edition.as_str(),
                dropped
                    .iter()
                    .map(|scope| format!("`{scope}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }

        let considered = supported_versions(pack);
        let verdicts = weigh_versions(ir, &resolution, pack, floor, &considered, &dropped);
        for (version, refusals) in &verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_core_diagnostics(file, source, lines, refusals);
        }
        if let Some(floor) = floor
            && !verdicts.below_floor.is_empty()
        {
            eprintln!(
                "note: {} {} {} below the `@requires version>={}` floor this file declares",
                edition.as_str(),
                verdicts.below_floor.join(", "),
                if verdicts.below_floor.len() == 1 {
                    "is"
                } else {
                    "are"
                },
                floor.version,
            );
        }

        rows.push(EditionReport {
            edition,
            portable: portability.counts().portable,
            degraded: portability.counts().degraded,
            unsupported: portability.counts().unsupported,
            unsupported_entries: portability.into_unsupported(),
            buildable: verdicts.buildable,
            considered,
        });
    }

    // Every requested edition is walked before returning, so one bad edition
    // does not hide a second one's findings.
    if edition_specific_error {
        return Err(ExitCode::from(1));
    }
    Ok(rows)
}

/// The notes naming the palette entries one edition has no form for, in
/// the order they print, under the figure that counts them.
///
/// Returned rather than printed so a test can read the whole block: the
/// header carries the figure the stdout row carries, and an assertion on
/// one line of stderr cannot see that.
///
/// The figure is `entries.len()` and that is not a second tally of the
/// row's: [`PortabilityReport`] raises `unsupported` only beside a push,
/// and its fields are private, so the length of the list it hands out is
/// the number the row prints.
///
/// Stderr, beside the other notes this command prints. The four stdout
/// rows are the text twin of the JSON's top level and what a reader greps;
/// a per-entry list is not the shape of a row, and a consumer that wants
/// these structured reads `edition_portability[].unsupported_entries`.
fn unsupported_notes(edition: Edition, entries: &[UnsupportedEntry]) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut notes = vec![format!(
        "note: what `unsupported: {}` counts on {}:",
        entries.len(),
        edition.as_str(),
    )];
    notes.extend(entries.iter().map(|entry| {
        format!(
            "  note: `{}` — {}",
            entry.id,
            unsupported_reason(&entry.reason)
        )
    }));
    notes
}

/// One entry's reason, as the clause that follows its id.
///
/// Four sentences for four repairs — change the material, wait for the
/// backend, fix the pack, edit the blockstate — which is what the single
/// figure they fold into cannot be read as. Each one ends on what the
/// reader can do, including the two nobody can do anything about, because
/// "nothing here is yours to fix" is itself the answer that stops them
/// looking.
fn unsupported_reason(reason: &UnsupportedReason) -> String {
    match reason {
        UnsupportedReason::AbsentFromEdition { suggestion } => {
            let absent = "no supported version of this edition declares the block";
            match suggestion {
                Some(suggestion) => format!("{absent}; did you mean `{suggestion}`?"),
                None => absent.to_owned(),
            }
        }
        // The edition is not what cannot express these — this backend is,
        // and only so far. Saying otherwise would send an author to
        // abandon a design that works, and leave the missing mapping
        // unreported because nobody was told there was one.
        UnsupportedReason::StatesUnmapped { states, mapped } => format!(
            "the edition has the block; this compiler maps states for {mapped} so far, so \
             `{states}` has no form here yet — bind the slot to a property-free material, or \
             build for the other edition"
        ),
        UnsupportedReason::StateValueUnexpected { key, value, valid } => format!(
            "`{key}={value}` is not a valid Java `{key}` (valid: {valid}); a registry pack is \
             expected to reject this and no pack schema can state a value domain yet, so it is \
             not yours to repair"
        ),
        // The one of the four the author can act on, and the error it
        // comes from says so — that `Fix:` is the reason this is reported
        // apart from the value case rather than with it.
        UnsupportedReason::StateKeyUnread { key, handled } => format!(
            "`{key}` is not a blockstate this compiler reads (it reads {handled}); remove it \
             from the source blockstate"
        ),
    }
}

/// Scopes the resolver recorded (`struct::NAME`, `site::SITE::PLACE`) that
/// the block-array pass did not turn into a structure, in resolver order.
///
/// `def::` keys are excluded: a def is a template and lowers to voxels only
/// through a `place` that instantiates it.
///
/// One definition for the two readers. `run_compile` refuses a build that
/// would leave any of these out, and `edition_rows` reports the same thing
/// as "no target can build this" — a second copy could drift into
/// disagreeing about which scopes count.
fn dropped_scopes(
    resolution: &cairn_lang_core::Resolution,
    block_ir: &BlockArrayIr,
) -> Vec<String> {
    resolution
        .scopes
        .keys()
        .filter(|key| !key.starts_with("def::"))
        .filter(|key| !block_ir.structures.contains_key(key.as_str()))
        .cloned()
        .collect()
}

/// Every version a pack declares, in ascending release order.
///
/// Sorted here rather than taken as written: `DataVersionTable` documents
/// its row order as informational, and this list is shown to a reader, so
/// reordering rows in a pack's JSON must not reorder the output.
fn supported_versions(pack: &RegistryPack) -> Vec<String> {
    let mut rows: Vec<&cairn_lang_formats::registry::DataVersionEntry> =
        pack.data_versions.versions.iter().collect();
    rows.sort_by_key(|entry| entry.data_version);
    rows.into_iter()
        .map(|entry| entry.mc_version.clone())
        .collect()
}

/// How each supported version of one edition answered.
#[derive(Debug, Default)]
struct VersionVerdicts {
    /// Versions a build would accept.
    buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, carrying them: the
    /// caller prints them because nothing else in the run will.
    refused: Vec<(String, Vec<cairn_lang_core::check::Diagnostic>)>,
    /// Versions below the `@requires` floor, which are refused without
    /// being lowered at all — the floor is a relation between the source
    /// and the target, and no id table changes it.
    below_floor: Vec<String>,
}

/// Weigh each supported version against the gates `run_compile` applies to
/// the source.
///
/// `dropped` is the edition's unlowered scopes: non-empty means every
/// version refuses, so none is lowered a second time to find that out.
fn weigh_versions(
    ir: &cairn_lang_core::intent::IntentModule,
    resolution: &cairn_lang_core::Resolution,
    pack: &RegistryPack,
    floor: Option<&VersionFloor>,
    considered: &[String],
    dropped: &[String],
) -> VersionVerdicts {
    let mut verdicts = VersionVerdicts::default();
    for version in considered {
        if floor.is_some_and(|floor| compare_versions(version, &floor.version).is_lt()) {
            verdicts.below_floor.push(version.clone());
            continue;
        }
        if !dropped.is_empty() {
            continue;
        }
        let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
        // Severity rather than a list of codes. `E_UNKNOWN_ID` is the only
        // finding pinning a target raises today, and naming it here would
        // keep reporting a version as buildable the day a second one
        // lands. Nothing is filtered against what the caller already
        // printed: this is reached only after the edition-neutral gate,
        // which returns on any Error, so everything already reported is a
        // warning and no warning refuses a version.
        let refusals: Vec<_> = pinned
            .diagnostics
            .into_iter()
            .filter(|d| d.severity() == Severity::Error)
            .collect();
        if refusals.is_empty() {
            verdicts.buildable.push(version.clone());
        } else {
            verdicts.refused.push((version.clone(), refusals));
        }
    }
    verdicts
}

fn print_text(axes: &VersionAxes) {
    // Axis 1: the registry-compatible range is currently edition-agnostic
    // — `RegistryRange` holds a single `min/max` pair. The output renders
    // it as one entry to match. Once registry-pack data makes the range
    // per-edition, this is the line that grows a per-edition list to
    // mirror axis 2.
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
                    capitalise(ep.edition.as_str()),
                    ep.portable,
                    ep.degraded,
                    ep.unsupported,
                )
            })
            .collect::<Vec<_>>()
            .join("   ")
    };
    println!("edition portability:     {portability_line}");

    let buildable_line = if axes.buildable_targets.is_empty() {
        String::from("(no editions requested)")
    } else {
        axes.buildable_targets
            .iter()
            .map(|bt| {
                format!(
                    "{}: {}",
                    capitalise(bt.edition.as_str()),
                    buildable_text(bt)
                )
            })
            .collect::<Vec<_>>()
            .join("   ")
    };
    println!("buildable targets:       {buildable_line}");

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

/// One edition's buildable versions, with the refusing ones named after
/// them.
///
/// The refusals are in the sentence rather than left to be inferred: a bare
/// `1.21.0` cannot be told apart from an edition whose pack declares only
/// that one version, and the difference between "one target of three" and
/// "the only target there is" is the whole point of the row.
fn buildable_text(targets: &BuildableTargets) -> String {
    let refused: Vec<&str> = targets
        .considered
        .iter()
        .filter(|version| !targets.buildable.contains(version))
        .map(String::as_str)
        .collect();
    let built = if targets.buildable.is_empty() {
        String::from("none")
    } else {
        targets.buildable.join(", ")
    };
    if refused.is_empty() {
        return built;
    }
    let tail = if let [only] = refused.as_slice() {
        format!("{only} refuses")
    } else if targets.buildable.is_empty() {
        format!("{} all refuse", refused.join(", "))
    } else {
        format!("{} refuse", refused.join(", "))
    };
    format!("{built} ({tail})")
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn run_lower(file: &Path, format: LowerFormat) -> ExitCode {
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
    let resolution = resolve(&ir, None);
    // `lower` takes no `--target` at all, so there is no version to
    // check ids against; see the note in `run_info`.
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().view(None)));
    // Mirror `load_and_lower`: the check pass gates the pipeline, and the
    // lowering deferrals its findings tend to cascade into follow on the
    // same stream.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        None,
        std::mem::take(&mut block_ir.diagnostics),
    );

    let lines = LineStarts::new(&source);
    let mut has_error = false;
    for d in &block_ir.diagnostics {
        let pos = lines.position(&source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity().as_str(),
            d.code.as_str(),
            d.primary,
        );
        report_notes(file, &source, &lines, &d.notes);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }

    // Refuse before printing, the way `run_info` and `run_compile` do. The
    // exit code alone does not protect a redirect: `cairn lower f.crn
    // --format json > ir.json` creates the file before the process ends, so
    // emitting the IR anyway hands a pipeline a well-formed artifact built
    // from a source `cairn check` rejects.
    if has_error {
        return ExitCode::from(1);
    }

    match format {
        LowerFormat::Ascii => {
            print_block_ir_ascii(&block_ir);
            ExitCode::SUCCESS
        }
        LowerFormat::Json => match serde_json::to_string_pretty(&block_ir) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: failed to serialise block-array IR as JSON: {err}");
                ExitCode::from(1)
            }
        },
        LowerFormat::Debug => {
            println!("{block_ir:#?}");
            ExitCode::SUCCESS
        }
    }
}

fn run_synth(
    file: &Path,
    experimental_flag: bool,
    stage: SynthStage,
    edition: Option<EditionArg>,
) -> ExitCode {
    if !experimental_flag {
        // Gated behind `--experimental-logic-synth` because the redstone
        // pipeline is still Internal-tier (`spec/compatibility`) — the
        // Logic IR wire form will grow the netlist / placement / route
        // layers over later changes, and every intermediate shape is fair
        // game for a breaking change. Exit 2 (usage error) so a script
        // that accidentally reaches this subcommand does not read the
        // gate as a warning it can ignore.
        eprintln!(
            "error: `cairn synth` is an internal / experimental surface; pass --experimental-logic-synth to opt in",
        );
        return ExitCode::from(2);
    }

    // Reject `--edition` on the edition-neutral stages loud instead of
    // silently ignoring it — a caller who passed the flag on `--stage
    // logic` or `--stage netlist` almost certainly expected it to shape
    // the output, and swallowing the mistake would make the CLI's
    // stage-vs-edition axis ambiguous.
    if !stage_requires_edition(stage) && edition.is_some() {
        eprintln!(
            "error: `--edition` is only meaningful with {}; the {} stages are edition-neutral",
            edition_required_stage_list(),
            edition_neutral_stage_list(),
        );
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
    let lines = LineStarts::new(&source);

    // Mirror `run_check` / `run_lower` / `run_compile`: surface the check
    // diagnostics before running the redstone synth. A `.crn` whose only
    // problem is `E_UNRESOLVED_SLOT` or a typo caught by `check` would
    // otherwise exit 0 through the synth path with a partially resolved
    // IR, which is a poor CI gate. Synth does not lower to block arrays,
    // so there are no lowering diagnostics to append here.
    let diagnostics = build_diagnostics(&module, &ir, None, Vec::new());
    if report_core_diagnostics(file, &source, &lines, &diagnostics) {
        return ExitCode::from(1);
    }

    let synth = synthesize(&ir);
    if report_synth_diagnostics(file, &source, &lines, &synth.diagnostics) {
        return ExitCode::from(1);
    }

    let (json, label) =
        match dispatch_synth_stage(stage, edition, &synth, &ir, file, &source, &lines) {
            Ok(pair) => pair,
            Err(code) => return code,
        };
    match json {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: failed to serialise {label} as JSON: {err}");
            ExitCode::from(1)
        }
    }
}

/// Run the requested pipeline stage and return the JSON serialisation
/// plus a human-facing label. The body walks the pipeline linearly and
/// short-circuits at the requested stage. Each pass whose contract can
/// raise diagnostics (Placement / Route / Delay / Crossing) is followed
/// immediately by `report_synth_diagnostics` so the report call sits
/// next to the pass that produced it and is hard to forget on future
/// additions; `compile_netlist` and `compile_edition_netlist` are
/// diagnostic-free by contract and intentionally have no report call.
/// The tail is an exhaustive `match` on `SynthStage` so adding a new
/// variant fails to compile here instead of silently reusing the
/// Crossing payload.
///
/// One thing sits outside that linear order on purpose: the
/// `--edition` gate runs before the first pass, not at the point the
/// value is first consumed. A pass inserted ahead of the edition-tagged
/// stages belongs below the gate, so a caller who forgot the flag still
/// hears about the flag rather than about whatever that pass had to say.
fn dispatch_synth_stage(
    stage: SynthStage,
    edition: Option<EditionArg>,
    synth: &cairn_lang_redstone::SynthOutput,
    ir: &cairn_lang_core::IntentModule,
    file: &Path,
    source: &str,
    lines: &LineStarts,
) -> Result<(serde_json::Result<String>, &'static str), ExitCode> {
    if matches!(stage, SynthStage::Logic) {
        return Ok((serde_json::to_string_pretty(&synth.scoped), "Logic IR"));
    }

    // Resolved ahead of `compile_netlist`: a missing `--edition` is a
    // usage mistake, and a usage mistake is worth reporting before any
    // synthesis work is paid for, not after.
    let edition = if stage_requires_edition(stage) {
        Some(require_edition(edition, stage_cli_name(stage))?.as_edition())
    } else {
        None
    };

    let netlist = compile_netlist(&synth.scoped);
    // The edition-neutral tail dispatches on the stage, not on "no
    // edition was resolved". The two say the same thing today, but only
    // the former makes a stage added later state its own answer here:
    // the negative form would hand it the Netlist payload, under the
    // Netlist label, with exit 0.
    let edition = match (edition, stage) {
        (Some(edition), _) => edition,
        (None, SynthStage::Netlist) => {
            return Ok((serde_json::to_string_pretty(&netlist), "Netlist IR"));
        }
        (None, SynthStage::Logic) => unreachable!("the Logic guard above returns"),
        (
            None,
            SynthStage::Edition
            | SynthStage::Placement
            | SynthStage::Route
            | SynthStage::Delay
            | SynthStage::Crossing,
        ) => unreachable!(
            "stage_requires_edition holds here, so the gate above resolved an edition or returned"
        ),
    };
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    if matches!(stage, SynthStage::Edition) {
        return Ok((
            serde_json::to_string_pretty(&edition_netlist),
            "Edition Netlist IR",
        ));
    }

    let placement = compile_placement(&edition_netlist, ir);
    if report_synth_diagnostics(file, source, lines, &placement.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Placement) {
        return Ok((
            serde_json::to_string_pretty(&placement.scoped),
            "Placement IR",
        ));
    }

    let routing = compile_routing(&placement.scoped);
    if report_synth_diagnostics(file, source, lines, &routing.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Route) {
        return Ok((
            serde_json::to_string_pretty(&routing.scoped),
            "Routed Placement IR",
        ));
    }

    let delay = compile_delay(&routing.scoped);
    if report_synth_diagnostics(file, source, lines, &delay.diagnostics) {
        return Err(ExitCode::from(1));
    }
    if matches!(stage, SynthStage::Delay) {
        return Ok((
            serde_json::to_string_pretty(&delay.scoped),
            "Delayed Placement IR",
        ));
    }

    let crossing = compile_crossing(&delay.scoped);
    if report_synth_diagnostics(file, source, lines, &crossing.diagnostics) {
        return Err(ExitCode::from(1));
    }
    match stage {
        SynthStage::Crossing => Ok((
            serde_json::to_string_pretty(&crossing.scoped),
            "Legalized Placement IR",
        )),
        SynthStage::Logic
        | SynthStage::Netlist
        | SynthStage::Edition
        | SynthStage::Placement
        | SynthStage::Route
        | SynthStage::Delay => {
            unreachable!("earlier guards return for non-Crossing stages")
        }
    }
}

/// Hand-maintained mirror of clap's kebab-case derivation of
/// `SynthStage` variant names: the single place the messages this
/// binary composes at runtime read a stage's spelling from, so what
/// a caller is told to type matches what the parser accepts. The
/// canonical spelling is whatever clap accepts on the command line
/// (derived from `#[derive(ValueEnum)]` on `SynthStage`); this
/// function must be kept in sync on every variant addition or
/// rename. Its exhaustive `match` provides a compile-time nudge to
/// do so.
///
/// What it does not reach is the `--stage` / `--edition` `--help`
/// prose, which clap takes as string literals and which therefore
/// spells every stage by hand — the same carve-out
/// `stage_requires_edition` names for the partition it owns. A
/// variant added here still has to be worked into that prose
/// separately.
///
/// The four Placement IR stages take their spelling from
/// [`PlacementStage::as_str`] rather than repeating the literal, so
/// the word this function returns and the word the dump's `"stage"`
/// key carries cannot drift apart. What no type can enforce is the
/// third spelling in the chain — the one clap derives from the
/// variant identifier — so `placement_stage_names_match_clap` below
/// pins that against `ValueEnum` directly.
fn stage_cli_name(stage: SynthStage) -> &'static str {
    match stage {
        SynthStage::Logic => "logic",
        SynthStage::Netlist => "netlist",
        SynthStage::Edition => "edition",
        SynthStage::Placement => PlacementStage::Placement.as_str(),
        SynthStage::Route => PlacementStage::Route.as_str(),
        SynthStage::Delay => PlacementStage::Delay.as_str(),
        SynthStage::Crossing => PlacementStage::Crossing.as_str(),
    }
}

/// Whether `--stage <stage>` reads the target-edition cell library and
/// therefore needs `--edition <java|bedrock>` alongside it.
///
/// The same partition drives both halves of the flag's contract:
/// `run_synth` refuses `--edition` as stray on the stages this returns
/// `false` for, and `dispatch_synth_stage` demands it on the ones it
/// returns `true` for. Spelling the set once is what keeps a stage
/// from landing in neither half — or, worse, in both. The exhaustive
/// `match` makes a new `SynthStage` variant a compile error here,
/// where the decision belongs, rather than a silent default to
/// edition-neutral.
///
/// The stray-`--edition` message renders its two stage lists from this
/// function too, so what a caller is told matches what the gates
/// enforce. What stays hand-written is the same partition as it
/// appears in prose in the `--stage` / `--edition` `--help` text,
/// which clap takes as string literals: a stage added on the `true`
/// side has to be worked into both sentences by hand.
fn stage_requires_edition(stage: SynthStage) -> bool {
    match stage {
        SynthStage::Logic | SynthStage::Netlist => false,
        SynthStage::Edition
        | SynthStage::Placement
        | SynthStage::Route
        | SynthStage::Delay
        | SynthStage::Crossing => true,
    }
}

/// `` `--stage a`, `--stage b`, or `--stage c` `` over the stages that
/// require `--edition`, for the stray-flag message.
fn edition_required_stage_list() -> String {
    join_stages(stage_requires_edition, "or", |name| {
        format!("`--stage {name}`")
    })
}

/// `` `a` and `b` `` over the stages that refuse `--edition`, for the
/// same message. Renders bare stage names because that half of the
/// sentence talks about the stages themselves rather than about the
/// flag a caller would have typed.
fn edition_neutral_stage_list() -> String {
    join_stages(
        |stage| !stage_requires_edition(stage),
        "and",
        |name| format!("`{name}`"),
    )
}

/// Render the `--stage` values matching `select` as an English list,
/// in the order clap declares them, with each name passed through
/// `render` first. Walking `ValueEnum` rather than a literal list is
/// what lets the stray-`--edition` message pick up a stage the day it
/// lands instead of naming a set that has since moved on.
fn join_stages(
    select: impl Fn(SynthStage) -> bool,
    conjunction: &str,
    render: impl Fn(&str) -> String,
) -> String {
    let items: Vec<String> = SynthStage::value_variants()
        .iter()
        .copied()
        .filter(|stage| select(*stage))
        .map(|stage| render(stage_cli_name(stage)))
        .collect();
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, [only])) => format!("{only} {conjunction} {last}"),
        Some((last, rest)) => format!("{}, {conjunction} {last}", rest.join(", ")),
    }
}

/// Enforce the `--edition <java|bedrock>` requirement for the
/// edition-tagged stages. The stage name is baked into the error
/// message so a caller sees exactly which stage tripped the gate,
/// plus a short "why" hint so a caller who doesn't know the pipeline
/// still connects the flag to the edition-specific cell library.
fn require_edition(edition: Option<EditionArg>, stage_name: &str) -> Result<EditionArg, ExitCode> {
    edition.ok_or_else(|| {
        eprintln!(
            "error: `cairn synth --stage {stage_name}` requires --edition <java|bedrock> (this stage picks the target-edition cell realisation, so the flag is not optional)",
        );
        ExitCode::from(2)
    })
}

/// Print `cairn-lang-core::check::Diagnostic`s in gcc-style, returning
/// `true` when any Error-severity finding was seen. Shared between
/// `run_synth`'s resolve + check pre-passes.
/// Every diagnostic a command must report, in one stream, in the order the
/// passes ran.
///
/// [`check`] runs the syntactic passes **and** merges the resolver's
/// findings (see `cairn_lang_core::check::check`), so a caller must not
/// append `Resolution::diagnostics` on top of it. Doing so printed every
/// resolver finding twice, and — because [`check`] returns span-sorted
/// output while the resolver emits in discovery order — the second copy
/// walked the file backwards.
///
/// `lowering` carries the block-array pass's own diagnostics, which
/// [`check`] never sees; pass an empty vector from a command that does not
/// lower.
fn build_diagnostics(
    module: &cairn_lang_core::ast::Module,
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    lowering: Vec<cairn_lang_core::check::Diagnostic>,
) -> Vec<cairn_lang_core::check::Diagnostic> {
    let mut combined = check(module, ir, edition);
    combined.extend(lowering);
    combined
}

fn report_core_diagnostics(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    diagnostics: &[cairn_lang_core::check::Diagnostic],
) -> bool {
    let mut has_error = false;
    for d in diagnostics {
        let pos = lines.position(source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity().as_str(),
            d.code.as_str(),
            d.primary,
        );
        report_notes(file, source, lines, &d.notes);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

/// Print redstone synth diagnostics in the same format the core passes
/// use. Kept as a separate function because the two `Diagnostic` types
/// differ in the one field that matters here — their `code` — so a merged
/// version would take a trait over the finding to read four fields off it.
/// The notes are already shared: both are
/// [`cairn_lang_core::check::DiagnosticNote`].
fn report_synth_diagnostics(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    diagnostics: &[cairn_lang_redstone::Diagnostic],
) -> bool {
    let mut has_error = false;
    for d in diagnostics {
        let pos = lines.position(source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity().as_str(),
            d.code.as_str(),
            d.primary,
        );
        report_notes(file, source, lines, &d.notes);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

fn print_block_ir_ascii(block_ir: &BlockArrayIr) {
    if block_ir.structures.is_empty() {
        println!("(no structures lowered)");
        return;
    }
    for (key, ba) in &block_ir.structures {
        println!("{key}  dims={}x{}x{}", ba.dims.x, ba.dims.y, ba.dims.z);
        println!("  palette:");
        for (i, state) in ba.palette.entries.iter().enumerate() {
            let glyph = ascii_glyph(i);
            if state.properties.is_empty() {
                println!("    [{i:>3}] {glyph}  {}", state.id);
            } else {
                let props = state
                    .properties
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                println!("    [{i:>3}] {glyph}  {}[{props}]", state.id);
            }
        }
        for y in 0..ba.dims.y {
            println!("  y={y}");
            print_y_slice(ba, y);
        }
    }
}

const ASCII_ALPHABET: &[u8] = b"#abcdefghijklmnopqrstuvwxyz0123456789";

/// Glyph for a palette index in ASCII slice output: air → `.`, anything
/// else → `#` for the first non-air, then digits/letters so a slice with
/// many distinct materials still reads. Any palette entry past index 36
/// renders as `?` — debug-format only, and well above the per-structure
/// palette size the current examples use (cottage uses 3 entries), but
/// worth a glance before reading a `?`-heavy slice as evidence of broken
/// lowering.
fn ascii_glyph(palette_index: usize) -> char {
    if palette_index == 0 {
        return '.';
    }
    ASCII_ALPHABET
        .get(palette_index - 1)
        .copied()
        .map_or('?', char::from)
}

fn print_y_slice(ba: &BlockArray, y: u32) {
    for z in 0..ba.dims.z {
        let mut row = String::with_capacity(ba.dims.x as usize);
        for x in 0..ba.dims.x {
            let i = ba.dims.index(x, y, z).expect("in-range coordinate");
            row.push(ascii_glyph(usize::from(ba.voxels[i].0)));
        }
        println!("    {row}");
    }
}

/// A resolved compile target: an edition-specific version-integer wrapper
/// plus the knowledge of which backend serialises it. Every downstream
/// step (filename extension, tag builder, writer, lockfile row) branches
/// on this one value so a new edition is added in a single place.
enum ResolvedTarget {
    /// Java vanilla structure target (`.nbt`, gzip).
    Java(JavaTarget),
    /// Bedrock structure target (`.mcstructure`, uncompressed).
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// On-disk extension the backend writes. The three edition-varying
    /// steps of a compile — extension, tag builder ([`Self::build_tag`]),
    /// and writer ([`Self::write_tag`]) — all live on this type so their
    /// correspondence is co-located rather than kept in step by convention
    /// across scattered `match`es. Adding an edition means adding one arm
    /// to each and the compiler flags any it misses.
    fn output_ext(&self) -> OutputExt {
        match self {
            ResolvedTarget::Java(_) => OutputExt::Nbt,
            ResolvedTarget::Bedrock(_) => OutputExt::Mcstructure,
        }
    }

    /// Build the structure tag tree for this edition's backend, plus any
    /// `W_INTENT_DEGRADED` parity notes raised while lowering intent to the
    /// edition (Java is always lossless, so its note list is empty). The two
    /// backends raise different error types; both are rendered to a message
    /// string here so the caller has one error shape to report.
    ///
    /// [`ParityNote`] is threaded verbatim rather than flattened to a message
    /// string so the CLI can key the warning by the palette id that
    /// degraded, keeping the (`id`, `message`) pair machine-parsable for
    /// downstream tools.
    fn build_tag(&self, ba: &BlockArray) -> Result<(Compound, Vec<ParityNote>), String> {
        match self {
            ResolvedTarget::Java(t) => build_structure_tag(ba, t)
                .map(|tag| (tag, Vec::new()))
                .map_err(|e| e.to_string()),
            ResolvedTarget::Bedrock(t) => build_mcstructure_tag(ba, t).map_err(|e| e.to_string()),
        }
    }

    /// Write a built tag tree in this edition's on-disk form: Java `.nbt`
    /// is gzip-wrapped big-endian, Bedrock `.mcstructure` is raw
    /// little-endian.
    fn write_tag<W: std::io::Write>(
        &self,
        writer: &mut W,
        tag: &Compound,
    ) -> Result<(), std::io::Error> {
        let encoded = match self {
            ResolvedTarget::Java(_) => write_compound_gzip(writer, tag),
            ResolvedTarget::Bedrock(_) => write_mcstructure(writer, tag),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }

    /// Human-facing Minecraft version string for the lockfile.
    fn mc_version(&self) -> &str {
        match self {
            ResolvedTarget::Java(t) => &t.mc_version,
            ResolvedTarget::Bedrock(t) => &t.mc_version,
        }
    }

    /// Edition-specific version integer for the lockfile (`DataVersion`
    /// for Java, block-palette `version` for Bedrock).
    fn version_int(&self) -> i32 {
        match self {
            ResolvedTarget::Java(t) => t.data_version,
            ResolvedTarget::Bedrock(t) => t.block_version,
        }
    }

    /// Registry pack whose bytes the compile resolved against, hashed into
    /// the lockfile.
    fn registry_pack(&self) -> &'static RegistryPack {
        match self {
            ResolvedTarget::Java(_) => builtin_java(),
            ResolvedTarget::Bedrock(_) => builtin_bedrock(),
        }
    }
}

fn run_compile(
    file: &Path,
    edition: EditionArg,
    target: &str,
    out: Option<&Path>,
    lock: Option<&Path>,
) -> ExitCode {
    // Resolved before lowering because lowering checks every block id
    // against the pinned version's table, and reported below rather than
    // here — see `resolve_target`. A target that does not resolve leaves
    // lowering with nothing to check against, which is the same "no target
    // pinned" mode `cairn check` runs in.
    let resolved_target = resolve_target(edition, target);
    let pinned = resolved_target
        .as_ref()
        .ok()
        .map(ResolvedTarget::mc_version);
    let Lowered {
        source,
        block_ir,
        dropped_scopes,
        version_floor,
    } = match load_and_lower(file, edition, pinned) {
        Ok(lowered) => lowered,
        Err(code) => return code,
    };
    if report_lowering_diagnostics(file, &source, &block_ir) {
        return ExitCode::from(1);
    }
    // A lockfile records that a specific resolved IR was built for a
    // specific target, so it must not certify a build that is missing part
    // of what the source asked for. Every code that drops a scope
    // (`W_STRUCT_NO_SIZE`, `W_DEF_NO_SIZE`, ...) is Warning severity, so
    // without this the exit code stays 0 and the lockfile still reads
    // `verified: true` — the same shape of defect as compiling a source
    // `cairn check` rejects.
    //
    // Declaring nothing is not the same as losing something: a source that
    // is only templates and themes requests no scopes, produces none, and
    // still compiles (see
    // `c26_bare_def_without_place_emits_w_unused_def_and_no_nbt`).
    if !dropped_scopes.is_empty() {
        eprintln!(
            "error[E_PARTIAL_BUILD]: {}: {} of {} requested scopes did not lower; \
             refusing to certify a partial build",
            file.display(),
            dropped_scopes.len(),
            dropped_scopes.len() + block_ir.structures.len(),
        );
        for scope in &dropped_scopes {
            eprintln!("  note: `{scope}` produced no voxels");
        }
        return ExitCode::from(1);
    }

    let target = match resolved_target {
        Ok(t) => t,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if let Err(code) =
        enforce_version_floor(file, &source, version_floor.as_ref(), edition, &target)
    {
        return code;
    }

    let out_dir = match prepare_out_dir(file, out) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let prepared = match prepare_artifacts(&block_ir, &target, &out_dir) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    if let Err(code) = check_lock_path_is_free(&prepared, &lock_path) {
        return code;
    }
    // Before the file is replaced, not after: the lockfile about to be
    // overwritten is the only record of what was previously verified.
    report_previous_target(&lock_path, edition, &target);
    write_artifacts_and_lock(&prepared, &source, &block_ir, edition, &target, &lock_path)
}

/// Compare the lockfile at `lock_path` with the target being built, and
/// report the divergence the way `spec/versioning-editions.md` §10.6 does.
///
/// The lockfile is the record of what was verified, so a recompile for a
/// different target is the moment that record stops describing what is on
/// disk. Nothing here changes the build or the exit code — both lines are
/// warnings, and a first compile or an unchanged target says nothing at
/// all. A lockfile that cannot be read says so; only its absence is
/// silent.
fn report_previous_target(lock_path: &Path, edition: EditionArg, target: &ResolvedTarget) {
    let previous = match Lockfile::read_from_path(lock_path) {
        Ok(previous) => previous,
        // No lockfile is the ordinary first-compile case, and the only one
        // that should be silent. Testing `exists()` first would fold a
        // permission error into it, and leave a window in which the file
        // vanishes between the check and the read.
        Err(LockError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => return,
        // A document from a newer Cairn is not corrupt, and replacing it
        // does lose something, so it says so in its own words.
        Err(LockError::UnsupportedSchemaVersion { found, supported }) => {
            eprintln!(
                "warning: {}: the existing lockfile is schema version {found} and this build \
                 reads {supported}; it was written by a newer Cairn and is being replaced",
                lock_path.display(),
            );
            return;
        }
        Err(err) => {
            // Not an error: the compile is valid, and a corrupt file beside
            // the source is no reason to refuse to build. But it was being
            // overwritten in silence, which is how a tampered or stale
            // lockfile went unnoticed.
            eprintln!(
                "warning: {}: the existing lockfile could not be read ({err}); replacing it",
                lock_path.display(),
            );
            return;
        }
    };
    let now = LockTarget {
        edition: edition.as_lock_edition(),
        mc_version: target.mc_version().to_owned(),
        data_version: target.version_int(),
    };
    if previous.target == now {
        return;
    }
    // The edition appears only when it changed: two editions number their
    // releases differently, so `1.21.4` against `1.21.60` reads as noise
    // without it, and naming it on every line would pad the common case.
    let name_edition = previous.target.edition != now.edition;
    eprintln!(
        "W_PREVIOUSLY_VERIFIED_TARGET: verified for {}, now {}.",
        describe_verified(&previous.target, name_edition),
        describe_now(&now, name_edition),
    );
    if previous.member_version_sensitivity.is_empty() {
        return;
    }
    let ids: Vec<&str> = previous
        .member_version_sensitivity
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    eprintln!(
        "W_SEMANTIC_SENSITIVITY: {} member{} may resolve differently: {}",
        ids.len(),
        if ids.len() == 1 { "" } else { "s" },
        ids.join(", "),
    );
}

/// The left half of the warning: `1.20.4/DataVersion 3700`.
///
/// The integer is named here and bare on the right, which is the shape
/// §10.6 prints. Java's is Minecraft's `DataVersion`; Bedrock's is the
/// block palette's own `version`, and calling both `DataVersion` would name
/// the Java concept for a number that is not one.
fn describe_verified(target: &LockTarget, name_edition: bool) -> String {
    let field = match target.edition {
        LockEdition::Java => "DataVersion",
        LockEdition::Bedrock => "block version",
    };
    format!(
        "{}{}/{} {}",
        edition_prefix(target, name_edition),
        target.mc_version,
        field,
        target.data_version,
    )
}

/// The right half of the warning: `1.21.4/4189`.
fn describe_now(target: &LockTarget, name_edition: bool) -> String {
    format!(
        "{}{}/{}",
        edition_prefix(target, name_edition),
        target.mc_version,
        target.data_version,
    )
}

fn edition_prefix(target: &LockTarget, name_edition: bool) -> String {
    if name_edition {
        format!("{} ", target.edition.as_str())
    } else {
        String::new()
    }
}

/// A `.crn` read, parsed, resolved, and lowered, plus what the lowering
/// failed to produce.
struct Lowered {
    source: String,
    block_ir: BlockArrayIr,
    /// Scopes the source asked for that produced no voxels, as
    /// [`dropped_scopes`] collects them.
    dropped_scopes: Vec<String>,
    /// The strictest `@requires` floor the source declares, carried out of
    /// the parse so `compile` can hold `--target` to it without reading the
    /// file a second time. `None` when the source declares none, which is
    /// the ordinary case.
    version_floor: Option<VersionFloor>,
}

fn load_and_lower(
    file: &Path,
    edition: EditionArg,
    mc_version: Option<&str>,
) -> Result<Lowered, ExitCode> {
    let source = std::fs::read_to_string(file).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })?;
    let module = parse(&source).map_err(|err| {
        eprintln!(
            "error: {}:{}: {}",
            file.display(),
            err.position(),
            err.user_message(),
        );
        ExitCode::from(1)
    })?;
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition.as_edition()));
    // The pack is edition-specific: an abstract `@token` resolves through
    // the pack whose backend will serialise it, and the id table it is
    // checked against belongs to the one version that pack was pinned to.
    // `mc_version: None` (no `--target` resolved) leaves the id check off
    // rather than running it against a version nobody chose.
    let registry = edition.registry_pack().view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // The check pass is the gate `cairn check` exposes; running it here is
    // what keeps `compile` from accepting a source that `check` rejects. It
    // used to be skipped, so an `E_DUPLICATE_ID` would compile to artifacts
    // plus a `verified: true` lockfile at exit 0. Resolver findings such as
    // a `place use=cottag` typo (`E_UNRESOLVED_PLACE_REF`) reach the stream
    // through the same call — `check` merges them.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        Some(edition.as_edition()),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    Ok(Lowered {
        version_floor: declared_version_floor(&module),
        source,
        block_ir,
        dropped_scopes,
    })
}

/// Refuse a `--target` below the floor the source declares.
///
/// `@requires version>=X` is the source's own statement of what it needs.
/// It was rendered by `cairn info` and enforced nowhere, so compiling
/// against a lower target succeeded and wrote a lockfile reading
/// `verified: true` for a version the file itself rules out. A lock records
/// what was checked; certifying a target the source disowns is the one
/// thing it must not do.
///
/// Checked here rather than in `check()`: the constraint is a relation
/// between the source and `--target`, and `cairn check` has no target. It
/// runs before any artifact is prepared, so a refusal leaves nothing on
/// disk.
///
/// Spec §10.4 shows this code on a different comparison — a *material*
/// introduced after the target, from the registry's `since` data. That data
/// is not in the pack yet; when it arrives it joins this code rather than
/// getting its own, because both answer "the target is below a floor".
///
/// The comparison is `cairn-lang-core`'s dotted-decimal one, which does not
/// know that the two editions number releases differently. A Java-shaped
/// floor of `1.21.4` reads as satisfied by Bedrock `1.21.40` on `40 > 4`;
/// the spec's "Ordering, and where it stops" records that, and whether
/// `@requires` is edition-neutral at all is an open language question
/// rather than something to settle here.
///
/// # Errors
///
/// Returns exit code 1 when the target is below the floor.
fn enforce_version_floor(
    file: &Path,
    source: &str,
    floor: Option<&VersionFloor>,
    edition: EditionArg,
    target: &ResolvedTarget,
) -> Result<(), ExitCode> {
    let Some(floor) = floor else {
        return Ok(());
    };
    if !compare_versions(target.mc_version(), &floor.version).is_lt() {
        return Ok(());
    }
    let position = LineStarts::new(source).position(source, floor.span.start);
    eprintln!(
        "error[E_VERSION_CAP]: {}:{}: this file requires version>={} (target {}).",
        file.display(),
        position,
        floor.version,
        target.mc_version(),
    );
    // `spec/lint.md` §11.2 makes the closed set of candidates valid in the
    // target part of the message, not an extra. Naming the floor alone
    // sends an author to `--target >=99.0`, which is a second error and no
    // closer to a build; whether *any* supported target satisfies the floor
    // is the fact that decides what they do next.
    let supported = edition.registry_pack().supported_list();
    let usable: Vec<&str> = supported
        .split(", ")
        .filter(|candidate| {
            *candidate != "latest" && !compare_versions(candidate, &floor.version).is_lt()
        })
        .collect();
    if usable.is_empty() {
        eprintln!(
            "  no supported {} target satisfies it: {supported}",
            edition.as_str(),
        );
        eprintln!("  fix: lower the `@requires` floor, or build against another edition");
    } else {
        eprintln!(
            "  valid {} targets: {}",
            edition.as_str(),
            usable.join(", ")
        );
        eprintln!(
            "  fix: --target {}, or lower the `@requires` floor",
            usable[0],
        );
    }
    Err(ExitCode::from(1))
}

/// Print one finding's `note:` lines under its primary.
///
/// A note that carries a span is printed with that position, the way the
/// primary is: it names a second place in the file the reader has to go
/// look at, and "declared here" with no *here* is not a note. A note
/// without one is indented and left unprefixed, so a footer does not read
/// as a second pointer at the primary span.
///
/// Shared rather than copied: this loop existed six times in this file,
/// and three of the six had dropped the position. Both note types are
/// `cairn_lang_core::check::DiagnosticNote` — `cairn-lang-redstone`
/// re-exports it — so one signature covers every caller.
fn report_notes(file: &Path, source: &str, lines: &LineStarts, notes: &[Note]) {
    for note in notes {
        match note.span.as_ref() {
            Some(span) => {
                let pos = lines.position(source, span.start);
                eprintln!("{}:{}:   note: {}", file.display(), pos, note.message);
            }
            None => eprintln!("  note: {}", note.message),
        }
    }
}

fn report_lowering_diagnostics(file: &Path, source: &str, block_ir: &BlockArrayIr) -> bool {
    let lines = LineStarts::new(source);
    let mut has_error = false;
    for d in &block_ir.diagnostics {
        let pos = lines.position(source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity().as_str(),
            d.code.as_str(),
            d.primary,
        );
        report_notes(file, source, &lines, &d.notes);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

/// Resolve `--target` against the pack for `--edition`.
///
/// Returns the failure instead of printing it: `run_compile` resolves the
/// target before it lowers (block-array lowering needs the pinned version
/// to check block ids against) but reports the failure where it always
/// did, after the parse and lowering diagnostics. A command-line mistake
/// that jumped ahead of a syntax error would change which problem a user
/// is told about first.
fn resolve_target(
    edition: EditionArg,
    target: &str,
) -> Result<ResolvedTarget, cairn_lang_formats::data_version::UnsupportedTarget> {
    match edition {
        EditionArg::Java => resolve_java_target(target).map(ResolvedTarget::Java),
        EditionArg::Bedrock => resolve_bedrock_target(target).map(ResolvedTarget::Bedrock),
    }
}

fn prepare_out_dir(file: &Path, requested: Option<&Path>) -> Result<PathBuf, ExitCode> {
    let Some(out_dir) = resolve_out_dir(file, requested) else {
        eprintln!(
            "error: source `{}` has no parent directory and --out was not given",
            file.display(),
        );
        return Err(ExitCode::from(1));
    };
    std::fs::create_dir_all(&out_dir).map_err(|err| {
        eprintln!(
            "error: cannot create output directory `{}`: {err}",
            out_dir.display(),
        );
        ExitCode::from(1)
    })?;
    Ok(out_dir)
}

/// Build every structure tag tree up front. A backend error here (abstract
/// palette entry, stateful Bedrock entry, dimension overflow) must not
/// leave half-written artifacts behind, so the function holds off all I/O
/// until it knows the IR is serialisable.
fn prepare_artifacts(
    block_ir: &BlockArrayIr,
    target: &ResolvedTarget,
    out_dir: &Path,
) -> Result<Vec<(PathBuf, Compound)>, ExitCode> {
    let mut prepared = Vec::with_capacity(block_ir.structures.len());
    let mut seen_paths: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::with_capacity(block_ir.structures.len());
    for (scope, ba) in &block_ir.structures {
        let (tag, degraded) = target.build_tag(ba).map_err(|err| {
            eprintln!("error: building `{scope}`: {err}");
            ExitCode::from(1)
        })?;
        for note in degraded {
            // Keep `id` on the warning line so tools can group by the
            // degraded palette entry, not just by scope.
            eprintln!(
                "warning[W_INTENT_DEGRADED]: {scope}: {id}: {message}",
                id = note.id,
                message = note.message,
            );
        }
        let path = out_dir.join(output_filename(scope, target.output_ext()));
        // Walkway IR keys allow `.` / `_` in place and port ids; the
        // `output_filename` flatten of `.` → `_` can fold two distinct
        // walkways into the same on-disk name (e.g. `a.b_c__d.e_f` vs
        // `a_b.c__d_e.f` both → `..._a_b_c__d_e_f`). Detecting that
        // here keeps the second walkway from silently overwriting the
        // first.
        if let Some(first) = seen_paths.insert(path.clone(), scope.clone()) {
            eprintln!(
                "error: output filename `{}` collides between scopes `{first}` and `{scope}`",
                path.display(),
            );
            return Err(ExitCode::from(1));
        }
        prepared.push((path, tag));
    }
    Ok(prepared)
}

/// Refuse a `--lock` that would land on a path the artifacts already own.
///
/// `prepare_artifacts` checks the artifacts against each other, and the
/// lockfile was never folded into that check, so `--lock out/home1.nbt` put
/// two entries with the same destination into one set. They staged over each
/// other's bytes, and during the commit the second one deleted the backup
/// the first had just taken — destroying the previous build's artifact with
/// no copy left anywhere. That is the failure this whole path exists to
/// prevent, reachable through an argument the CLI accepted without comment.
///
/// The scratch names count too: `--lock out/home1.nbt.tmp` collides during
/// staging rather than during the commit, and is just as unrecoverable.
fn check_lock_path_is_free(
    prepared: &[(PathBuf, Compound)],
    lock_path: &Path,
) -> Result<(), ExitCode> {
    let taken: std::collections::HashSet<PathBuf> = prepared
        .iter()
        .flat_map(|(path, _)| staging::reserved_paths(path))
        .collect();
    for reserved in staging::reserved_paths(lock_path) {
        if taken.contains(&reserved) {
            eprintln!(
                "error: lockfile path `{}` collides with an artifact this build writes (`{}`)",
                lock_path.display(),
                reserved.display(),
            );
            eprintln!(
                "  note: pass a `--lock` outside `--out`, or rename the struct whose artifact \
                 shares the name"
            );
            return Err(ExitCode::from(1));
        }
    }
    Ok(())
}

/// Write the prepared structure files and the lockfile as one set: either
/// every artifact plus the lock, or the directory exactly as it was.
///
/// Two phases. Staging writes each file beside where it belongs, so every
/// way the encode or the write can fail happens while the previous build is
/// still untouched. Committing is renames only; [`staging::StagedSet::commit`]
/// describes what makes that step recoverable.
///
/// What the phases buy is a place to fail from. The single-phase version
/// wrote each artifact atomically but *individually*, so by the time the
/// lockfile failed some destinations had already been overwritten, and the
/// only undo available was deleting them.
fn write_artifacts_and_lock(
    prepared: &[(PathBuf, Compound)],
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
    target: &ResolvedTarget,
    lock_path: &Path,
) -> ExitCode {
    // Phase 1 — stage. Nothing a previous build produced is touched, so a
    // failure here costs only our own scratch.
    let mut staged = staging::StagedSet::default();
    for (path, tag) in prepared {
        if let Err(err) = staged.stage(path, staging::Kind::Artifact, |file| {
            target.write_tag(file, tag)
        }) {
            staged.discard();
            eprintln!("error: writing `{}`: {err}", path.display());
            return ExitCode::from(1);
        }
    }

    // Encode before touching the filesystem: a hash or YAML failure has to
    // be indistinguishable from never having started.
    let lock_body = match build_lockfile(source, block_ir, edition, target)
        .map_err(|err| err.to_string())
        .and_then(|lockfile| lockfile.to_yaml().map_err(|err| err.to_string()))
    {
        Ok(body) => body,
        Err(err) => {
            staged.discard();
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if let Err(err) = staged.stage(lock_path, staging::Kind::Lockfile, |file| {
        std::io::Write::write_all(file, lock_body.as_bytes())
    }) {
        staged.discard();
        eprintln!("error: writing lockfile `{}`: {err}", lock_path.display());
        return ExitCode::from(1);
    }

    // Phase 2 — commit. Renames only, each keeping what it replaces until
    // the whole set has landed.
    match staged.commit() {
        Ok(written) => {
            for path in written {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("error: {failure}");
            ExitCode::from(1)
        }
    }
}

/// Staging a set of output files and committing them together.
///
/// A module rather than loose functions so the invariant that makes the
/// commit recoverable — a staged file is only ever reachable through the
/// scratch path this code chose — is enforced by privacy instead of by
/// convention. `main.rs` has no other modules, so without one the fields
/// below would be visible to every line in the file.
mod staging {
    use std::fmt;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    /// What a staged file is going to become, so a failure can name it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        Artifact,
        Lockfile,
    }

    impl fmt::Display for Kind {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Artifact => f.write_str("artifact"),
                Self::Lockfile => f.write_str("lockfile"),
            }
        }
    }

    /// Which half of the commit failed.
    ///
    /// The two call for different fixes and point at different paths, so a
    /// single "writing `X`" message for both sent the operator after the
    /// wrong file: a stale `X.bak` directory blocks the *displace* step
    /// while the error named `X`.
    #[derive(Debug, Clone, Copy)]
    enum Step {
        /// Moving what is already at the destination out of the way.
        Displace,
        /// Renaming the staged file into the destination.
        Place,
    }

    /// A commit that could not complete. The directory has been put back.
    #[derive(Debug)]
    pub struct CommitFailure {
        kind: Kind,
        step: Step,
        /// The path the failing rename touched — the backup for a displace,
        /// the destination for a place.
        path: PathBuf,
        source: io::Error,
    }

    impl fmt::Display for CommitFailure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self {
                kind,
                step,
                path,
                source,
            } = self;
            match step {
                Step::Displace => write!(
                    f,
                    "the existing {kind} could not be moved aside to `{}`: {source}",
                    path.display(),
                ),
                Step::Place => write!(f, "writing {kind} `{}`: {source}", path.display()),
            }
        }
    }

    /// A file written beside its destination, waiting to be renamed there.
    struct Staged {
        kind: Kind,
        /// Where it belongs once every file in the set has been written.
        final_path: PathBuf,
        /// Where it is now.
        tmp_path: PathBuf,
    }

    /// A destination that already held something, moved aside for the
    /// duration of the commit so a failure part-way can put it back.
    struct Displaced {
        final_path: PathBuf,
        backup_path: PathBuf,
    }

    /// Output files written but not yet in place.
    #[derive(Default)]
    pub struct StagedSet {
        entries: Vec<Staged>,
    }

    /// `path` with `suffix` appended to the whole file name.
    ///
    /// Not `Path::with_extension`, which replaces the extension rather than
    /// extending it: a lockfile at `village.crn.lock` would stage to
    /// `village.crn.tmp`, colliding with any other `village.crn.*` scratch
    /// and losing the `.lock` that names it.
    fn suffixed(path: &Path, suffix: &str) -> PathBuf {
        let mut raw = path.as_os_str().to_owned();
        raw.push(suffix);
        PathBuf::from(raw)
    }

    /// Every path this set will create or rename, staging scratch included.
    ///
    /// Callers check these against each other before staging starts: two
    /// entries sharing a destination would stage over each other's bytes and
    /// then fight over the same backup during the commit.
    pub fn reserved_paths(final_path: &Path) -> [PathBuf; 3] {
        [
            final_path.to_path_buf(),
            suffixed(final_path, ".tmp"),
            suffixed(final_path, ".bak"),
        ]
    }

    /// What is sitting at a destination right now.
    enum Occupant {
        /// Nothing — the commit will create the file.
        Vacant,
        /// A file or symlink, which `rename` can move aside and put back.
        Movable,
        /// A directory. `rename` cannot replace one with a file, so this is
        /// not something to back up; it is the reason the place step below
        /// is about to fail, and it fails with the directory named.
        Directory,
    }

    /// Deliberately `symlink_metadata` rather than `Path::is_file`, which
    /// folds every metadata error into `false`. A `false` here means "no
    /// backup needed", so a transient stat failure would let the commit
    /// overwrite a file it could not restore — the exact loss this whole
    /// module exists to prevent. An unreadable destination is a hard error
    /// instead.
    fn occupant(path: &Path) -> io::Result<Occupant> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => Ok(Occupant::Directory),
            Ok(_) => Ok(Occupant::Movable),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Occupant::Vacant),
            Err(err) => Err(err),
        }
    }

    impl StagedSet {
        /// Write one file beside `final_path`.
        ///
        /// # Errors
        ///
        /// Propagates the I/O failure. The partial scratch file is removed
        /// before returning: a failed entry is never added to the set, so
        /// [`Self::discard`] has no way to reach it afterwards.
        pub fn stage(
            &mut self,
            final_path: &Path,
            kind: Kind,
            write: impl FnOnce(&mut fs::File) -> io::Result<()>,
        ) -> io::Result<()> {
            use std::io::Write as _;

            let tmp_path = suffixed(final_path, ".tmp");
            let result = (|| {
                let mut file = fs::File::create(&tmp_path)?;
                write(&mut file)?;
                file.flush()?;
                // The commit below is a rename, which is only atomic with
                // respect to the directory entry — without this the bytes
                // can still be in flight when the rename publishes the name,
                // so a crash leaves a correctly-named, half-written file.
                file.sync_all()
            })();
            if let Err(err) = result {
                let _ = fs::remove_file(&tmp_path);
                return Err(err);
            }
            self.entries.push(Staged {
                kind,
                final_path: final_path.to_path_buf(),
                tmp_path,
            });
            Ok(())
        }

        /// Throw the staged files away without touching any destination.
        pub fn discard(self) {
            for entry in &self.entries {
                let _ = fs::remove_file(&entry.tmp_path);
            }
        }

        /// Move every staged file into place, or leave the directory as it
        /// was, and return the paths written.
        ///
        /// A rename consumes whatever is already at the destination, so each
        /// one moves that file aside first and only deletes the backup once
        /// the whole set has landed. Deleting eagerly is what made a failure
        /// on the last file take the previous build with it: a list of "files
        /// we wrote" mixes ones this run created with ones it replaced, and
        /// undoing it deleted both.
        ///
        /// Consuming `self` so the set cannot be committed twice, nor
        /// discarded after its scratch files have been renamed away.
        ///
        /// # Errors
        ///
        /// Returns the first rename that failed, after undoing the ones
        /// before it.
        pub fn commit(self) -> Result<Vec<PathBuf>, CommitFailure> {
            let mut displaced: Vec<Displaced> = Vec::new();
            let mut committed: Vec<PathBuf> = Vec::new();

            for entry in &self.entries {
                match occupant(&entry.final_path) {
                    Ok(Occupant::Movable) => {
                        let backup_path = suffixed(&entry.final_path, ".bak");
                        if let Err(err) = fs::rename(&entry.final_path, &backup_path) {
                            undo(&displaced, &committed);
                            self.discard_scratch();
                            return Err(CommitFailure {
                                kind: entry.kind,
                                step: Step::Displace,
                                path: backup_path,
                                source: err,
                            });
                        }
                        displaced.push(Displaced {
                            final_path: entry.final_path.clone(),
                            backup_path,
                        });
                    }
                    Ok(Occupant::Vacant | Occupant::Directory) => {}
                    Err(err) => {
                        undo(&displaced, &committed);
                        self.discard_scratch();
                        return Err(CommitFailure {
                            kind: entry.kind,
                            step: Step::Displace,
                            path: entry.final_path.clone(),
                            source: err,
                        });
                    }
                }
                if let Err(err) = fs::rename(&entry.tmp_path, &entry.final_path) {
                    undo(&displaced, &committed);
                    self.discard_scratch();
                    return Err(CommitFailure {
                        kind: entry.kind,
                        step: Step::Place,
                        path: entry.final_path.clone(),
                        source: err,
                    });
                }
                committed.push(entry.final_path.clone());
            }

            for entry in &displaced {
                if let Err(err) = fs::remove_file(&entry.backup_path) {
                    // The build succeeded, so this is not a failure — but
                    // the leftover is a copy of the previous build under a
                    // name that looks like scratch, and nothing later will
                    // come back for it.
                    eprintln!(
                        "warning: `{}` was left behind after replacing `{}`: {err}",
                        entry.backup_path.display(),
                        entry.final_path.display(),
                    );
                }
            }
            Ok(committed)
        }

        /// `discard` by reference, for the undo paths that still need the
        /// entry list afterwards.
        fn discard_scratch(&self) {
            for entry in &self.entries {
                let _ = fs::remove_file(&entry.tmp_path);
            }
        }
    }

    /// Put the directory back the way the commit found it.
    ///
    /// Every step is reported on failure rather than swallowed. A half-
    /// restored directory the operator knows about beats one they do not,
    /// and the delete loop in particular has no later step that would
    /// surface its failure: an entry that was created rather than replaced
    /// appears in `committed` and never in `displaced`, so nothing touches
    /// its path again. On Windows that is not hypothetical — deleting a file
    /// another process holds open fails, while renaming onto a vacant name
    /// succeeds regardless.
    fn undo(displaced: &[Displaced], committed: &[PathBuf]) {
        for path in committed {
            if let Err(err) = fs::remove_file(path) {
                eprintln!(
                    "error: `{}` was written by a build that then failed, and could not be \
                     removed: {err}",
                    path.display(),
                );
            }
        }
        for entry in displaced {
            let _ = fs::remove_file(&entry.final_path);
            if let Err(err) = fs::rename(&entry.backup_path, &entry.final_path) {
                eprintln!(
                    "error: `{}` could not be restored from `{}`: {err}",
                    entry.final_path.display(),
                    entry.backup_path.display(),
                );
            }
        }
    }
}

fn resolve_out_dir(source: &Path, requested: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = requested {
        return Some(p.to_path_buf());
    }
    let parent = source.parent()?;
    // `Path::parent` returns `Some("")` for a bare filename like `foo.crn`;
    // treat that as "current directory" so the obvious one-file invocation
    // still works.
    Some(if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    })
}

/// Append a `.lock` suffix to the source file name so multiple `.crn`
/// files in the same directory get distinct locks. `Path::with_extension`
/// would drop `.crn`, fusing `cottage.crn`'s lock with any other
/// `cottage.*` source's lock.
fn default_lock_path(source: &Path) -> PathBuf {
    let mut p = source.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

fn build_lockfile(
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
    target: &ResolvedTarget,
) -> Result<Lockfile, cairn_lang_core::lock::HashError> {
    Ok(Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: hash_source(source),
        cairn_version: CAIRN_VERSION.to_owned(),
        target: LockTarget {
            edition: edition.as_lock_edition(),
            mc_version: target.mc_version().to_owned(),
            data_version: target.version_int(),
        },
        inputs: LockInputs {
            // The registry pack ingest replaces the hardcoded version
            // table; its bytes hash pins the exact (mc_version, version
            // integer) resolution rules a downstream re-compile must
            // match, keyed to the edition being compiled. The constraint
            // catalog ingest will fill the second field once catalogs
            // ship; until then it stays zero (per `LockInputs::zero`'s
            // contract).
            registry_pack_hash: target.registry_pack().bytes_hash.clone(),
            constraint_catalog_hash: HashHex::zero(),
        },
        resolved_ir_hash: hash_resolved_ir(block_ir)?,
        verified: true,
        member_version_sensitivity: vec![],
        placements: block_ir
            .placements
            .values()
            .map(|p| LockPlacement {
                site: p.site.clone(),
                id: p.place_id.clone(),
                def: p.source_def.clone(),
                theme: p.theme.clone(),
                origin: [p.origin.0, p.origin.1, p.origin.2],
                dims: [p.dims.x, p.dims.y, p.dims.z],
            })
            .collect(),
        walkways: block_ir
            .walkways
            .values()
            .map(|w| {
                // `Footprint::to_dims_y1` is the single place that
                // re-attaches the implicit `y = 1` for the lockfile's
                // `dims: [u32; 3]` wire format; the block-array IR's
                // own `dims.y` invariant is asserted at
                // `lower_connects`'s Footprint construction site.
                let d = w.footprint.to_dims_y1();
                LockWalkway {
                    site: w.site.clone(),
                    from: w.from.clone(),
                    to: w.to.clone(),
                    path_material: w.path_material.clone(),
                    origin: [w.origin.0, w.origin.1, w.origin.2],
                    dims: [d.x, d.y, d.z],
                }
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    //! Unit coverage for the argument-surface invariants the
    //! end-to-end `tests/cli_*.rs` binaries can only assert
    //! circumstantially, by hard-coding both sides of a pairing.
    use super::*;

    /// The whole note block, header included.
    ///
    /// Nothing else reads these lines: the end-to-end tests look for an id
    /// and a clause inside them, which leaves the header, the guard, the
    /// order and the indent free to change or vanish. The header is the
    /// part that matters most — it names the figure on stdout that the
    /// lines below explain, and one that disagreed with the row would be
    /// worse than no header at all.
    #[test]
    fn the_notes_answer_the_figure_they_sit_under() {
        let entries = vec![
            UnsupportedEntry {
                id: "minecraft:oak_sign".to_owned(),
                reason: UnsupportedReason::AbsentFromEdition {
                    suggestion: Some("minecraft:oak_log".to_owned()),
                },
            },
            UnsupportedEntry {
                id: "minecraft:oak_stairs".to_owned(),
                reason: UnsupportedReason::StateKeyUnread {
                    key: "waterlogged".to_owned(),
                    handled: "facing, half, shape".to_owned(),
                },
            },
        ];
        assert_eq!(
            unsupported_notes(Edition::Bedrock, &entries),
            [
                "note: what `unsupported: 2` counts on bedrock:".to_owned(),
                format!(
                    "  note: `minecraft:oak_sign` — {}",
                    unsupported_reason(&entries[0].reason)
                ),
                format!(
                    "  note: `minecraft:oak_stairs` — {}",
                    unsupported_reason(&entries[1].reason)
                ),
            ],
        );
        // A clean source says nothing rather than announcing a count of
        // nothing, and every edition would otherwise get a block of its
        // own back to back.
        assert!(unsupported_notes(Edition::Java, &[]).is_empty());
    }

    /// Each `unsupported` reason renders the repair it names, including
    /// the three no `.crn` can reach.
    ///
    /// Two paths put blockstate properties on a palette entry, and
    /// neither reaches these branches. `roof::stair_state` builds them
    /// from `Cardinal` and `StairShape` and only for a material the
    /// family check already accepted, so its values are in domain by
    /// construction; an authored `@id[k=v]` token would carry arbitrary
    /// ones, and the lexer refuses the bracket. A registry pack cannot
    /// supply them either — `PackView::lookup` answers with
    /// `BlockState::bare`. So the end-to-end tests can only ever produce
    /// the absent-id case. The rendering is a pure function of the reason,
    /// so the other three are asked here rather than left as the branches
    /// nothing reads.
    #[test]
    fn every_unsupported_reason_renders_the_repair_it_names() {
        let bare = unsupported_reason(&UnsupportedReason::AbsentFromEdition { suggestion: None });
        assert!(
            bare.contains("declares the block") && !bare.contains("did you mean"),
            "no suggestion means no dangling clause, got: {bare}",
        );
        let suggested = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
            suggestion: Some("minecraft:oak_slab".to_owned()),
        });
        assert!(
            suggested.contains("did you mean `minecraft:oak_slab`?"),
            "got: {suggested}",
        );
        // The block is not missing from the edition and the edition is not
        // what cannot express the states — this compiler is, so far.
        let unmapped = unsupported_reason(&UnsupportedReason::StatesUnmapped {
            states: "facing=north".to_owned(),
            mapped: "the stair family".to_owned(),
        });
        assert!(
            unmapped.contains("facing=north")
                && unmapped.contains("has the block")
                && unmapped.contains("the stair family")
                && unmapped.contains("so far"),
            "the gap is this compiler's and it is not permanent, got: {unmapped}",
        );
        // Nothing to edit: the value should not have reached the
        // translator, and saying so is what stops the search.
        let value = unsupported_reason(&UnsupportedReason::StateValueUnexpected {
            key: "facing".to_owned(),
            value: "up".to_owned(),
            valid: "east, west, south, north".to_owned(),
        });
        assert!(
            value.contains("`facing=up`")
                && value.contains("east, west, south, north")
                && value.contains("not yours to repair"),
            "got: {value}",
        );
        // The one the author can act on, so the fix survives the render.
        let key = unsupported_reason(&UnsupportedReason::StateKeyUnread {
            key: "waterlogged".to_owned(),
            handled: "facing, half, shape".to_owned(),
        });
        assert!(
            key.contains("`waterlogged`")
                && key.contains("facing, half, shape")
                && key.contains("remove it from the source blockstate"),
            "the author's repair must survive the rendering, got: {key}",
        );
    }

    /// The four Placement IR stages spell their `--stage` value, the
    /// `--stage <name>` fragment `require_edition` prints, and the
    /// `"stage"` key of the JSON dump the same way. `stage_cli_name`
    /// already derives the second from the third, but the first is
    /// clap's own kebab-casing of the variant identifier, which no
    /// type ties to either — renaming `SynthStage::Route` to
    /// `Routing` would silently start accepting `--stage routing`
    /// while the dump kept saying `route`. Reading the name back out
    /// of `ValueEnum` is the only way to pin that.
    #[test]
    fn placement_stage_names_match_clap() {
        for (stage, placement) in [
            (SynthStage::Placement, PlacementStage::Placement),
            (SynthStage::Route, PlacementStage::Route),
            (SynthStage::Delay, PlacementStage::Delay),
            (SynthStage::Crossing, PlacementStage::Crossing),
        ] {
            let clap_name = stage
                .to_possible_value()
                .expect("no SynthStage variant is skipped");
            assert_eq!(clap_name.get_name(), placement.as_str());
            assert_eq!(stage_cli_name(stage), placement.as_str());
        }
    }

    /// The edition-neutral stages have no Placement IR counterpart,
    /// so their spellings stay literals in `stage_cli_name` — pinned
    /// here against clap for the same reason.
    #[test]
    fn edition_neutral_stage_names_match_clap() {
        for stage in [SynthStage::Logic, SynthStage::Netlist, SynthStage::Edition] {
            let clap_name = stage
                .to_possible_value()
                .expect("no SynthStage variant is skipped");
            assert_eq!(clap_name.get_name(), stage_cli_name(stage));
        }
    }

    /// `stage_requires_edition` decides two user-visible behaviours at
    /// once — whether `--edition` is refused as stray and whether its
    /// absence is a usage error — so the set it names is pinned here
    /// against the `--stage` spellings a caller actually types. The
    /// loop walks `ValueEnum`'s full variant list rather than a
    /// hand-written one, so a stage added without a decision on its
    /// edition-dependence fails here instead of quietly joining the
    /// edition-neutral side.
    #[test]
    fn edition_required_stages_match_the_documented_set() {
        for stage in SynthStage::value_variants() {
            let name = stage
                .to_possible_value()
                .expect("no SynthStage variant is skipped");
            let expected = match name.get_name() {
                "logic" | "netlist" => false,
                "edition" | "placement" | "route" | "delay" | "crossing" => true,
                other => panic!("unclassified --stage value `{other}`"),
            };
            assert_eq!(
                stage_requires_edition(*stage),
                expected,
                "--stage {} edition-dependence",
                name.get_name(),
            );
        }
    }

    /// The stray-`--edition` message reads its two stage lists off
    /// `stage_requires_edition`, which keeps them honest but puts the
    /// English between them — the commas, the conjunction, the
    /// backticks — under no other check. Pinned here so the derivation
    /// cannot start producing a list that is correct and unreadable.
    #[test]
    fn stray_edition_message_lists_read_as_english() {
        assert_eq!(
            edition_required_stage_list(),
            "`--stage edition`, `--stage placement`, `--stage route`, \
             `--stage delay`, or `--stage crossing`",
        );
        assert_eq!(edition_neutral_stage_list(), "`logic` and `netlist`");
    }
}
