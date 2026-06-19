//! Accumulator that the `check` passes push [`Diagnostic`]s into.

use super::Diagnostic;

/// Collector that gathers diagnostics from one or more passes without
/// short-circuiting.
///
/// Passes call [`Self::push`] for each finding. The driver in
/// [`super::check`] consumes the sink via [`Self::into_sorted`] which
/// returns a stable, position-ordered vector — a single pass might emit
/// diagnostics out of order (e.g. a duplicate noticed mid-walk reports the
/// second occurrence after a later attribute), and the sort makes the wire
/// output reproducible.
#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diags: Vec<Diagnostic>,
}

impl DiagnosticSink {
    /// Empty sink. Equivalent to `DiagnosticSink::default()` but spells out
    /// the intent at call sites.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a single diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diags.push(diagnostic);
    }

    /// Number of diagnostics collected so far. Used by passes that need to
    /// suppress a follow-on finding when an earlier one already covered the
    /// same byte range.
    #[must_use]
    pub fn len(&self) -> usize {
        self.diags.len()
    }

    /// `true` when no diagnostics have been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diags.is_empty()
    }

    /// Drain into a `Vec` sorted by primary span position
    /// (`(start, end)`). Stable: passes that emit two diagnostics with the
    /// same span preserve their push order.
    #[must_use]
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        self.diags.sort_by_key(|d| (d.span.start, d.span.end));
        self.diags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{DiagnosticCode, Severity};

    fn diag(start: usize, end: usize, code: DiagnosticCode, msg: &str) -> Diagnostic {
        Diagnostic {
            code,
            severity: code.severity(),
            span: start..end,
            primary: msg.into(),
            notes: Vec::new(),
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
