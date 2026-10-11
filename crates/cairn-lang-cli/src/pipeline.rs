//! The front half the subcommands share: reading and parsing a source,
//! the check passes, lowering it for a build, and the scopes a lowering
//! lost.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::ast::Header;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::check::{LineStarts, weigh_intended_targets};
use cairn_lang_core::resolve::{declared_version_floors, resolve};
use cairn_lang_core::{
    Diagnostic, DiagnosticCode, Edition, check, diagnose_parse_failure, lower, parse,
};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

use crate::cli::EditionArg;
use crate::floors::{DeclaredFloors, floor_refusing_target};
use crate::report::{FailureReport, report_failure_document};
use crate::target::{ResolvedTarget, resolve_target};

pub(crate) use requested::{RequestedEditions, requested_editions};

/// The built-in registry pack an edition compiles against.
pub(crate) fn builtin_pack(edition: Edition) -> &'static RegistryPack {
    match edition {
        Edition::Java => builtin_java(),
        Edition::Bedrock => builtin_bedrock(),
    }
}

/// Read the source a command was pointed at, or report why it could not
/// be and hand back the exit code: 2 for a path that names nothing (a
/// usage mistake), 1 for anything else (permissions, encoding, I/O).
pub(crate) fn read_source(file: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(file).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })
}

/// Read and parse a source, reporting a parse failure the way the
/// command's format asks for.
///
/// A command with no `--format` passes [`FailureReport::Text`], which is
/// the prose report every subcommand has always given.
pub(crate) fn load_module(
    file: &Path,
    report: FailureReport,
) -> Result<(String, cairn_lang_core::ast::Module), ExitCode> {
    let source = read_source(file)?;
    let module = parse(&source).map_err(|err| {
        let lines = LineStarts::new(&source);
        let parse_failure = [diagnose_parse_failure(&source, &lines, &err)];
        report_failure_document(file, &source, report, &parse_failure)
    })?;
    Ok((source, module))
}

/// What [`check_lowering`] found, and what the run owes a refusal for.
pub(crate) struct CheckLowering {
    /// The block-array pass's findings. Empty when no target was pinned
    /// and nothing lowered, or when lowering ran and reported nothing; a
    /// target that did not resolve still lowers, so `unsupported` being
    /// set says nothing about this field.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Scopes the source asked for that produced no voxels, as
    /// [`dropped_scopes`] collects them. Empty on a run that pinned no
    /// target, which lowers nothing and so loses nothing.
    pub(crate) dropped_scopes: Vec<String>,
    /// How many scopes did lower, so the refusal can say "1 of 2".
    pub(crate) built_scopes: usize,
    /// The failure report for a `--target` that did not resolve — the
    /// edition, the version asked for, the nearest suggestion and the
    /// supported list — or `None` when the target resolved or none was
    /// given.
    pub(crate) unsupported: Option<cairn_lang_formats::data_version::UnsupportedTarget>,
}

/// Lower far enough to check block ids, or not at all.
///
/// `cairn check --target` is the only run of the check gate that lowers.
/// The id check needs the one `(edition, version)` pair an id either
/// exists in or does not, and lowering is where the palette is built, so
/// the two arrive together: pinning a target turns the pass on, and every
/// other lowering-stage finding comes with it rather than being filtered
/// back out — a report that saw `E_INCOMPATIBLE_MATERIAL` and said
/// nothing would be the same silence this flag exists to end.
///
/// A `--target` the edition's table cannot resolve is carried back for
/// `run_check` to print after the file's own findings, for the reason
/// [`resolve_target`] gives. Lowering still runs, exactly as it does in
/// [`load_and_lower`], against the unpinned view: only the id check needs
/// the version, so refusing to lower at all would hide every finding that
/// does not — and send the author back for a second run once the flag is
/// spelled right.
pub(crate) fn check_lowering(
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<EditionArg>,
    target: Option<&str>,
) -> CheckLowering {
    // `--target` without `--edition` cannot reach here — clap's `requires`
    // refuses it (`spec/compilation` "Target axes"), which is what
    // `target_without_edition_is_refused_as_a_usage_error` guards. Every
    // other combination can: a bare `check` and `check --edition E` both
    // take the `let else` and lower nothing, which is the ordinary
    // no-target run rather than an edge case.
    let (Some(edition), Some(target)) = (edition, target) else {
        return CheckLowering {
            diagnostics: Vec::new(),
            dropped_scopes: Vec::new(),
            built_scopes: 0,
            unsupported: None,
        };
    };
    let resolved = resolve_target(edition, target);
    // `None` where the target did not resolve, which is the same "no
    // version pinned" mode `cairn lower` runs in: the id check is left off
    // rather than run against a version nobody chose.
    let pinned = resolved.as_ref().ok().map(ResolvedTarget::mc_version);
    // Pinned to the same edition the id table belongs to, matching
    // `load_and_lower`: a theme's per-edition variant decides which id a
    // `mat_slot=` reaches, so resolving the file edition-neutrally and
    // then checking the ids against one edition's table would ask the
    // question of a palette the build never has.
    let resolution = resolve(ir, Some(edition.as_edition()));
    let registry = edition.registry_pack().view(pinned);
    let block_ir = lower_to_block_array(ir, &resolution, Some(&registry));
    CheckLowering {
        dropped_scopes: dropped_scopes(&resolution, &block_ir),
        built_scopes: built_scopes(&block_ir),
        diagnostics: block_ir.diagnostics,
        unsupported: resolved.err(),
    }
}

/// Scopes the source asked for that the block-array pass did not turn into
/// a structure holding at least one block: first the resolver's
/// (`struct::NAME`, `site::SITE::PLACE`) in resolver order, then one per
/// `connect` pair that laid none, in row order.
///
/// `def::` keys are excluded: a def is a template and lowers to voxels only
/// through a `place` that instantiates it.
///
/// Built means at least one non-air voxel, not an entry in
/// `block_ir.structures`: lowering keeps an array for a scope whose every
/// member deferred, and for a walkway whose every cell overlapped a
/// placement, and both are all air. Writing that array out would certify
/// a build that placed nothing the source asked for there, so either is a
/// loss like a scope with no array at all.
///
/// A walkway is not in `resolution.scopes`, because its key is minted
/// during lowering, so it is judged by its `connect` row instead: a row
/// asks for the walkway between its two `(place, port)` endpoints, and
/// that walkway is built when `block_ir.walkways` holds the same site and
/// the same two endpoints, in either order, and its array holds a block.
/// So a `W_DUPLICATE_WALKWAY` row, whose pair the earlier row laid, loses
/// nothing, and two rows naming one pair that neither laid are one loss.
/// The loss is named `site::SITE::FROM ↔ TO`, as the first row asking for
/// the pair wrote its endpoints, rather than by the `walkway::` key it
/// would have had: a row refused for its identifiers never got a key, and
/// the key it would have spelled can be another row's.
///
/// Only the rows in `resolution.connects` are judged. A row the resolver
/// dropped (`E_CONNECT_ARITY` and the other resolution errors) is not
/// there, so it is neither in this list nor in the `M` of
/// [`report_partial_build`]'s "N of M". Today each such drop comes with an
/// error of its own, on the row or, under `W_DEFERRED_CONNECT`, on the
/// `place` it names — or, where that `place` carries `W_DEFERRED_PLACE`, on
/// the row refused for its id that the warning's note points at. One root
/// cause can still raise `N` by more than one:
/// a placement refused upstream loses its own scope and, through
/// `W_DEFERRED_MEMBER`, every walkway with an endpoint on it.
///
/// One definition for the three callers, so no copy can drift into
/// disagreeing about which scopes count. `check_lowering` and
/// `load_and_lower` collect the list for the two refusal points, `run_check`
/// (for the compile at its pinned target) and `run_compile`, which both
/// print it through [`report_partial_build`] and exit 1. `edition_rows`
/// reports the same list as "no target can build this", and `cairn info`
/// still exits 0 over it, deliberately (`spec/versioning-editions`).
pub(crate) fn dropped_scopes(
    resolution: &cairn_lang_core::Resolution,
    block_ir: &BlockArrayIr,
) -> Vec<String> {
    let built = |key: &str| block_ir.structures.get(key).is_some_and(holds_a_block);
    let mut dropped: Vec<String> = resolution
        .scopes
        .keys()
        .filter(|key| !key.starts_with("def::"))
        .filter(|key| !built(key))
        .cloned()
        .collect();
    let pair = |site: &str, from: String, to: String| {
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        (site.to_owned(), a, b)
    };
    let mut accounted: HashSet<(String, String, String)> = block_ir
        .walkways
        .iter()
        .filter(|(key, _)| built(key.as_str()))
        .map(|(_, w)| pair(w.site.as_str(), w.from.to_string(), w.to.to_string()))
        .collect();
    for connect in &resolution.connects {
        let (from, to) = (connect.from.to_string(), connect.to.to_string());
        let key = format!("site::{}::{from} ↔ {to}", connect.site);
        if accounted.insert(pair(connect.site.as_str(), from, to)) {
            dropped.push(key);
        }
    }
    dropped
}

/// How many structures hold at least one block — the scopes
/// [`dropped_scopes`] counts as built, so the two add up to the `M` of
/// [`report_partial_build`]'s "N of M".
pub(crate) fn built_scopes(block_ir: &BlockArrayIr) -> usize {
    block_ir
        .structures
        .values()
        .filter(|array| holds_a_block(array))
        .count()
}

/// Whether any voxel of `array` is something other than air.
///
/// Read through the palette rather than against index `0`: the palette
/// keeps air at `0`, but the question is what the voxel is, and a palette
/// assembled another way must not turn an all-air array into a built one.
fn holds_a_block(array: &BlockArray) -> bool {
    array.voxels.iter().any(|index| {
        array
            .palette
            .entries
            .get(usize::from(index.0))
            .is_some_and(|state| state.id != BlockState::AIR_ID)
    })
}

/// Report the scopes a lowering lost, as `E_PARTIAL_BUILD`.
///
/// One message for the two refusal points, `run_check` and `run_compile`,
/// because the count and the per-scope notes are the part an author reads
/// and two copies of them drift. `built` is [`built_scopes`]'s count, so
/// the "N of M" adds the losses to the scopes that hold a block. `because` is the half that cannot be shared: a
/// compile refuses because a lockfile must not certify a build missing
/// part of what the source asked for, and `cairn check --target` certifies
/// nothing — it refuses because the compile at that pin would, which is
/// the whole promise of the flag.
pub(crate) fn report_partial_build(file: &Path, dropped: &[String], built: usize, because: &str) {
    eprintln!(
        "error[E_PARTIAL_BUILD]: {}: {} of {} requested scopes did not lower; {because}",
        file.display(),
        dropped.len(),
        dropped.len() + built,
    );
    for scope in dropped {
        eprintln!("  note: `{scope}` produced no voxels");
    }
}

/// Every version a pack can build for, in ascending release order.
pub(crate) fn supported_versions(pack: &RegistryPack) -> Vec<String> {
    pack.targetable()
        .into_iter()
        .map(|entry| entry.mc_version.clone())
        .collect()
}

/// Every diagnostic a command must report, in one stream, in the order the
/// passes ran.
///
/// [`check()`] runs the syntactic passes **and** merges the resolver's
/// findings, so a caller must not append `Resolution::diagnostics` on
/// top of it. Doing so printed every
/// resolver finding twice, and — because [`check()`] returns span-sorted
/// output while the resolver emits in discovery order — the second copy
/// walked the file backwards.
///
/// `lowering` carries the block-array pass's own diagnostics, which
/// [`check()`] never sees; pass an empty vector from a command that does not
/// lower.
///
/// `edition` and `asked` are two different questions and are answered
/// separately. `edition` pins the *resolver*, where `None` means "no
/// target picked yet" and softens per-edition theme variants together.
/// `asked` is the set of editions the command is reporting about, which
/// is what [`intended_target_findings`] weighs `@intended_targets` in: a
/// `cairn info --editions bedrock` pins no resolver and still must not be
/// refused by a finding that is only true of Java.
pub(crate) fn build_diagnostics(
    module: &cairn_lang_core::ast::Module,
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    asked: &[Edition],
    lowering: Vec<cairn_lang_core::check::Diagnostic>,
) -> Vec<cairn_lang_core::check::Diagnostic> {
    let mut combined = check(module, ir, edition);
    // Merged by span rather than appended: `check` returns its half
    // sorted, and a header finding printed after a finding forty lines
    // below it reads as a second file. The lowering half is still
    // appended, as it always has been — it is a different stage, and the
    // commands that show it show it under its own heading.
    for finding in intended_target_findings(module, asked) {
        let at = combined.partition_point(|kept| {
            (kept.span.start, kept.span.end) <= (finding.span.start, finding.span.end)
        });
        combined.insert(at, finding);
    }
    combined.extend(lowering);
    combined
}

/// Weigh `@intended_targets` against the file's own floors, in the table
/// of every edition the command is reporting about.
///
/// The pass itself asks one edition at a time
/// ([`weigh_intended_targets`]), and the commands that call this are
/// about different sets: `cairn compile` names one, `cairn check` may,
/// `cairn info --editions` names the ones its rows cover, and the rest
/// are about both. `asked` is that set, so a report scoped to one edition
/// is not refused by the other's answer.
///
/// A contradiction either edition reaches is reported, rather than only
/// one both agree on: it is *inside the file* — a floor refusing every
/// version the same file says it is for is a mistake in one of the two
/// lines however the author later builds it — and reporting it only under
/// a pin would leave the bare `cairn check`, the run an author makes
/// most, silent on it.
///
/// `W_INTENDED_TARGET_UNSUPPORTED` waits until exactly one edition is
/// asked. "No `--target` names this version" is a question about one
/// edition's pack, and a version Java cannot build is routinely the
/// Bedrock target the author means: with two editions in scope the answer
/// is not that the header is wrong but that nobody has said which one is
/// being asked.
///
/// One header is one line to fix, so one span carries one cap finding.
/// Two editions reaching the same code collapse to the first, and two
/// reaching *different* cap codes collapse to the error — otherwise a
/// header earned a warning and an error at one position, one of them
/// saying the other is too weak.
fn intended_target_findings(
    module: &cairn_lang_core::ast::Module,
    asked: &[Edition],
) -> Vec<cairn_lang_core::check::Diagnostic> {
    // Before either pack is touched: the packs are embedded JSON that is
    // parsed and validated on first use, and `cairn check` is the command
    // run most. A file with no such header asks none of the questions
    // below, and should not pay for both tables to find that out.
    if !module
        .headers
        .iter()
        .any(|header| matches!(header, Header::IntendedTargets { .. }))
    {
        return Vec::new();
    }
    let mut findings: Vec<cairn_lang_core::check::Diagnostic> = Vec::new();
    for edition in asked {
        let pack = builtin_pack(*edition);
        let targetable = supported_versions(pack);
        for finding in weigh_intended_targets(module, *edition, &pack.version_order(), &targetable)
        {
            if asked.len() > 1 && finding.code == DiagnosticCode::IntendedTargetUnsupported {
                continue;
            }
            let clash = findings.iter().position(|kept| {
                kept.span == finding.span
                    && (kept.code == finding.code
                        || (is_intended_target_cap(kept.code)
                            && is_intended_target_cap(finding.code)))
            });
            match clash {
                // The error, whichever edition reached it: one edition
                // finding part of the list still buildable does not make
                // the other's "none of it is" less true.
                Some(at) if findings[at].code == DiagnosticCode::IntendedTargetCapPartial => {
                    if finding.code == DiagnosticCode::IntendedTargetCap {
                        findings[at] = finding;
                    }
                }
                Some(_) => {}
                None => findings.push(finding),
            }
        }
    }
    findings
}

/// The editions `cairn info --editions` reports on, in a module of their
/// own for the reason [`staging`](crate::staging) is one: [`RequestedEditions`]' field is
/// private to it, so [`requested_editions`] is the only way to build one,
/// and a function taking one is handed a list with no repeat in it.
mod requested {
    use cairn_lang_core::Edition;

    /// The distinct editions an `--editions` list names, in the order it
    /// first names them.
    pub(crate) struct RequestedEditions(Vec<Edition>);

    impl RequestedEditions {
        pub(crate) fn as_slice(&self) -> &[Edition] {
            &self.0
        }
    }

    /// Read an `--editions` list as the editions it asks about, or the
    /// message refusing it.
    ///
    /// Deduplicated because the list is a user's, and `--editions java,java`
    /// asks about Java once. A repeat would walk the edition twice, giving
    /// it a second entry in `edition portability` and `buildable targets`
    /// and printing its notes a second time, and would make
    /// `--editions java,java` read as two editions in scope, which is what
    /// decides whether `W_INTENDED_TARGET_UNSUPPORTED` has been asked for
    /// at all.
    ///
    /// # Errors
    ///
    /// An empty entry, or a name that is not an edition. The caller prints
    /// the message and exits 2 before reading the source.
    pub(crate) fn requested_editions(editions: &[String]) -> Result<RequestedEditions, String> {
        // Reject empty edition entries early so they cannot leak into the
        // output. `--editions ""` and `--editions java,,bedrock` both produce
        // empty strings under the comma value-delimiter, which would render
        // as `: 1.20 .. latest` rows or `"edition":""` JSON.
        if editions.iter().any(|e| e.trim().is_empty()) {
            return Err("--editions value must not contain empty entries".to_owned());
        }
        // Reject unknown edition names before the (expensive) dry-run lowering.
        // The parity table's contract is "every entry is a real portability
        // figure"; letting an unknown edition through would either silently
        // produce zeros or a Java-flavoured fallback the caller couldn't
        // distinguish from a real portable-only classification.
        let mut asked: Vec<Edition> = Vec::with_capacity(editions.len());
        for name in editions {
            let edition: Edition = name.parse().map_err(|err| format!("{err}"))?;
            if !asked.contains(&edition) {
                asked.push(edition);
            }
        }
        Ok(RequestedEditions(asked))
    }
}

/// The editions a command with a single optional pin is about.
///
/// A pin names one; without one the command is about both, because a
/// source that has not chosen an edition may still be built for either.
pub(crate) fn weighed_editions(pin: Option<Edition>) -> Vec<Edition> {
    pin.map_or_else(|| vec![Edition::Java, Edition::Bedrock], |one| vec![one])
}

/// Whether a code is one of the two `@intended_targets` cap findings.
///
/// The pair is one defect reported at two reaches, so they are one code
/// for the purpose of "this span already has an answer". Written as a
/// predicate rather than inlined because a third reach would have to be
/// added here and nowhere else.
fn is_intended_target_cap(code: DiagnosticCode) -> bool {
    matches!(
        code,
        DiagnosticCode::IntendedTargetCap | DiagnosticCode::IntendedTargetCapPartial
    )
}

/// A `.crn` read, parsed, resolved, and lowered, plus what the lowering
/// failed to produce.
pub(crate) struct Lowered {
    pub(crate) source: String,
    pub(crate) block_ir: BlockArrayIr,
    /// Scopes the source asked for that produced no voxels, as
    /// [`dropped_scopes`] collects them.
    pub(crate) dropped_scopes: Vec<String>,
    /// The `@requires` floors this edition's build is held to, carried out
    /// of the parse so `compile` can hold `--target` to them without
    /// reading the file a second time. `None` when the source declares
    /// none, which is the ordinary case.
    pub(crate) version_floors: Option<DeclaredFloors>,
}

/// Read, parse, resolve and lower `file` for `edition`'s build.
///
/// `target` is the `--target` the run resolved, or `None` when it did not
/// resolve. Lowering checks block ids against that target's table unless
/// the floors the source declares refuse it ([`floor_refusing_target`]).
/// `run_compile` then refuses the build on the version axis, with
/// `E_VERSION_CAP` or `E_REQUIRES_UNORDERABLE`, and an `E_UNKNOWN_ID` for a
/// block that version lacks — usually the very reason the floor was
/// declared — would send the author to replace a block when the repair is
/// `--target` or the floor's own line. Such a target is lowered against no
/// version, as `cairn lower` and `cairn info` lower: no id is checked at
/// all, a typo no version declares included, and every lowering-stage
/// finding that does not need the version, such as
/// `E_UNKNOWN_ABSTRACT_TOKEN`, still comes out. Those and the check pass's
/// findings are reported before the refusal is reached, and an error among
/// them stops the build there. A malformed `@requires`
/// (`E_INVALID_REQUIRES`) declares no floor, so it never suspends the
/// check.
pub(crate) fn load_and_lower(
    file: &Path,
    edition: EditionArg,
    target: Option<&ResolvedTarget>,
) -> Result<Lowered, ExitCode> {
    let (source, module) = load_module(file, FailureReport::Text)?;
    let floors = declared_version_floors(&module, edition.as_edition());
    let version_floors = (!floors.is_empty()).then(|| DeclaredFloors {
        floors,
        order: edition.registry_pack().version_order(),
    });
    let mc_version = target
        .filter(|target| {
            version_floors.as_ref().is_none_or(|declared| {
                floor_refusing_target(&declared.floors, &declared.order, target).is_none()
            })
        })
        .map(ResolvedTarget::mc_version);
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition.as_edition()));
    // The pack is edition-specific: an abstract `@token` resolves through
    // the pack whose backend will serialise it, and the id table it is
    // checked against belongs to the one version that pack was pinned to.
    // `mc_version: None` leaves the id check off rather than running it
    // against a version nobody chose (no `--target` resolved) or one the
    // source disowns (the floors refuse it).
    let registry = edition.registry_pack().view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // The check pass is the gate `cairn check` exposes; running it here is
    // what keeps `compile` from accepting a source that `check` rejects.
    // Skipped, an `E_DUPLICATE_ID` would compile to artifacts plus a
    // `verified: true` lockfile at exit 0. Resolver findings such as
    // a `place use=cottag` typo (`E_UNRESOLVED_PLACE_REF`) reach the stream
    // through the same call — `check` merges them.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        Some(edition.as_edition()),
        &weighed_editions(Some(edition.as_edition())),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    Ok(Lowered {
        source,
        block_ir,
        dropped_scopes,
        version_floors,
    })
}

/// `struct probe size=3x3` with walls, lowered for `edition` against that
/// edition's pack with no version pinned: one structure, for the unit tests
/// that need a real lowering to rekey or corrupt.
#[cfg(test)]
pub(crate) fn lowered_probe(edition: Edition) -> BlockArrayIr {
    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(&ir, &resolution, Some(&builtin_pack(edition).view(None)))
}
