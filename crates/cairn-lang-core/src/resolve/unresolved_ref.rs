//! The finding for a cross-scope reference that names nothing, with its nearest match.

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::error::Span;
use crate::suggest::{did_you_mean_note, nearest_match};

/// An unresolved reference, with a nearest-match suggestion from `candidates`.
pub(super) fn unresolved_ref_diag<'a>(
    code: DiagnosticCode,
    primary: &str,
    span: Span,
    typo: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    let mut notes = Vec::new();
    notes.extend(nearest_match(typo, candidates).map(did_you_mean_note));
    Diagnostic {
        code,
        span,
        primary: primary.to_owned(),
        notes,
        data: None,
    }
}
