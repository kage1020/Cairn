//! The findings every command reports before it builds anything: [`check()`]'s, with
//! `@intended_targets` weighed in the editions a run asks about.

use cairn_lang_core::ast::Header;
use cairn_lang_core::check::{Diagnostic, weigh_intended_targets};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::{DiagnosticCode, Edition, Module, check};

use crate::registry::builtin_pack;
use crate::target::supported_versions;

/// Every diagnostic a command must report, in the order the passes ran: [`check()`]'s, which
/// include the resolver's, then `lowering`'s.
///
/// `asked` is the editions `@intended_targets` is weighed in.
#[must_use]
pub fn build_diagnostics(
    module: &Module,
    ir: &IntentModule,
    edition: Option<Edition>,
    asked: &[Edition],
    lowering: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
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
fn intended_target_findings(module: &Module, asked: &[Edition]) -> Vec<Diagnostic> {
    // Packs parse on first use, which a file without the header should not pay for.
    if !module
        .headers
        .iter()
        .any(|header| matches!(header, Header::IntendedTargets { .. }))
    {
        return Vec::new();
    }
    let mut findings: Vec<Diagnostic> = Vec::new();
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

/// Whether `code` is one of the two `@intended_targets` cap findings.
fn is_intended_target_cap(code: DiagnosticCode) -> bool {
    matches!(
        code,
        DiagnosticCode::IntendedTargetCap | DiagnosticCode::IntendedTargetCapPartial
    )
}

/// The distinct editions an `--editions` list names, in the order it first names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedEditions(Vec<Edition>);

impl RequestedEditions {
    /// The editions, in the order the list first names them.
    #[must_use]
    pub fn as_slice(&self) -> &[Edition] {
        &self.0
    }
}

/// Read an `--editions` list as the distinct editions it asks about.
///
/// # Errors
///
/// The message refusing an empty entry or a name that is not an edition.
pub fn requested_editions(editions: &[String]) -> Result<RequestedEditions, String> {
    // A comma-separated list with a doubled comma, `java,,bedrock`, gives an empty entry.
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

/// The editions a command with one optional pin is about: the pin, or both without one.
#[must_use]
pub fn weighed_editions(pin: Option<Edition>) -> Vec<Edition> {
    pin.map_or_else(|| vec![Edition::Java, Edition::Bedrock], |one| vec![one])
}
