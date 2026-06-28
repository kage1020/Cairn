//! Diagnostic-collecting validation over a parsed [`Module`] and its
//! [`IntentModule`].
//!
//! Each pass is non-fatal: passes accumulate findings into a
//! [`DiagnosticSink`] and the top-level [`check`] runs every pass before
//! returning. The order `duplicate` → `keyword_allowlist` →
//! `connect_arity` → `type_mismatch` is fixed so the emitted list is
//! stable across runs, but the diagnostics themselves are sorted by
//! source position once everything has finished collecting.
//!
//! The boundary with lowering is intentional: `crate::intent::lower` is a
//! total function (see its module doc) and never rejects input. Any
//! "structural surprise" — an unknown keyword, a duplicate `size=`, an `id=`
//! whose value is not a label — surfaces here as a [`Diagnostic`] instead of
//! a hard parse error, so a single `cairn check` invocation reports every
//! problem in a file rather than only the first one.

mod connect_arity;
mod diagnostic;
mod duplicate;
mod keyword_allowlist;
mod sink;
mod type_mismatch;

pub use diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, LineStarts, RenderedDiagnostic,
    RenderedNote, Severity, position_at,
};
pub use sink::DiagnosticSink;

use crate::ast::Module;
use crate::intent::IntentModule;

/// Run every validation pass over the given module + IR and collect all
/// findings.
///
/// Passes run unconditionally; none short-circuit, none depend on another's
/// findings being empty. The returned list is sorted by `(span.start,
/// span.end)` so consumers can stream it line-by-line.
///
/// A final theme-binding pass runs via [`crate::resolve::resolve`]; its
/// diagnostics (`E_UNRESOLVED_SLOT`, `E_UNKNOWN_SLOT_TARGET`,
/// `E_THEME_SELECTOR_UNMATCHED`) are merged with the syntactic findings so a
/// single `cairn check` invocation reports both kinds together.
#[must_use]
pub fn check(module: &Module, ir: &IntentModule) -> Vec<Diagnostic> {
    let mut sink = DiagnosticSink::new();
    duplicate::run(module, &mut sink);
    keyword_allowlist::run(ir, &mut sink);
    connect_arity::run(ir, &mut sink);
    type_mismatch::run(module, &mut sink);
    for d in crate::resolve::resolve(ir).diagnostics {
        sink.push(d);
    }
    sink.into_sorted()
}
