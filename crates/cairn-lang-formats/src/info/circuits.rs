//! Laying each struct's circuit, and keeping the circuit findings every edition's run raises.

use cairn_lang_core::block_array::{BlockArrayIr, TargetRegistry};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::{Edition, Severity};
use cairn_lang_redstone::{Diagnostic as CircuitDiagnostic, build_circuits};

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

/// Each distinct finding of `runs` that every other run raises with the same code at the same span.
#[must_use]
pub fn raised_by_each(runs: &[Vec<CircuitDiagnostic>]) -> Vec<CircuitDiagnostic> {
    kept_findings(runs, |_, _| false)
}

/// Each distinct finding of `runs` that `own` says is its run's own, or that every other run
/// raises with the same code at the same span.
pub fn kept_findings(
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
