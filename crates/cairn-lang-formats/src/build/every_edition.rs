//! The errors an unpinned check reports when every edition refuses a source the findings it keeps
//! let through (`spec/lint` "Error vs warning").

use cairn_lang_core::ast::Module;
use cairn_lang_core::check::DiagnosticNote;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::{Edition, Severity};

use super::circuits::check_circuits;
use super::finding::{Finding, Found};
use super::gate::build_diagnostics;
use super::lowering::check_lowering;

/// Each edition's errors under `cairn check --edition`, each noted with its edition, when every
/// edition raises one; empty when some edition raises none. An edition's circuit findings count
/// when `circuits` is set and its other findings hold no error.
#[must_use]
pub fn refused_by_every_edition(module: &Module, ir: &IntentModule, circuits: bool) -> Vec<Found> {
    let mut refused = Vec::new();
    for edition in Edition::weighed(None) {
        let lowering = check_lowering(ir, Some(edition), None).diagnostics;
        let mut errors: Vec<Found> =
            build_diagnostics(module, ir, Some(edition), &[edition], lowering)
                .into_iter()
                .map(Found::Core)
                .filter(|d| d.severity() == Severity::Error)
                .collect();
        if errors.is_empty() && circuits {
            errors.extend(
                check_circuits(ir, Some(edition), None)
                    .into_iter()
                    .map(Found::Circuit)
                    .filter(|d| d.severity() == Severity::Error),
            );
        }
        if errors.is_empty() {
            return Vec::new();
        }
        let note = DiagnosticNote {
            span: None,
            message: format!(
                "raised for `--edition {edition}`; every edition refuses this source, each with \
                 errors of its own"
            ),
        };
        for error in &mut errors {
            match error {
                Found::Core(d) => d.notes.push(note.clone()),
                Found::Circuit(d) => d.notes.push(note.clone()),
            }
        }
        refused.extend(errors);
    }
    refused
}

#[cfg(test)]
mod tests;
