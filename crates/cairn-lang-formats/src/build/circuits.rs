//! Each struct's circuit laid under it, and which of its findings a run reports when it weighs
//! more than one edition.

use cairn_lang_core::block_array::{BlockArrayIr, TargetRegistry, lower_to_block_array};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{Edition, Severity};
use cairn_lang_redstone::{Diagnostic as CircuitDiagnostic, build_circuits};

use super::gate::weighed_editions;
use crate::registry::builtin_pack;
use crate::target::{ResolvedTarget, resolve_target};

/// Lay each struct's circuit under it for `edition`, its ids read from `registry`, unless
/// `built` already holds an error: a member that failed to lower is not there to wire.
pub fn lay_circuits(
    ir: &IntentModule,
    built: &mut BlockArrayIr,
    edition: Edition,
    registry: &dyn TargetRegistry,
) -> Vec<CircuitDiagnostic> {
    if built
        .diagnostics
        .iter()
        .any(|d| d.severity() == Severity::Error)
    {
        return Vec::new();
    }
    build_circuits(ir, built, edition, Some(registry))
}

/// The circuit findings `cairn check` reports: the pinned edition's, at the pinned version when
/// `target` names one it ships, or without a pin those each edition raises at the same span and
/// each edition's own, which touch a line under its guards (`spec/lint` "Error vs warning").
#[must_use]
pub fn check_circuits(
    ir: &IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> Vec<CircuitDiagnostic> {
    let resolved = edition
        .zip(target)
        .and_then(|(edition, target)| resolve_target(edition, target).ok());
    let pinned = resolved.as_ref().map(ResolvedTarget::mc_version);
    let editions = weighed_editions(edition);
    let runs: Vec<Vec<CircuitDiagnostic>> = editions
        .iter()
        .map(|&edition| {
            let registry = builtin_pack(edition).view(pinned);
            let mut built = lower_to_block_array(ir, &resolve(ir, Some(edition)), Some(&registry));
            lay_circuits(ir, &mut built, edition, &registry)
        })
        .collect();
    let guarded: Vec<Vec<&Span>> = editions
        .iter()
        .map(|&edition| ir.lines_guarded_for(edition))
        .collect();
    kept_findings(&runs, |run, finding| finding.touches(&guarded[run]))
}

/// Each distinct finding of `runs` that every other run raises with the same code at the same span.
#[must_use]
pub fn raised_by_each(runs: &[Vec<CircuitDiagnostic>]) -> Vec<CircuitDiagnostic> {
    kept_findings(runs, |_, _| false)
}

/// Each distinct finding of `runs` that `own` says is its run's own, or that every other run
/// raises with the same code at the same span.
fn kept_findings(
    runs: &[Vec<CircuitDiagnostic>],
    own: impl Fn(usize, &CircuitDiagnostic) -> bool,
) -> Vec<CircuitDiagnostic> {
    let mut kept: Vec<CircuitDiagnostic> = Vec::new();
    for (i, found) in runs.iter().enumerate() {
        for finding in found {
            let raised_by_each = runs
                .iter()
                .enumerate()
                .all(|(j, other)| j == i || other.iter().any(|o| same_place(o, finding)));
            if (own(i, finding) || raised_by_each) && !kept.contains(finding) {
                kept.push(finding.clone());
            }
        }
    }
    kept
}

/// Whether `a` and `b` are one finding: the same code at the same span.
#[must_use]
pub fn same_place(a: &CircuitDiagnostic, b: &CircuitDiagnostic) -> bool {
    a.code == b.code && a.span == b.span
}
