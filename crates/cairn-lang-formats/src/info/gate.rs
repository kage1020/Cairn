//! The findings a command must report before it builds anything: `check`'s, `@intended_targets`
//! weighed per edition, then the lowering's.

use cairn_lang_core::ast::{Header, Module};
use cairn_lang_core::check::weigh_intended_targets;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::{Diagnostic, DiagnosticCode, Edition, check};

use crate::registry::{RegistryPack, builtin_pack};

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

/// Every version a pack can build for, in ascending release order.
#[must_use]
pub fn supported_versions(pack: &RegistryPack) -> Vec<String> {
    pack.targetable()
        .into_iter()
        .map(|entry| entry.mc_version.clone())
        .collect()
}
