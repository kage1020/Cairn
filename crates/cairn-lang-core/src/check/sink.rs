//! Accumulator that the `check` passes push [`Diagnostic`]s into.

use crate::diagnostic::{Diagnostic, sort_by_position};

/// Collects the findings of every `check` pass.
///
/// Passes push in walk order, which is not source order (a duplicate is
/// reported where its second occurrence is reached), so the list leaves
/// only through [`Self::into_sorted`].
#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diags: Vec<Diagnostic>,
}

impl DiagnosticSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a single diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diags.push(diagnostic);
    }

    /// Number of diagnostics collected so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.diags.len()
    }

    /// `true` when no diagnostics have been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diags.is_empty()
    }

    /// Drain into a `Vec` sorted by `(span.start, span.end)`, keeping push
    /// order among diagnostics with the same span.
    #[must_use]
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        sort_by_position(&mut self.diags);
        self.diags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{DiagnosticCode, Severity};

    fn diag(start: usize, end: usize, code: DiagnosticCode, msg: &str) -> Diagnostic {
        Diagnostic {
            code,
            span: start..end,
            primary: msg.into(),
            notes: Vec::new(),
            data: None,
        }
    }

    #[test]
    fn into_sorted_orders_by_span_start_then_end() {
        let mut sink = DiagnosticSink::new();
        sink.push(diag(10, 20, DiagnosticCode::DuplicateArg, "later"));
        sink.push(diag(0, 5, DiagnosticCode::DuplicateArg, "first"));
        sink.push(diag(0, 3, DiagnosticCode::DuplicateSlot, "tightest"));
        let sorted = sink.into_sorted();
        assert_eq!(sorted[0].primary, "tightest");
        assert_eq!(sorted[1].primary, "first");
        assert_eq!(sorted[2].primary, "later");
    }

    #[test]
    fn into_sorted_is_stable_for_same_span() {
        let mut sink = DiagnosticSink::new();
        sink.push(diag(0, 4, DiagnosticCode::DuplicateArg, "alpha"));
        sink.push(diag(0, 4, DiagnosticCode::DuplicateArg, "beta"));
        let sorted = sink.into_sorted();
        assert_eq!(sorted[0].primary, "alpha");
        assert_eq!(sorted[1].primary, "beta");
    }

    #[test]
    fn empty_sink_has_severity_independent_invariants() {
        let sink = DiagnosticSink::new();
        assert!(sink.is_empty());
        assert_eq!(sink.len(), 0);
        let drained = sink.into_sorted();
        assert!(drained.is_empty());
        assert_eq!(Severity::Error.as_str(), "error");
    }
}
