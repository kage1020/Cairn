//! Which of each edition's findings a check that pins no edition reports
//! (`spec/lint` "Error vs warning").

use std::collections::HashSet;

use crate::error::Span;

use super::{Diagnostic, sort_by_position};

/// One edition's findings, with the source only that edition's build reads.
pub(crate) struct EditionFindings<'a> {
    /// What the edition's pass raised.
    pub(crate) found: Vec<Diagnostic>,
    /// The lines under the edition's `@edition` guards.
    pub(crate) guarded: Vec<&'a Span>,
    /// The themes only the edition binds.
    pub(crate) themes: Vec<&'a Span>,
}

/// The findings of `editions` an unpinned check reports, sorted: an edition's own, whose span or
/// one of whose notes' spans is on a line under its guards, or whose span is in a theme only it
/// binds, and any other that every edition raises with the same code at the same span, once per
/// wording.
pub(crate) fn agreed(mut editions: Vec<EditionFindings<'_>>) -> Vec<Diagnostic> {
    if editions.len() == 1 {
        return editions.pop().map(|one| one.found).unwrap_or_default();
    }
    let mut seen: HashSet<&Diagnostic> = HashSet::new();
    let mut kept: Vec<Diagnostic> = Vec::new();
    for (i, edition) in editions.iter().enumerate() {
        for finding in &edition.found {
            let own = finding.touches(&edition.guarded)
                || edition.themes.iter().any(|theme| {
                    theme.start <= finding.span.start && finding.span.end <= theme.end
                });
            let raised_by_each = editions.iter().enumerate().all(|(j, other)| {
                i == j
                    || other
                        .found
                        .iter()
                        .any(|o| o.code == finding.code && o.span == finding.span)
            });
            if (own || raised_by_each) && seen.insert(finding) {
                kept.push(finding.clone());
            }
        }
    }
    sort_by_position(&mut kept);
    kept
}
