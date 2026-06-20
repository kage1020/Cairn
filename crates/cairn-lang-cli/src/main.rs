//! Cairn command-line entry point.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::lock::{LockInputs, LockTarget, Lockfile, hash_resolved_ir, hash_source};
use cairn_lang_core::resolve::{VersionAxes, compute_axes, resolve};
use cairn_lang_core::{Severity, check, lower, parse};
use cairn_lang_formats::data_version::resolve_java_target;
use cairn_lang_formats::java_structure::{output_filename, write_structure_gzip};
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
    /// Compile a .crn source file to its edition+version-pinned NBT artifact
    /// set and write a lockfile next to the source. The Java backend in
    /// this release supports `floor` and `walls` only; other roles degrade
    /// to air with a `W_DEFERRED_MEMBER` warning and the build still
    /// succeeds, matching `cairn lower`. Exits 0 on success, 1 on parse,
    /// lowering, or I/O failure (including an unsupported `--target`),
    /// and 2 when the source file cannot be located.
    Compile {
        /// Path to the .crn file to compile.
        file: PathBuf,
        /// Target edition. Required by spec §4.2 (`--target` alone is
        /// forbidden).
        #[arg(long, value_enum)]
        edition: EditionArg,
        /// Minecraft version string. Resolved against the backend's data
        /// table; opaque label per spec §10.1. `latest` aliases the newest
        /// version this release knows.
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
    /// Java Edition. The only fully implemented backend in this release.
    Java,
    /// Bedrock Edition. Reserved for M4; passing it here exits with a
    /// dedicated error so the CLI surface is stable now.
    Bedrock,
}

impl EditionArg {
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
        Some(Command::Check { file, format }) => run_check(&file, format),
        Some(Command::Info {
            file,
            editions,
            format,
        }) => run_info(&file, &editions, format),
        Some(Command::Lower { file, format }) => run_lower(&file, format),
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
    let resolution = resolve(&ir);
    let block_ir = lower_to_block_array(&ir, &resolution);

    let lines = LineStarts::new(&source);
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
/// renders as `?` — debug-format only, and well above M2's expected
/// per-structure palette size (cottage uses 3 entries), but worth a glance
/// before reading a `?`-heavy slice as evidence of broken lowering.
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

fn run_compile(
    file: &Path,
    edition: EditionArg,
    target: &str,
    out: Option<&Path>,
    lock: Option<&Path>,
) -> ExitCode {
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
    let block_ir = lower_to_block_array(&ir, &resolution);

    // Surface lowering warnings on stderr (deferred members etc.) before any
    // backend action — matches `cairn lower` so a CI log shows the same set
    // regardless of which subcommand was run.
    let lines = LineStarts::new(&source);
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
    }

    let target = match edition {
        EditionArg::Bedrock => {
            eprintln!("error: --edition bedrock is not implemented in this release; Java only");
            return ExitCode::from(1);
        }
        EditionArg::Java => match resolve_java_target(target) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::from(1);
            }
        },
    };

    let out_dir: PathBuf = match out {
        Some(p) => p.to_path_buf(),
        None => file
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
    };
    if let Err(err) = std::fs::create_dir_all(&out_dir) {
        eprintln!(
            "error: cannot create output directory `{}`: {err}",
            out_dir.display(),
        );
        return ExitCode::from(1);
    }

    for (scope, ba) in &block_ir.structures {
        let name = output_filename(scope);
        let path = out_dir.join(&name);
        let mut f = match std::fs::File::create(&path) {
            Ok(f) => f,
            Err(err) => {
                eprintln!("error: cannot open `{}` for write: {err}", path.display());
                return ExitCode::from(1);
            }
        };
        if let Err(err) = write_structure_gzip(&mut f, ba, target) {
            eprintln!("error: writing `{}`: {err}", path.display());
            return ExitCode::from(1);
        }
        println!("wrote {}", path.display());
    }

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    let lockfile = build_lockfile(&source, &block_ir, edition, target);
    if let Err(err) = lockfile.write_to_path(&lock_path) {
        eprintln!("error: writing lockfile `{}`: {err}", lock_path.display());
        return ExitCode::from(1);
    }
    println!("wrote {}", lock_path.display());

    ExitCode::SUCCESS
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
    target: cairn_lang_formats::data_version::JavaTarget,
) -> Lockfile {
    Lockfile {
        source_hash: hash_source(source),
        cairn_version: CAIRN_VERSION.to_owned(),
        target: LockTarget {
            edition: edition.as_str().to_owned(),
            mc_version: target.mc_version.to_owned(),
            data_version: target.data_version,
        },
        inputs: LockInputs::zero(),
        resolved_ir_hash: hash_resolved_ir(block_ir),
        verified: true,
        member_version_sensitivity: vec![],
    }
}
