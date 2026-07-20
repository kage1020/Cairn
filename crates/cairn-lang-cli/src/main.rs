//! Cairn command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::lock::{
    HashHex, LockEdition, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile,
    hash_resolved_ir, hash_source,
};
use cairn_lang_core::resolve::{EditionPortability, VersionAxes, compute_axes, resolve};
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
    compile_crossing, compile_delay, compile_edition_netlist, compile_netlist, compile_placement,
    compile_routing, synthesize,
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
    /// Lower a .crn source file all the way to the block-array IR and print
    /// the result. A debugging surface for the universal voxel pivot;
    /// `cairn compile` writes the same IR out as a Java `.nbt` artifact.
    /// Lowering warnings (deferred members, themeless scopes, abstract
    /// tokens) print to stderr but do not affect the exit code. Exits 0 on
    /// success, 1 on parse failure or I/O error, 2 when the file cannot be
    /// located.
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
    /// uncompressed `.mcstructure` files. The Bedrock backend emits
    /// stateless palettes only for now — a palette entry that carries
    /// blockstate properties is a hard error rather than a silent drop.
    /// Exits 0 on success, 1 on parse, lowering, or I/O failure (including
    /// an unsupported `--target` or a stateful Bedrock palette), and 2
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
    /// driver segment beyond the `DUST_ATTENUATION_LIMIT`;
    /// `--stage crossing` runs crossing legalization over the delayed
    /// IR, refuses with `E_CROSSING_CONGESTION` when a cross-net
    /// plane overlap cannot fit inside the `void=<N>` reservation,
    /// and fills every cell's `buffer_coords` with the concrete
    /// coord of each implicit buffer repeater (escaping to a
    /// `RouteLayer::Bridge` y-layer whenever the plane candidate
    /// collides with a cell / pad / plane crossing / earlier
    /// buffer). The `--edition <java|bedrock>` flag is required in
    /// the `edition`, `placement`, `route`, `delay`, and `crossing`
    /// modes and refused otherwise (the earlier stages are
    /// edition-neutral by contract).
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
    /// with the sum of Manhattan distances from each driver source
    /// into the cell; `delay_ticks` stays `None` until the
    /// delay-insertion pass (stage 3) runs.
    Route,
    /// Delayed Placement IR: delay insertion over the routed Placement
    /// IR against `--edition`. Stage 3 of `spec/redstone` §14.5's
    /// place-and-route pipeline. Fills every cell's `delay_ticks`
    /// with the sum of the cell's physical base delay
    /// ([`cairn_lang_redstone::EditionCell::base_delay_ticks`]) and
    /// each implicit buffer repeater's
    /// [`cairn_lang_redstone::BUFFER_REPEATER_TICKS`] contribution
    /// implied by driver segments beyond
    /// [`cairn_lang_redstone::DUST_ATTENUATION_LIMIT`]; refuses with
    /// `E_ATTENUATION_LIMIT` when a segment exceeds the v1 sanity cap
    /// [`cairn_lang_redstone::MAX_ATTENUATION_SEGMENT`], the threshold
    /// past which a stage-4 crossing-legalization escape becomes
    /// unavoidable.
    Delay,
    /// Legalized Placement IR: crossing legalization over the delayed
    /// Placement IR against `--edition`. Stage 4 of `spec/redstone`
    /// §14.5's place-and-route pipeline. Detects wire coords two
    /// distinct nets would otherwise share on the ground plane
    /// (refused with `E_CROSSING_CONGESTION` when the
    /// `circuit region=<label> void=<N>` reservation offers no
    /// y-layer to escape to) and materialises the concrete coord of
    /// every implicit buffer repeater the delay pass counted into
    /// every cell's `buffer_coords`. A buffer whose plane candidate
    /// collides with a cell / pad / plane crossing / earlier buffer
    /// escapes to the first free `RouteLayer::Bridge` y-layer inside
    /// the `void=<N>` budget; if every bridge y-layer at that
    /// `(x, z)` is also taken, refuses with
    /// `E_BUFFER_COORD_COLLISION`. v1 does not lift the wire
    /// crossing itself onto `Bridge` — the routed wire path is not
    /// carried on the IR, and stage-5 block-array lowering re-runs
    /// the routing algorithm to derive the crossings itself.
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

    // Surface resolver + lowering diagnostics before running the parity
    // dry-run — the same contract `run_check` / `run_lower` / `run_compile`
    // already honor. Without this, a `.crn` carrying an
    // `E_UNRESOLVED_SLOT` (or any other Error-severity finding) would
    // still get `cairn info` exit 0 with a portability row computed
    // against a partially unresolved IR, which is a poor CI gate.
    //
    // The diagnostic set follows `cairn check` semantics — `resolve(ir,
    // None)` unions slot names across per-edition variants — so a file
    // whose only "problem" is that one variant declares a slot the other
    // doesn't still passes here. A per-edition strict pass (`resolve(ir,
    // Some(edition))`) is what the parity dry-run below runs; its
    // downstream lowering may add lowering-level diagnostics that are
    // edition-agnostic in practice.
    let resolution = resolve(&ir, None);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().materials));
    let mut combined = resolution.diagnostics.clone();
    combined.append(&mut block_ir.diagnostics);

    let lines = LineStarts::new(&source);
    let mut has_error = false;
    for d in &combined {
        let pos = lines.position(&source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity.as_str(),
            d.code.as_str(),
            d.primary,
        );
        for note in &d.notes {
            eprintln!("  note: {}", note.message);
        }
        if d.severity == Severity::Error {
            has_error = true;
        }
    }
    if has_error {
        return ExitCode::from(1);
    }

    // One dry-run lower per requested edition: the resolver's per-edition
    // theme variant selection can produce a different palette per edition
    // (the whole point of spec §10.7 hierarchy #2), so a single shared
    // block-array IR would misrepresent the parity axis.
    //
    // The lowering never writes files here — it stops at the in-memory
    // `BlockArrayIr` that `portability_for_*` inspects.
    let mut per_edition: Vec<EditionPortability> = Vec::with_capacity(editions.len());
    for e in editions {
        let edition: Edition = e.parse().expect("validated above");
        let per_edition_resolution = resolve(&ir, Some(edition));
        let materials = match edition {
            Edition::Java => &builtin_java().materials,
            Edition::Bedrock => &builtin_bedrock().materials,
        };
        let per_block_ir = lower_to_block_array(&ir, &per_edition_resolution, Some(materials));
        let counts = match edition {
            Edition::Java => portability_for_java(&per_block_ir),
            Edition::Bedrock => portability_for_bedrock(&per_block_ir),
        };
        per_edition.push(EditionPortability {
            edition,
            portable: counts.portable,
            degraded: counts.degraded,
            unsupported: counts.unsupported,
        });
    }

    let axes = compute_axes(&module, &ir, &resolution, per_edition);

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
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().materials));
    // Mirror `load_and_lower`: semantic findings produced by the resolver
    // belong on the same diagnostic stream as the lowering deferrals they
    // tend to cascade into.
    let mut combined = resolution.diagnostics;
    combined.append(&mut block_ir.diagnostics);
    block_ir.diagnostics = combined;

    let lines = LineStarts::new(&source);
    let mut has_error = false;
    for d in &block_ir.diagnostics {
        let pos = lines.position(&source, d.span.start);
        eprintln!(
            "{}:{}: {}[{}]: {}",
            file.display(),
            pos,
            d.severity.as_str(),
            d.code.as_str(),
            d.primary,
        );
        for note in &d.notes {
            eprintln!("  note: {}", note.message);
        }
        if d.severity == Severity::Error {
            has_error = true;
        }
    }

    let success_exit = if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    };

    match format {
        LowerFormat::Ascii => {
            print_block_ir_ascii(&block_ir);
            success_exit
        }
        LowerFormat::Json => match serde_json::to_string_pretty(&block_ir) {
            Ok(json) => {
                println!("{json}");
                success_exit
            }
            Err(err) => {
                eprintln!("error: failed to serialise block-array IR as JSON: {err}");
                ExitCode::from(1)
            }
        },
        LowerFormat::Debug => {
            println!("{block_ir:#?}");
            success_exit
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
    if !matches!(
        stage,
        SynthStage::Edition
            | SynthStage::Placement
            | SynthStage::Route
            | SynthStage::Delay
            | SynthStage::Crossing,
    ) && edition.is_some()
    {
        eprintln!(
            "error: `--edition` is only meaningful with `--stage edition`, `--stage placement`, `--stage route`, `--stage delay`, or `--stage crossing`; the `logic` and `netlist` stages are edition-neutral",
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

    // Mirror `run_check` / `run_lower` / `run_compile`: surface
    // resolver + check diagnostics before running the redstone synth. A
    // `.crn` whose only problem is `E_UNRESOLVED_SLOT` or a typo caught
    // by `check` would otherwise exit 0 through the synth path with a
    // partially resolved IR, which is a poor CI gate.
    let resolution = resolve(&ir, None);
    let mut has_error = report_core_diagnostics(file, &source, &lines, &resolution.diagnostics);
    let check_diagnostics = check(&module, &ir, None);
    if report_core_diagnostics(file, &source, &lines, &check_diagnostics) {
        has_error = true;
    }
    if has_error {
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

    let netlist = compile_netlist(&synth.scoped);
    if matches!(stage, SynthStage::Netlist) {
        return Ok((serde_json::to_string_pretty(&netlist), "Netlist IR"));
    }

    let edition = require_edition(edition, stage_flag(stage))?.as_edition();
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
/// `SynthStage` variant names, used by `require_edition` when it
/// composes the `--stage <name>` fragment of its stderr message. The
/// canonical spelling is whatever clap accepts on the command line
/// (derived from `#[derive(ValueEnum)]` on `SynthStage`); this
/// function must be kept in sync on every variant addition or
/// rename. Its exhaustive `match` provides a compile-time nudge to
/// do so.
fn stage_flag(stage: SynthStage) -> &'static str {
    match stage {
        SynthStage::Logic => "logic",
        SynthStage::Netlist => "netlist",
        SynthStage::Edition => "edition",
        SynthStage::Placement => "placement",
        SynthStage::Route => "route",
        SynthStage::Delay => "delay",
        SynthStage::Crossing => "crossing",
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
            d.severity.as_str(),
            d.code.as_str(),
            d.primary,
        );
        for note in &d.notes {
            if let Some(span) = note.span.as_ref() {
                let note_pos = lines.position(source, span.start);
                eprintln!("{}:{}:   note: {}", file.display(), note_pos, note.message);
            } else {
                eprintln!("  note: {}", note.message);
            }
        }
        if d.severity == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

/// Print redstone synth diagnostics in the same format the core passes
/// use. Kept as a separate function because the finding type is
/// crate-local — merging the two would require an `impl` trait bound on
/// the diagnostic shape that neither side owns.
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
            d.severity.as_str(),
            d.code.as_str(),
            d.primary,
        );
        for note in &d.notes {
            if let Some(span) = note.span.as_ref() {
                let note_pos = lines.position(source, span.start);
                eprintln!("{}:{}:   note: {}", file.display(), note_pos, note.message);
            } else {
                eprintln!("  note: {}", note.message);
            }
        }
        if d.severity == Severity::Error {
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
    let (source, block_ir) = match load_and_lower(file, edition) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if report_lowering_diagnostics(file, &source, &block_ir) {
        return ExitCode::from(1);
    }

    let target = match resolve_target(edition, target) {
        Ok(t) => t,
        Err(code) => return code,
    };

    let out_dir = match prepare_out_dir(file, out) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let prepared = match prepare_artifacts(&block_ir, &target, &out_dir) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    write_artifacts_and_lock(&prepared, &source, &block_ir, edition, &target, &lock_path)
}

fn load_and_lower(file: &Path, edition: EditionArg) -> Result<(String, BlockArrayIr), ExitCode> {
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
    // The materials catalog is edition-specific: an abstract `@token`
    // resolves through the pack whose backend will serialise it, so a
    // future per-edition block vocabulary lowers correctly without a
    // second lowering pass.
    let materials = match edition {
        EditionArg::Java => &builtin_java().materials,
        EditionArg::Bedrock => &builtin_bedrock().materials,
    };
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(materials));
    // Resolver diagnostics (`E_UNRESOLVED_PLACE_REF`, `E_UNRESOLVED_SLOT`,
    // `W_UNUSED_DEF`, ...) are produced before lowering and must still reach
    // the CLI's diagnostic stream — otherwise a `place use=cottag` typo
    // (which the resolver flags as an Error) would silently produce zero
    // `.nbt` files at exit 0. Prepend so semantic problems read above the
    // lowering deferrals that may have cascaded from them.
    let mut combined = resolution.diagnostics;
    combined.append(&mut block_ir.diagnostics);
    block_ir.diagnostics = combined;
    Ok((source, block_ir))
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
            d.severity.as_str(),
            d.code.as_str(),
            d.primary,
        );
        for note in &d.notes {
            eprintln!("  note: {}", note.message);
        }
        if d.severity == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

fn resolve_target(edition: EditionArg, target: &str) -> Result<ResolvedTarget, ExitCode> {
    match edition {
        EditionArg::Java => resolve_java_target(target)
            .map(ResolvedTarget::Java)
            .map_err(|err| {
                eprintln!("error: {err}");
                ExitCode::from(1)
            }),
        EditionArg::Bedrock => resolve_bedrock_target(target)
            .map(ResolvedTarget::Bedrock)
            .map_err(|err| {
                eprintln!("error: {err}");
                ExitCode::from(1)
            }),
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

/// Write the prepared structure files and the lockfile, rolling back
/// every already-written file (and the lockfile) on any failure so the
/// on-disk state stays consistent — either every artifact + the lock, or
/// none.
fn write_artifacts_and_lock(
    prepared: &[(PathBuf, Compound)],
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
    target: &ResolvedTarget,
    lock_path: &Path,
) -> ExitCode {
    let mut written: Vec<PathBuf> = Vec::with_capacity(prepared.len());
    for (path, tag) in prepared {
        if let Err(err) = write_tag_atomically(path, tag, target) {
            rollback(&written, None);
            eprintln!("error: writing `{}`: {err}", path.display());
            return ExitCode::from(1);
        }
        written.push(path.clone());
    }

    let lockfile = match build_lockfile(source, block_ir, edition, target) {
        Ok(lf) => lf,
        Err(err) => {
            rollback(&written, None);
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if let Err(err) = lockfile.write_to_path(lock_path) {
        rollback(&written, None);
        eprintln!("error: writing lockfile `{}`: {err}", lock_path.display());
        return ExitCode::from(1);
    }

    for path in &written {
        println!("wrote {}", path.display());
    }
    println!("wrote {}", lock_path.display());
    ExitCode::SUCCESS
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

fn write_tag_atomically(
    final_path: &Path,
    tag: &Compound,
    target: &ResolvedTarget,
) -> Result<(), std::io::Error> {
    use std::io::Write as _;

    // Write to a sibling `.tmp` file then rename so an interrupted write
    // (process kill, disk full mid-stream) never leaves a half-encoded
    // structure at the real path.
    let mut tmp_path = final_path.as_os_str().to_owned();
    tmp_path.push(".tmp");
    let tmp_path = PathBuf::from(tmp_path);

    // Any failure before the rename must clean up the partial `.tmp` so a
    // retry does not accumulate orphans (and the caller's rollback, which
    // only knows the final paths, cannot reach it).
    let result = (|| {
        let mut f = std::fs::File::create(&tmp_path)?;
        target.write_tag(&mut f, tag)?;
        f.flush()?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp_path, final_path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

fn rollback(written: &[PathBuf], lock_path: Option<&Path>) {
    for path in written {
        let _ = std::fs::remove_file(path);
    }
    if let Some(p) = lock_path {
        let _ = std::fs::remove_file(p);
    }
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
