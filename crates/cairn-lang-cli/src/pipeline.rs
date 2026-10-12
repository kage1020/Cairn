//! The front half the subcommands share: reading, parsing, checking and lowering a source.

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
use cairn_lang_formats::registry::builtin_pack;

use crate::floors::{DeclaredFloors, floor_refusing_target};
use crate::report::{FailureReport, report_failure_document};
use crate::target::{ResolvedTarget, resolve_target, supported_versions};

pub(crate) use requested::{RequestedEditions, requested_editions};

/// Read a source, or report why not: exit 2 for a path that names nothing, 1 for anything else.
pub(crate) fn read_source(file: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(file).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })
}

/// Read and parse a source, reporting a parse failure as `report` asks.
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
    /// The block-array pass's findings, empty when no target was given.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Scopes that produced no voxels, empty when no target was given.
    pub(crate) dropped_scopes: Vec<String>,
    /// How many scopes did lower, so the refusal can say "1 of 2".
    pub(crate) built_scopes: usize,
    /// Why a `--target` did not resolve.
    pub(crate) unsupported: Option<cairn_lang_formats::data_version::UnsupportedTarget>,
}

/// Lower for `cairn check --target`'s id check, or not at all without a target; an unresolved
/// target lowers unpinned, so every finding that needs no version is still reported.
pub(crate) fn check_lowering(
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> CheckLowering {
    // clap refuses `--target` without `--edition` (`spec/compilation` "Target axes").
    let (Some(edition), Some(target)) = (edition, target) else {
        return CheckLowering {
            diagnostics: Vec::new(),
            dropped_scopes: Vec::new(),
            built_scopes: 0,
            unsupported: None,
        };
    };
    let resolved = resolve_target(edition, target);
    let pinned = resolved.as_ref().ok().map(ResolvedTarget::mc_version);
    // The theme variant decides which id a `mat_slot=` reaches, so resolve for the edition.
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

/// Scopes the source asked for that hold no block after lowering, all-air arrays included: the
/// resolver's scopes, then one `site::SITE::FROM ↔ TO` per `connect` pair that laid none.
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

/// How many structures hold at least one block.
pub(crate) fn built_scopes(block_ir: &BlockArrayIr) -> usize {
    block_ir
        .structures
        .values()
        .filter(|array| holds_a_block(array))
        .count()
}

/// Whether any voxel of `array` is something other than air, wherever its palette puts air.
fn holds_a_block(array: &BlockArray) -> bool {
    array.voxels.iter().any(|index| {
        array
            .palette
            .entries
            .get(usize::from(index.0))
            .is_some_and(|state| state.id != BlockState::AIR_ID)
    })
}

/// Report the scopes a lowering lost, as `E_PARTIAL_BUILD`, ending the line with `because`.
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

/// Every diagnostic a command must report, in the order the passes ran: [`check()`]'s, which
/// include the resolver's, then `lowering`'s.
///
/// `asked` is the editions `@intended_targets` is weighed in.
pub(crate) fn build_diagnostics(
    module: &cairn_lang_core::ast::Module,
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    asked: &[Edition],
    lowering: Vec<cairn_lang_core::check::Diagnostic>,
) -> Vec<cairn_lang_core::check::Diagnostic> {
    let mut combined = check(module, ir, edition);
    // Merged by span into `check`'s sorted half; the lowering stage's findings follow.
    for finding in intended_target_findings(module, asked) {
        let at = combined.partition_point(|kept| {
            (kept.span.start, kept.span.end) <= (finding.span.start, finding.span.end)
        });
        combined.insert(at, finding);
    }
    combined.extend(lowering);
    combined
}

/// `@intended_targets` weighed in the table of every edition in `asked`, one cap finding a span.
///
/// `W_INTENDED_TARGET_UNSUPPORTED` waits for a single edition: a version Java cannot build is
/// routinely the Bedrock target the author means.
fn intended_target_findings(
    module: &cairn_lang_core::ast::Module,
    asked: &[Edition],
) -> Vec<cairn_lang_core::check::Diagnostic> {
    // Packs parse on first use, which a file without the header should not pay for.
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
                // The error wins: one edition's partial answer leaves the other's "none" true.
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

/// Its own module, so [`requested_editions`] is the only way to build a [`RequestedEditions`].
mod requested {
    use cairn_lang_core::Edition;

    /// The distinct editions an `--editions` list names, in the order it first names them.
    pub(crate) struct RequestedEditions(Vec<Edition>);

    impl RequestedEditions {
        pub(crate) fn as_slice(&self) -> &[Edition] {
            &self.0
        }
    }

    /// Read an `--editions` list as the distinct editions it asks about.
    ///
    /// # Errors
    ///
    /// The message refusing an empty entry or a name that is not an edition.
    pub(crate) fn requested_editions(editions: &[String]) -> Result<RequestedEditions, String> {
        // clap's comma delimiter turns `java,,bedrock` into an empty entry.
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

/// The editions a command with one optional pin is about: the pin, or both without one.
pub(crate) fn weighed_editions(pin: Option<Edition>) -> Vec<Edition> {
    pin.map_or_else(|| vec![Edition::Java, Edition::Bedrock], |one| vec![one])
}

/// Whether `code` is one of the two `@intended_targets` cap findings.
fn is_intended_target_cap(code: DiagnosticCode) -> bool {
    matches!(
        code,
        DiagnosticCode::IntendedTargetCap | DiagnosticCode::IntendedTargetCapPartial
    )
}

/// A `.crn` read, parsed, resolved and lowered, plus what the lowering failed to produce.
pub(crate) struct Lowered {
    pub(crate) source: String,
    pub(crate) block_ir: BlockArrayIr,
    pub(crate) dropped_scopes: Vec<String>,
    /// The `@requires` floors this edition's build is held to, if the source declares any.
    pub(crate) version_floors: Option<DeclaredFloors>,
}

/// Read, parse, resolve and lower `file` for `edition`'s build.
///
/// Ids are checked against `target` unless it is `None` or the source's floors refuse it: the
/// floor refusal then explains the failed build, not an `E_UNKNOWN_ID`.
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
    let registry = builtin_pack(edition).view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // `check`'s gate, so `compile` never certifies a source `check` rejects.
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

/// A walled three-by-three `struct probe` lowered unpinned, for unit tests to rekey or corrupt.
#[cfg(test)]
pub(crate) fn lowered_probe(edition: Edition) -> BlockArrayIr {
    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(&ir, &resolution, Some(&builtin_pack(edition).view(None)))
}
