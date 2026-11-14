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
    about = "Compile .crn build descriptions to Minecraft structure files",
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
        long_about = "Run the validation passes and block-array lowering against a .crn source \
            file, writing nothing. Exits 0 when \
            nothing `Error`-severity is reported, 1 when any `Error`-severity diagnostic is \
            emitted (or the file fails to parse, or cannot be read for a reason other than \
            absence, or `--target` names no version the pinned edition ships, or a pinned run \
            loses a scope), 2 when the file cannot be located or `--target` is given without \
            `--edition`.\n\n\
            Without `--target` this command still runs block-array lowering, with no target, and \
            reports the lowering-stage findings every target that reads their line raises — a \
            roof bound outside the stair family, a `door side=` it cannot read — weighed as if \
            every block id some shipped version of the edition declares were declared. A finding \
            only a target's registry pack can give is left out: `E_UNKNOWN_ID` and \
            `E_UNKNOWN_ABSTRACT_TOKEN` among them, and any verdict on the block an abstract \
            token, or an id no shipped version declares, stands for; a note that would name such \
            a block says \"a block\" instead. `--edition E --target V` pins the one \
            `(edition, version)` those findings \
            are answers about and lowers against its id table, which is what lets a CI job gate \
            on `cairn check` and see every lowering-stage finding `cairn compile` would refuse \
            on, plus the `E_PARTIAL_BUILD` a lost scope earns. Nothing is written either way — \
            `compile` remains the command that produces artifacts and the lockfile, and the only \
            one that holds `--target` to the file's `@requires` floors (`E_VERSION_CAP`).\n\n\
            When nothing else is an error, each struct's `logic` is built into its circuit as \
            `cairn compile` builds it (`spec/redstone` \"Place-and-route\"), and the circuit's \
            findings follow the lowering's: the pinned edition's, at the `--target` version when \
            one is given, or without `--edition` the ones each edition raises at the same \
            place.\n\n\
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
                regardless of which edition it ends up compiling for. Without `--target`, \
                lowering runs with no target for the pinned edition, or for both when none is \
                pinned: a finding on a line only one edition's variant reads is that edition's, \
                and any other is reported when each edition raises it at the same place.\n\n\
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
                Setting it lowers against that version's id table, so the lowering stage's \
                findings that need a target join the report — `E_UNKNOWN_ID` above all, which \
                exists only where a target is pinned: `stone_bricks` is a block on Bedrock \
                1.21.40 and not on Bedrock 1.21.0, so with no version chosen there is no question \
                to answer. A version the edition does not ship \
                still lowers, against the same unpinned view `cairn lower` uses, so every finding \
                that needs no id table is reported before the target is refused. Leaving the flag \
                off still lowers, with no target, and reports only the findings every target that \
                reads their line raises."
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
            failure, on any `Error`-severity diagnostic (the check passes run here, as does each \
            requested edition's lowering with its circuits laid, and a range derived from a file \
            `cairn check` rejects is a confident wrong answer), or on any \
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
            same IR out as `.nbt` or `.mcstructure` files, per edition. Without `--edition` no \
            edition is pinned, so a member under an `@edition` guard and every struct's circuit \
            are left out, and a note says how many were. Lowering warnings \
            (deferred members, themeless scopes, abstract tokens) print to stderr but do not \
            affect the exit code. Exits 0 on success, 1 on a parse failure, on any \
            `Error`-severity diagnostic (`E_UNKNOWN_ABSTRACT_TOKEN` and the circuit's among them; \
            `E_UNKNOWN_ID` is not, since nothing here pins a target to check ids against), or on \
            an I/O error, and 2 when the file cannot be located"
    )]
    Lower {
        /// Path to the .crn file to lower.
        file: PathBuf,
        /// Edition to lower for; omitted, guarded members and circuits are left out.
        #[arg(
            long,
            value_parser = edition_parser(),
            long_help = "Optional edition pin. When set, the members the edition's `@edition` \
                guards hold are lowered, and each struct's `logic` is built into the circuit \
                `cairn compile --edition E` lays in the layers under it, the struct rising over \
                them (`spec/redstone` \"Place-and-route\"); the circuit's findings are reported \
                after the lowering's, and an error among them exits 1 as one of the lowering's \
                does. A source whose lowering holds an error lays no circuit, since a member that \
                failed to lower is not there to wire"
        )]
        edition: Option<Edition>,
        /// Output format for the lowered block-array IR.
        #[arg(long, value_enum, default_value_t = LowerFormat::Ascii)]
        format: LowerFormat,
    },
    /// Compile a .crn source file to its structure artifacts and write its lockfile.
    #[command(
        long_about = "Compile a .crn source file to its edition+version-pinned structure artifact \
            set and write a lockfile next to the source. Each struct's `logic` is built into the \
            circuit laid in the layers its `circuit` line reserves under it, the struct rising \
            over them (`spec/redstone` \"Place-and-route\"), and an error that build reports refuses \
            the compile as a lowering error does. `--edition java` writes gzip `.nbt` \
            structures; `--edition bedrock` writes uncompressed `.mcstructure` files, translating \
            each blockstate into the target version's Bedrock `states` through the registry \
            pack's state map; a state the map has no form for is a hard error \
            (`E_STATE_UNMAPPED`), and intent it can only approximate (stair `shape` before \
            1.26.60) is dropped with a `W_INTENT_DEGRADED` warning rather than silently. \
            `--target` pins the one version a block id and its states have an answer for, which \
            is what lets the compile check them (`E_UNKNOWN_ID`, `E_PARITY_UNSUPPORTED`, \
            `E_STATE_DOMAIN`, `E_STATE_UNMAPPED`); `cairn check --edition E --target V` pins the \
            same pair to reach those findings without building anything. \
            `--structure-format litematic` writes Java's artifacts as Litematica `.litematic` \
            schematics instead: a struct to a file of its own and a site to one file with a \
            region per placement and walkway (`spec/ecosystem-interop` \"Litematica\"); \
            `--structure-format schem` writes them as Sponge Schematic version 3 `.schem` files, \
            one per struct, placement and walkway, each offset by where it sits in its site \
            (`spec/ecosystem-interop` \"Sponge Schematic\"). Exits 0 on success, 1 on parse, \
            lowering, or I/O failure (including an unsupported `--target` or a state literal \
            outside its block's states), and 2 when the source file cannot be located or \
            `--structure-format` is given with `--edition bedrock`"
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
        /// Java structure file format; defaults to `nbt`.
        #[arg(
            long,
            value_enum,
            long_help = "The file format Java's structures are written in: `nbt` (the default), \
                one vanilla structure file per struct, placement and walkway; `litematic`, one \
                Litematica schematic per struct and per site; or `schem`, one Sponge Schematic \
                per struct, placement and walkway (`spec/ecosystem-interop` \"Forward \
                direction\"). Whichever is written, the artifacts share one naming rule, the \
                same collision guards and the same lockfile. Bedrock writes only `.mcstructure`, \
                so the flag with `--edition bedrock` is refused with exit 2 before the file is \
                read"
        )]
        structure_format: Option<StructureFormat>,
        /// Output directory for the structure files; defaults to the source file's directory.
        #[arg(
            long,
            long_help = "Output directory for the generated structure files (`.nbt`, \
                `.litematic` or `.schem` for Java, `.mcstructure` for Bedrock). Created if \
                missing. Defaults to the source file's parent directory"
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
    /// Write a structure file as a .crn of raw members, stamped with the target it compiles for.
    #[command(
        long_about = "Read a `.nbt`, `.litematic`, `.schem` or `.mcstructure` file and write it \
            as a `.crn` of `raw_block`, `raw_fill` and `raw_repeat` lines marked \
            `origin=imported`, which `cairn compile` builds back into the blocks the file holds \
            (`spec/ecosystem-interop` \"Reverse direction: the compiler transliterates, an LLM \
            lifts\"). The extension names the format, and the format the edition: `.mcstructure` \
            is Bedrock's, the other three Java's.\n\n\
            The source is stamped with one shipped target, which its `@requires` and \
            `@intended_targets` name: the one the file was saved at, the nearest after it when \
            the file was saved between two, the earliest when it was saved before every one, or \
            the latest when it names no version (`spec/ecosystem-interop` \"Import stamping and \
            pitfalls\"). A file saved past every \
            target this build ships is refused. The written source is compiled against the \
            stamp and compared cell by cell before anything is written, so a file whose blocks \
            the stamped target does not declare is refused with the compile's errors.\n\n\
            An import recovers blocks and their regularity, not intent: every run warns with \
            `W_SEMANTIC_LOSS`, once for that and once for each thing a raw member cannot carry, \
            such as a chest's contents, an entity or a structure void. A Litematica region is a \
            struct of its own, and a structure wider or deeper than 48 blocks is a `site` of \
            48 by 48 chunks.\n\n\
            Exits 0 when the source is written, warnings included; 1 when the file cannot be \
            read as its format, is saved past every shipped target, holds blocks the stamped \
            target refuses or that no raw member builds, or cannot be read or written for a \
            reason other than absence; and 2 when the file cannot be located, its extension names no format this command \
            reads, or `--out` names the file itself"
    )]
    Import {
        /// Path to the structure file to import.
        file: PathBuf,
        /// How far the import goes; `raw` is the one mode.
        #[arg(
            long,
            value_enum,
            default_value_t = ImportMode::Raw,
            long_help = "How far the import goes. `raw` is the compiler's ceiling: a faithful \
                transliteration that names nothing (`spec/ecosystem-interop` \"Three tiers of \
                transliteration\"). Naming what the raw members build, as walls, windows and \
                material slots, is a refactor of the written source rather than a mode of this \
                command"
        )]
        mode: ImportMode,
        /// Path to write the .crn to; defaults to `<stem>.raw.crn` beside the file.
        #[arg(
            long,
            long_help = "Path to write the `.crn` to, its directory created if missing. Defaults \
                to `<stem>.raw.crn` beside the file, so `house.litematic` → `house.raw.crn`. An \
                existing file there is replaced"
        )]
        out: Option<PathBuf>,
    },
    /// Experimental: print one stage of a .crn source file's redstone pipeline as JSON.
    #[command(
        long_about = "Lower a .crn source file's `logic` bindings, sensors, and actuators through \
            the redstone pipeline and print an intermediate stage as JSON. `--stage logic` \
            (default) prints the edition-neutral Logic IR DAG; `--stage netlist` prints the \
            Netlist IR of Logical Cells + nets derived from that DAG; `--stage edition` picks the \
            target-edition realisation of each cell and prints the Edition Netlist IR; `--stage \
            placement` lays each cell's tile, each sensor's pad and each actuator's riser in the \
            layers each scope's `circuit` line reserves under the built struct; `--stage route` \
            runs each net's dust from the tile that drives it to every input it feeds, apart from \
            every other net's; `--stage delay` stands a buffer repeater in a net's dust wherever \
            it would run too weak for an input it feeds; `--stage legalize` turns every part into \
            the block it is built as, in the state the circuit rests in with its sensors off, \
            then checks each `assert latency`, `assert truth` and `assert always` against it and \
            prints no IR when one fails. Each of \
            the four layout stages carries a `\"stage\"` key echoing the flag value that produced \
            it (`placement` / `route` / `delay` / `legalize`), so a consumer reads the stage off \
            the output instead of inferring it from which keys are present. The `--edition <java|bedrock>` flag is \
            required in the `edition`, `placement`, `route`, `delay`, and `legalize` modes and \
            refused otherwise (the earlier stages are edition-neutral by contract). `cairn \
            compile` runs the same pipeline and lays the legalized circuit under the struct; this \
            command prints what it builds on the way. **Internal / experimental** — the shape of \
            the output is not covered by the stable compatibility tier and may change at any \
            time. Requires `--experimental-logic-synth` so a caller cannot end up depending on it \
            accidentally.\n\n\
            Exits 0 when the requested stage produced a well-formed IR (warnings still allowed); 1 \
            on a parse failure, on any `Error`-severity diagnostic (from `check`, from lowering \
            the struct for the edition, or from a redstone pass the stage runs), on a failure to \
            serialise the IR as JSON, or on any other I/O error (permission denied, non-UTF-8 \
            contents); 2 when the file cannot be located, and refuses a run without \
            `--experimental-logic-synth`, a missing `--edition` on the stages that require it and \
            a stray one on the edition-neutral stages with exit 2, before the file is read."
    )]
    Synth {
        /// Path to the .crn file to synthesise.
        file: PathBuf,
        /// Confirm that this subcommand's output is internal and may change at any time.
        #[arg(
            long,
            long_help = "Opt-in flag confirming the caller understands this surface is internal. \
                Required because the subcommand prints the compiler's internal stages, which may \
                change in any release; without it the subcommand exits 2 with a hint"
        )]
        experimental_logic_synth: bool,
        /// Which pipeline stage to print.
        #[arg(
            long,
            value_enum,
            default_value_t = SynthStage::Logic,
            long_help = "Which pipeline stage to print. For the four layout stages the value chosen \
                here is echoed back as the layout's `\"stage\"` key, so the flag and the dump \
                share one vocabulary",
        )]
        stage: SynthStage,
        /// Target edition, for the stages after `netlist`.
        #[arg(
            long,
            value_parser = edition_parser(),
            long_help = "Target edition for the Edition Netlist IR and the placed, routed, \
                delayed and legalized layouts. Required when `--stage edition`, `--stage \
                placement`, `--stage route`, `--stage delay`, or `--stage legalize` is set; \
                refused for `logic` and `netlist`, which are edition-neutral by contract"
        )]
        edition: Option<Edition>,
    },
    /// Print a page of the documentation this binary carries, the specification among them.
    #[command(
        long_about = "Print a page of the Cairn documentation site as Markdown, from the copy \
            embedded in this binary, so the page describes the compiler printing it and needs no \
            network. Without PAGE, list every page with its title.\n\n\
            PAGE is the path the site serves the page at, such as `spec/syntax` for \
            https://cairn.kage1020.com/spec/syntax/, or the path of its file, such as `spec/index` \
            or `spec/lint.md`; each Japanese page sits under `ja/`. A link to any page \
            `cairn docs` lists can be passed as written, its slashes, `index.md` and `#fragment` \
            included, as can a URL of the site: `cairn docs /spec/#two-version-axes` prints the \
            section that fragment opens.\n\n\
            TITLE names a part of the page the way `--help` cites the spec: a heading without its \
            number, a term a list item on the page defines in bold (`- **Term.**`), or the \
            page's own title. The citation `spec/lint` \"Error vs warning\" reads as \
            `cairn docs spec/lint \"Error vs warning\"`. A heading or the page's title is also \
            taken with its number, and a title more than one part has is refused with each of \
            them listed.\n\n\
            Exits 0 when the list, the page or its part is printed, and 2 when PAGE or TITLE \
            names nothing this binary carries, TITLE names more than one part, or a `#fragment` \
            comes with TITLE or `--toc`."
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
            Edition::Java => "Java Edition",
            Edition::Bedrock => "Bedrock Edition",
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
    /// Placement IR: every tile laid in the reservation.
    #[value(
        help = "Placement IR: each cell's tile, each sensor's pad and each actuator's riser laid \
            against `--edition` in the layers the scope's `circuit` line reserves under the \
            built struct (`spec/redstone` \"Place-and-route\"). Refuses a reservation the tiles \
            do not fit with `E_ROUTE_CONGESTION`, a macro with more stages than its tile carries \
            with `E_ATTENUATION_LIMIT`, and a sensor or actuator the layers under the floor cannot \
            reach with `E_ROUTE_UNREACHABLE_COMPONENT`"
    )]
    Placement,
    /// Routed layout: every net's dust.
    #[value(
        help = "Routed layout: each net's dust from the tile that drives it to every input it \
            feeds, apart from every other net's, against `--edition`. Refuses a net with \
            nowhere left to run with `E_ROUTE_CONGESTION` on its driver"
    )]
    Route,
    /// Delayed layout: the buffer repeaters each net's dust needs.
    #[value(
        help = "Delayed layout: a buffer repeater in a net's dust wherever it would run too weak \
            for an input it feeds, against `--edition`, each in a straight stretch of that net's \
            own dust. Refuses a stretch with nowhere a repeater stands with \
            `E_ATTENUATION_LIMIT`"
    )]
    Delay,
    /// Legalized layout: every block of the reservation, then every assertion checked against it.
    #[value(
        help = "Legalized layout: every part of the reservation as the block it is built as, each \
            torch, repeater, comparator and dust in the state the circuit rests in with its \
            sensors off, against `--edition`; this is what `cairn compile` lays under the \
            struct. Then checks each assertion against the circuit, refusing with \
            `E_SIM_ASSERTION_FAILED` an `assert latency` whose slowest path takes more ticks than \
            its bound, then an `assert truth` or `assert always` a run of the circuit breaks"
    )]
    Legalize,
}

/// The file format `cairn compile` writes Java's structures in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum StructureFormat {
    /// Vanilla structure files (`.nbt`), one per struct, placement and walkway (default).
    #[default]
    Nbt,
    /// Litematica schematics (`.litematic`), one per struct and per site.
    Litematic,
    /// Sponge Schematic version 3 (`.schem`), one per struct, placement and walkway.
    Schem,
}

/// How far `cairn import` takes a structure file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ImportMode {
    /// Raw members that build the file's blocks, with no name given to anything (default).
    Raw,
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
