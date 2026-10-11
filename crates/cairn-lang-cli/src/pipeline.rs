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
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::floors::{DeclaredFloors, floor_refusing_target};
use crate::report::{FailureReport, report_failure_document};
use crate::target::{ResolvedTarget, resolve_target};

pub(crate) use requested::{RequestedEditions, requested_editions};

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
    /// The block-array pass's findings, empty when no target was given. A
    /// target that did not resolve still lowers, so `unsupported` says
    /// nothing about this field.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Scopes the source asked for that produced no voxels, as
    /// [`dropped_scopes`] collects them. Empty when no target was given.
    pub(crate) dropped_scopes: Vec<String>,
    /// How many scopes did lower, so the refusal can say "1 of 2".
    pub(crate) built_scopes: usize,
    /// Why a `--target` did not resolve, or `None` when it resolved or none
    /// was given.
    pub(crate) unsupported: Option<cairn_lang_formats::data_version::UnsupportedTarget>,
}

/// Lower far enough to check block ids, or not at all.
///
/// Only `cairn check --target` lowers in the check gate: the id check
/// needs one `(edition, version)` pair, and lowering is where the palette
/// is built. Every other lowering-stage finding comes with it rather than
/// being filtered back out.
///
/// A `--target` the edition's table cannot resolve is carried back for
/// `run_check` to print after the file's own findings, for the reason
/// [`resolve_target`] gives. Lowering still runs against the unpinned
/// view, as in [`load_and_lower`], so every finding that needs no version
/// is reported in the same run.
pub(crate) fn check_lowering(
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> CheckLowering {
    // clap's `requires` refuses `--target` without `--edition`
    // (`spec/compilation` "Target axes"), so the `else` is a bare `check`
    // or `check --edition E`, the ordinary run that lowers nothing.
    let (Some(edition), Some(target)) = (edition, target) else {
        return CheckLowering {
            diagnostics: Vec::new(),
            dropped_scopes: Vec::new(),
            built_scopes: 0,
            unsupported: None,
        };
    };
    let resolved = resolve_target(edition, target);
    // An unresolved target pins no version, as `cairn lower` runs, rather
    // than checking ids against a version nobody chose.
    let pinned = resolved.as_ref().ok().map(ResolvedTarget::mc_version);
    // Resolved for the id table's edition, as in `load_and_lower`: the
    // theme variant decides which id a `mat_slot=` reaches.
    let resolution = resolve(ir, Some(edition));
    let registry = builtin_pack(edition).view(pinned);
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
/// `connect` pair that laid none, in row order. `def::` keys are excluded:
/// a def lowers only through a `place`.
///
/// Built means a non-air voxel, not an entry in `block_ir.structures`: a
/// scope whose every member deferred, or a walkway whose every cell
/// overlapped a placement, keeps an all-air array, and writing it would
/// certify a build that placed nothing there.
///
/// A walkway's key is minted during lowering, so it is judged by its
/// `connect` row: built when `block_ir.walkways` holds the same site and
/// endpoints, in either order, with a block in its array. A
/// `W_DUPLICATE_WALKWAY` row whose pair an earlier row laid loses nothing,
/// and two rows naming one unlaid pair are one loss. The loss is named
/// `site::SITE::FROM ↔ TO` as the first such row wrote it, not by a
/// `walkway::` key: a row refused for its identifiers never got one, and
/// the key it would have spelled can be another row's.
///
/// Only rows in `resolution.connects` are judged. A row the resolver
/// dropped (`E_CONNECT_ARITY` and the other resolution errors) is in
/// neither this list nor the `M` of [`report_partial_build`]'s "N of M",
/// and carries an error of its own. One root cause can still raise `N` by
/// more than one: a placement refused upstream loses its own scope and,
/// through `W_DEFERRED_MEMBER`, every walkway with an endpoint on it.
///
/// One definition for every caller, so none can disagree about which
/// scopes count. `cairn info` reports the same list but exits 0 over it
/// (`spec/versioning-editions`).
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
/// Read through the palette rather than against index `0`, so a palette
/// that keeps air elsewhere cannot turn an all-air array into a built one.
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
/// One message for `run_check` and `run_compile`, so the count and the
/// per-scope notes cannot drift. `built` is [`built_scopes`]'s count.
/// `because` is the part that differs: a compile refuses because a
/// lockfile must not certify a partial build, and `cairn check --target`
/// because the compile at that pin would.
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
/// [`check()`] already merges the resolver's findings, span-sorted, so a
/// caller must not append `Resolution::diagnostics` as well. `lowering`
/// carries the block-array pass's diagnostics, which [`check()`] never
/// sees; a command that does not lower passes an empty vector.
///
/// `edition` pins the resolver, where `None` means no target is picked and
/// per-edition theme variants are softened together. `asked` is the set
/// of editions the command reports about, which
/// [`intended_target_findings`] weighs `@intended_targets` in:
/// `cairn info --editions bedrock` pins no resolver and must not be
/// refused by a finding only true of Java.
pub(crate) fn build_diagnostics(
    module: &cairn_lang_core::ast::Module,
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    asked: &[Edition],
    lowering: Vec<cairn_lang_core::check::Diagnostic>,
) -> Vec<cairn_lang_core::check::Diagnostic> {
    let mut combined = check(module, ir, edition);
    // Merged by span, since `check`'s half is sorted. The lowering half is
    // appended: it is a later stage, shown under its own heading.
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
/// of every edition in `asked` ([`weigh_intended_targets`] asks one at a
/// time), so a report scoped to one edition is not refused by the other's
/// answer.
///
/// A contradiction either edition reaches is reported: a floor refusing
/// every version the file says it is for is a mistake inside the file
/// however it is built, and reporting it only under a pin would leave a
/// bare `cairn check` silent on it.
///
/// `W_INTENDED_TARGET_UNSUPPORTED` waits until exactly one edition is
/// asked, because a version Java cannot build is routinely the Bedrock
/// target the author means.
///
/// One header is one line to fix, so one span carries one cap finding:
/// two editions reaching the same code collapse to the first, and
/// different cap codes collapse to the error.
fn intended_target_findings(
    module: &cairn_lang_core::ast::Module,
    asked: &[Edition],
) -> Vec<cairn_lang_core::check::Diagnostic> {
    // Before either pack is touched: packs are parsed and validated on
    // first use, and a file without the header should not pay for that.
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
                // The error wins: one edition finding part of the list
                // buildable does not make the other's "none of it is" less
                // true.
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
/// own so that [`requested_editions`] is the only way to build a
/// [`RequestedEditions`], which therefore never holds a repeat.
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
    /// Deduplicated, because `--editions java,java` asks about Java once: a
    /// repeat would give it second entries and notes, and would count as
    /// two editions in scope, which decides whether
    /// `W_INTENDED_TARGET_UNSUPPORTED` is asked for.
    ///
    /// # Errors
    ///
    /// An empty entry, or a name that is not an edition. The caller prints
    /// the message and exits 2 before reading the source.
    pub(crate) fn requested_editions(editions: &[String]) -> Result<RequestedEditions, String> {
        // The comma delimiter turns `--editions ""` and `java,,bedrock` into
        // empty entries.
        if editions.iter().any(|e| e.trim().is_empty()) {
            return Err("--editions value must not contain empty entries".to_owned());
        }
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
/// The pair is one defect at two reaches, so they are one code for "this
/// span already has an answer".
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
    /// The `@requires` floors this edition's build is held to, so `compile`
    /// can hold `--target` to them without reading the file again. `None`
    /// when the source declares none.
    pub(crate) version_floors: Option<DeclaredFloors>,
}

/// Read, parse, resolve and lower `file` for `edition`'s build.
///
/// `target` is the `--target` the run resolved, or `None` when it did not
/// resolve. Block ids are checked against that target's table unless the
/// source's floors refuse it ([`floor_refusing_target`]): `run_compile`
/// then refuses the build with `E_VERSION_CAP` or `E_REQUIRES_UNORDERABLE`,
/// and an `E_UNKNOWN_ID` for a block that version lacks, often the reason
/// for the floor, would send the author to replace a block instead. Such a
/// target lowers against no version, as `cairn lower` and `cairn info` do:
/// no id is checked, and every other finding is reported before the
/// refusal, an error among them stopping the build there. A malformed
/// `@requires` (`E_INVALID_REQUIRES`) declares no floor, so it never
/// suspends the check.
pub(crate) fn load_and_lower(
    file: &Path,
    edition: Edition,
    target: Option<&ResolvedTarget>,
) -> Result<Lowered, ExitCode> {
    let (source, module) = load_module(file, FailureReport::Text)?;
    let floors = declared_version_floors(&module, edition);
    let version_floors = (!floors.is_empty()).then(|| DeclaredFloors {
        floors,
        order: builtin_pack(edition).version_order(),
    });
    let mc_version = target
        .filter(|target| {
            version_floors.as_ref().is_none_or(|declared| {
                floor_refusing_target(&declared.floors, &declared.order, target).is_none()
            })
        })
        .map(ResolvedTarget::mc_version);
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    // `mc_version: None` leaves the id check off rather than running it
    // against a version nobody chose or one the floors refuse.
    let registry = builtin_pack(edition).view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // `cairn check`'s gate, so `compile` never writes a `verified: true`
    // lockfile for a source `check` rejects.
    block_ir.diagnostics = build_diagnostics(
        &module,
        &ir,
        Some(edition),
        &weighed_editions(Some(edition)),
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
