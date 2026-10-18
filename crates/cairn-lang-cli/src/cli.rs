//! The argument surface: the subcommands, their flags and the values those flags take.

use std::path::PathBuf;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::Edition;
use clap::builder::{PossibleValue, PossibleValuesParser, TypedValueParser};
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
    #[command(
        long_about = "Run syntactic validation passes against a .crn source file. Exits 0 when \
            nothing `Error`-severity is reported, 1 when any `Error`-severity diagnostic is \
            emitted (or the file fails to parse, or cannot be read for a reason other than \
            absence, or `--target` names no version the pinned edition ships, or a pinned run \
            loses a scope), 2 when the file cannot be located or `--target` is given without \
            `--edition`.\n\n\
            Without `--target` this command runs no block-array lowering, so no lowering-stage \
            finding reaches it — `E_UNKNOWN_ID` and `E_UNKNOWN_ABSTRACT_TOKEN` among them. \
            `--edition E --target V` pins the one `(edition, version)` those findings are answers \
            about and runs the lowering pass too, which is what lets a CI job gate on `cairn \
            check` and see the lowering-stage findings `cairn compile` would refuse on, plus the \
            `E_PARTIAL_BUILD` a lost scope earns. Nothing is written either way — `compile` \
            remains the command that produces artifacts and the lockfile, and the only one that \
            holds `--target` to the file's `@requires` floors (`E_VERSION_CAP`).\n\n\
            The two run-level refusals — an unshipped `--target` and a lost scope — are reported \
            on stderr and by the exit code in both formats, never as elements of the `--format \
            json` array: neither is a finding at a span in the file, and inventing one would put a \
            line number on a fact that has none. This is `compile`'s shape for both, and the exit \
            code is what a JSON consumer reads them from: an exit of 1 over an array with no \
            error-severity element is one, and stderr is prose for a person rather than something \
            to parse.\n\n\
            `compile`'s `--target` defaults to `latest`, so a compile always pins a version while \
            `cairn check --edition java` — the mirror a CI job naturally writes — still runs the \
            unpinned gate. The default is deliberately not copied here: it would refuse ids on a \
            version nobody chose, which is the guess `spec/versioning-editions` \"Fail-loud and \
            minimum-version inference\" rules out."
    )]
    Check {
        /// Path to the .crn file to check.
        file: PathBuf,
        /// Edition to check against; omitted, slots resolve across both editions' theme variants.
        #[arg(
            long,
            value_parser = edition_parser(),
            long_help = "Optional edition pin. When set, per-edition theme variants \
                (`spec/versioning-editions` \"Java / Bedrock portability\") are resolved for the \
                picked edition specifically — a `mat_slot=X` reference to a slot only the *other* \
                variant declares fires `E_UNRESOLVED_SLOT`. When omitted, the resolver unions slot \
                names across both variants of one logical theme so the file passes `check` \
                regardless of which edition it ends up compiling for.\n\n\
                The pin also names the version table `@intended_targets` is weighed in. Without \
                one both editions weigh it, since a floor refusing every version the same file \
                says it is for is a mistake either way; `W_INTENDED_TARGET_UNSUPPORTED` is the one \
                finding that waits until a single edition is in scope, because a version Java \
                cannot build is routinely the Bedrock target the author means."
        )]
        edition: Option<Edition>,
        /// Minecraft version to check block ids against, resolved as `compile --target` is.
        // No `latest` default: it would refuse ids on a version nobody chose, the guess
        // `spec/versioning-editions` "Fail-loud and minimum-version inference" rules out.
        #[arg(
            long,
            requires = "edition",
            long_help = "Optional Minecraft version pin, resolved against the pinned edition's \
                data table exactly as `cairn compile --target` is. A version may be spelled with \
                or without trailing `.0` components: `1.21` and `1.21.0` name the same version. \
                `latest` aliases the version that table names as its `latest` row, which is not \
                necessarily the newest row it carries (see `DataVersionTable::latest`).\n\n\
                Requires `--edition`, mirroring the rule in `spec/compilation` \"Target axes\" \
                that `--target` alone is forbidden: \"1.21\" names different releases on Java and \
                Bedrock, and an id table belongs to one `(edition, version)` pair rather than to a \
                version string.\n\n\
                Setting it runs block-array lowering, so the lowering stage's findings join the \
                report — `E_UNKNOWN_ID` above all, which exists only where a target is pinned: \
                `stone_bricks` is a block on Bedrock 1.21.40 and not on Bedrock 1.21.0, so with no \
                version chosen there is no question to answer. A version the edition does not ship \
                still lowers, against the same unpinned view `cairn lower` uses, so every finding \
                that needs no id table is reported before the target is refused. Leaving the flag \
                off keeps the check a syntax-and-resolution gate."
        )]
        target: Option<String>,
        /// Output format for the diagnostics.
        #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
        format: CheckFormat,
    },
    /// Report a .crn source file's version range, edition portability and semantic-sensitive
    /// members.
    #[command(
        long_about = "Report the three version axes (declared registry range, edition portability, \
            semantic-sensitive members) for a .crn source file. Exits 0 on success; 1 on a parse \
            failure, on any `Error`-severity diagnostic (the check passes run here, and a range \
            derived from a file `cairn check` rejects is a confident wrong answer), or on any \
            other I/O error (permission denied, non-UTF-8 contents); 2 when the file cannot be \
            located, and rejects an empty `--editions` value with exit 2"
    )]
    Info {
        /// Path to the .crn file to inspect.
        file: PathBuf,
        /// Comma-separated editions to evaluate portability against.
        #[arg(
            long,
            value_delimiter = ',',
            default_values_t = vec!["java".to_owned(), "bedrock".to_owned()],
            long_help = "Comma-separated editions to evaluate portability against. Each distinct \
                edition produces one entry in the output's `edition portability` and `buildable \
                targets` sections; a repeated name is reported once",
        )]
        editions: Vec<String>,
        /// Output format for the report.
        #[arg(long, value_enum, default_value_t = InfoFormat::Text)]
        format: InfoFormat,
    },
    /// Lower a .crn source file all the way to the block-array IR and print the result.
    #[command(
        long_about = "Lower a .crn source file all the way to the block-array IR and print the \
            result. A debugging surface for the universal voxel pivot; `cairn compile` writes the \
            same IR out as a Java `.nbt` artifact. Lowering warnings (deferred members, themeless \
            scopes, abstract tokens) print to stderr but do not affect the exit code. Exits 0 on \
            success, 1 on a parse failure, on any `Error`-severity diagnostic \
            (`E_UNKNOWN_ABSTRACT_TOKEN` among them; `E_UNKNOWN_ID` is not, since nothing here pins \
            a target to check ids against), or on an I/O error, and 2 when the file cannot be \
            located"
    )]
    Lower {
        /// Path to the .crn file to lower.
        file: PathBuf,
        /// Output format for the lowered block-array IR.
        #[arg(long, value_enum, default_value_t = LowerFormat::Ascii)]
        format: LowerFormat,
    },
    /// Compile a .crn source file to its structure artifacts and write its lockfile.
    #[command(
        long_about = "Compile a .crn source file to its edition+version-pinned structure artifact \
            set and write a lockfile next to the source. `--edition java` writes gzip `.nbt` \
            structures; `--edition bedrock` writes uncompressed `.mcstructure` files, translating \
            the blockstate families it knows into Bedrock `states`; a property it cannot translate \
            is a hard error, and intent it can only approximate (stair `shape`) is dropped with a \
            `W_INTENT_DEGRADED` warning rather than silently. `--target` pins the one version a \
            block id has an answer for, which is what lets the compile check them \
            (`E_UNKNOWN_ID`); `cairn check --edition E --target V` pins the same pair to reach the \
            same finding without building anything. Exits 0 on success, 1 on parse, lowering, or \
            I/O failure (including an unsupported `--target` or a Bedrock property with no \
            `states` translation), and 2 when the source file cannot be located"
    )]
    Compile {
        /// Path to the .crn file to compile.
        file: PathBuf,
        /// Target edition.
        #[arg(
            long,
            value_parser = edition_parser(),
            long_help = "Target edition. Required by `spec/compilation` \"Target axes\" \
                (`--target` alone is forbidden)"
        )]
        edition: Edition,
        /// Minecraft version to compile for, resolved against the edition's data table.
        #[arg(
            long,
            default_value = "latest",
            long_help = "Minecraft version string. Resolved against the backend's data table; \
                opaque label per `spec/versioning-editions` \"The target is a compile-time \
                parameter\". A version may be spelled with or without trailing `.0` components: \
                `1.21` and `1.21.0` name the same version. `latest` aliases the version that table \
                names as its `latest` row, which is not necessarily the newest row it carries (see \
                `DataVersionTable::latest`)"
        )]
        target: String,
        /// Output directory for the structure files; defaults to the source file's directory.
        #[arg(
            long,
            long_help = "Output directory for the generated `.nbt` files. Created if missing. \
                Defaults to the source file's parent directory"
        )]
        out: Option<PathBuf>,
        /// Lockfile path; defaults to `<source>.lock` next to the source.
        #[arg(
            long,
            long_help = "Lockfile path. Defaults to `<source>.lock` next to the source (so \
                `cottage.crn` → `cottage.crn.lock`), keeping per-source locks unambiguous when \
                several `.crn` files share an output directory"
        )]
        lock: Option<PathBuf>,
    },
    /// Experimental: print one stage of a .crn source file's redstone pipeline as JSON.
    #[command(
        long_about = "Lower a .crn source file's `logic` bindings, sensors, and actuators through \
            the redstone pipeline and print an intermediate stage as JSON. `--stage logic` \
            (default) prints the edition-neutral Logic IR DAG; `--stage netlist` prints the \
            Netlist IR of Logical Cells + nets derived from that DAG; `--stage edition` picks the \
            target-edition realisation of each cell and prints the Edition Netlist IR; `--stage \
            placement` lays those edition-tagged cells out inside each scope's `circuit region=` \
            reservation and prints the Placement IR; `--stage route` runs Steiner routing over the \
            Placement IR and prints the routed layout with every cell's `wire_length` populated; \
            `--stage delay` runs delay insertion over the routed IR and fills every cell's \
            `local_delay_ticks` with the sum of the cell's base delay and each implicit buffer \
            repeater's `BUFFER_REPEATER_TICKS` contribution over every driving net's segment that \
            carries more than `DUST_ATTENUATION_LIMIT` blocks of dust — from a fresh source, a \
            segment of more than `DUST_ATTENUATION_LIMIT + 1` steps; `--stage crossing` runs \
            crossing legalization over the delayed IR and fills every cell's `buffer_coords` with \
            the coord of the buffer repeater each driver segment passes through (a repeater this \
            net already placed is recorded rather than duplicated, so one block can be named by \
            several segments). Every cell of the four Placement IR stages carries a `\"stage\"` \
            key echoing the flag value that produced the dump (`placement` / `route` / `delay` / \
            `crossing`), so a consumer reads the stage off the output instead of inferring it from \
            which optional keys are present. The `--edition <java|bedrock>` flag is required in \
            the `edition`, `placement`, `route`, `delay`, and `crossing` modes and refused \
            otherwise (the earlier stages are edition-neutral by contract). **Internal / \
            experimental** — the shape of the output is not covered by the stable compatibility \
            tier and may change at any time. Requires `--experimental-logic-synth` so a caller \
            cannot end up depending on it accidentally.\n\n\
            Exits 0 when the requested stage produced a well-formed IR (warnings still allowed); 1 \
            on a parse failure, on any `Error`-severity diagnostic (from `check` or from a \
            redstone pass the stage runs), on a failure to serialise the IR as JSON, or on any \
            other I/O error (permission denied, non-UTF-8 contents); 2 when the file cannot be \
            located, and refuses a run without `--experimental-logic-synth`, a missing `--edition` \
            on the stages that require it and a stray one on the edition-neutral stages with exit \
            2, before the file is read."
    )]
    Synth {
        /// Path to the .crn file to synthesise.
        file: PathBuf,
        /// Confirm that this subcommand's output is internal and may change at any time.
        #[arg(
            long,
            long_help = "Opt-in flag confirming the caller understands this surface is internal. \
                Required until the redstone pipeline reaches a stable tier; without it the \
                subcommand exits 2 with a hint"
        )]
        experimental_logic_synth: bool,
        /// Which pipeline stage to print.
        #[arg(
            long,
            value_enum,
            default_value_t = SynthStage::Logic,
            long_help = "Which pipeline stage to print. For the four Placement IR stages the value \
                chosen here is echoed back as every cell's `\"stage\"` key, so the flag and the \
                dump share one vocabulary",
        )]
        stage: SynthStage,
        /// Target edition, for the stages after `netlist`.
        #[arg(
            long,
            value_parser = edition_parser(),
            long_help = "Target edition for the Edition Netlist IR / Placement IR / routed \
                Placement IR / delayed Placement IR / legalized Placement IR. Required when \
                `--stage edition`, `--stage placement`, `--stage route`, `--stage delay`, or \
                `--stage crossing` is set; refused for `logic` and `netlist`, which are \
                edition-neutral by contract"
        )]
        edition: Option<Edition>,
    },
    /// Print a page of the documentation this binary carries, the specification among them.
    #[command(
        long_about = "Print a page of the Cairn documentation site as Markdown, from the copy \
            embedded in this binary, so the page describes the compiler printing it and needs no \
            network. Without PAGE, list every page with its title.\n\n\
            PAGE is the path the site serves the page at: `spec/syntax` is \
            https://cairn.kage1020.com/spec/syntax/, and each Japanese page sits under `ja/`. A \
            link inside a page can be passed as written, its slashes and `#fragment` included, as \
            can a URL of the site: `cairn docs /spec/lint/#113-error-vs-warning` prints the \
            section that fragment opens.\n\n\
            TITLE names a part of the page the way `--help` cites the spec: a heading without its \
            number, a term the page defines in bold, or the page's own title. The citation \
            `spec/lint` \"Error vs warning\" reads as `cairn docs spec/lint \"Error vs warning\"`.\
            \n\n\
            Exits 0 when the list, the page or its part is printed, and 2 when PAGE or TITLE \
            names nothing this binary carries."
    )]
    Docs {
        /// The page's path on the site, such as `spec/syntax`, optionally with a `#fragment`.
        page: Option<String>,
        /// A heading, defined term or page title of PAGE, to print instead of the whole page.
        #[arg(requires = "page")]
        title: Option<String>,
        /// List PAGE's headings, each as a link this command takes, instead of printing PAGE.
        #[arg(long, requires = "page", conflicts_with = "title")]
        toc: bool,
    },
}

/// The parser for `--edition`: core's edition names, each with the line `--help` gives it.
fn edition_parser() -> impl TypedValueParser<Value = Edition> {
    PossibleValuesParser::new([Edition::Java, Edition::Bedrock].map(|edition| {
        // Exhaustive, so an edition core adds stops the build here until it is listed.
        let help = match edition {
            Edition::Java => "Java Edition. Emits a gzip-compressed vanilla `.nbt` structure",
            Edition::Bedrock => {
                "Bedrock Edition. Emits an uncompressed little-endian `.mcstructure`; the stair \
                 family's blockstate is mapped to Bedrock `states` and unrepresentable intent \
                 (stair `shape`) degrades with a `W_INTENT_DEGRADED` warning"
            }
        };
        PossibleValue::new(edition.as_str()).help(help)
    }))
    .try_map(|name| name.parse::<Edition>())
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum SynthStage {
    /// Edition-neutral Logic IR DAG produced by `synthesize`.
    Logic,
    /// Netlist IR: Logical Cell selection over the Logic IR.
    #[value(
        help = "Netlist IR: Logical Cell selection over the Logic IR DAG. Still carries no delay \
            per `spec/redstone` \"Time model\" / \"Connection to the IR and phases\""
    )]
    Netlist,
    /// Edition Netlist IR: Edition Cell selection over the Netlist IR.
    #[value(
        help = "Edition Netlist IR: Edition Cell selection over the Netlist IR against \
            `--edition`. The middle tier of the three-tier cell library `spec/redstone` \"Edition \
            differences\" lays out. Still carries no delay"
    )]
    Edition,
    /// Placement IR: coordinate assignment over the Edition Netlist IR.
    #[value(
        help = "Placement IR: 1D coordinate assignment over the Edition Netlist IR against \
            `--edition`. Stage 1 of the pipeline `spec/redstone` \"Place-and-route\" lays out. \
            `wire_length` and `local_delay_ticks` stay `None` at this stage; `--stage route` and \
            `--stage delay` fill them"
    )]
    Placement,
    /// Routed Placement IR: Steiner routing, filling each cell's `wire_length`.
    #[value(
        help = "Routed Placement IR: Steiner routing over the Placement IR against `--edition`. \
            Stage 2 of the pipeline `spec/redstone` \"Place-and-route\" lays out. Fills every \
            cell's `wire_length` with the sum, over the nets driving it, of the routed length from \
            that net's source into the cell; `local_delay_ticks` stays `None` until the \
            delay-insertion pass (stage 3) runs. Refuses with `E_ATTENUATION_LIMIT` before laying \
            a route when a sink is further from its driver, in a straight line, than \
            `MAX_ATTENUATION_SEGMENT` steps: no route is shorter than that line, so stage 3 would \
            refuse it anyway, after laying it"
    )]
    Route,
    /// Delayed Placement IR: delay insertion, filling each cell's `local_delay_ticks`.
    #[value(
        help = "Delayed Placement IR: delay insertion over the routed Placement IR against \
            `--edition`. Stage 3 of the pipeline `spec/redstone` \"Place-and-route\" lays out. \
            Fills every cell's `local_delay_ticks` with the sum (a per-cell wire cost, not an \
            arrival time) of the cell's physical base delay and each implicit buffer repeater's \
            `BUFFER_REPEATER_TICKS` contribution implied by each driving net's segment that \
            carries more than `DUST_ATTENUATION_LIMIT` blocks of dust — from a fresh source, a \
            segment of more than `DUST_ATTENUATION_LIMIT + 1` steps; refuses with \
            `E_ATTENUATION_LIMIT` when a segment's routed length in steps exceeds \
            `MAX_ATTENUATION_SEGMENT`, or when a stretch of dust past the limit has no coord a \
            repeater can stand on"
    )]
    Delay,
    /// Legalized Placement IR: crossing legalization, filling each cell's `buffer_coords`.
    #[value(
        help = "Legalized Placement IR: crossing legalization over the delayed Placement IR \
            against `--edition`. Stage 4 of the pipeline `spec/redstone` \"Place-and-route\" lays \
            out. Materialises the coord of every implicit buffer repeater the delay pass counted \
            into every cell's `buffer_coords`, one entry per driver segment that passes through it \
            — so a block serving several segments is named once per segment and a consumer \
            counting blocks deduplicates by coord"
    )]
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
    /// Multi-line human report mirroring the three-answers section of `spec/versioning-editions`.
    Text,
    /// Pretty JSON serialisation of `VersionAxes`, for tools.
    Json,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum LowerFormat {
    /// Per-structure ASCII Y-slice plus a palette listing (default; easiest way to eyeball
    /// whether the walls came out right).
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
