//! Diagnostic codes emitted by the redstone synth pass.
//!
//! Structurally parallel to `cairn-lang-core::check::Diagnostic` — same
//! `code / severity / span / primary / notes` shape — but the code enum
//! lives in this crate so the redstone-specific taxonomy does not couple
//! `cairn-lang-core` to the redstone pipeline. Callers that want to render
//! findings with the same gcc-style formatter `cairn check` uses can borrow
//! the shared [`Severity`] rendering and the `code.as_str()` convention
//! adopted below.
//!
//! Message prose follows the self-correction triple from `spec/lint` §11.4:
//! what is wrong, valid alternatives, suggested fix. The primary string
//! renders the first clause; notes carry the alternatives and suggestion so
//! LSP quick-fix logic can dispatch on structured fields without re-parsing
//! the human-readable prose.

use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use serde::{Serialize, Serializer};

/// Stable identifier for a kind of redstone synth [`Diagnostic`].
///
/// The string form (`E_LOGIC_UNBOUND_SIGNAL`, ...) is the contract surface;
/// LSP quick-fix logic and CI annotators match on it without inspecting the
/// prose `primary` message. `#[non_exhaustive]` so a follow-up PR adding
/// codes for netlist / placement / route stages does not break external
/// exhaust matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A `logic sig.X = <expr>` references a signal name that no sensor
    /// emits and no earlier `logic` line defines in the same scope, or an
    /// actuator's `opened_by=` / `powered_by=` / `lit_by=` / `fired_by=`
    /// points at an undefined signal. Fail-loud: silently dropping the
    /// reference would leave the actuator wired to air (Java) or nothing
    /// (Bedrock) with no signal to the author.
    LogicUnboundSignal,
    /// Two or more `logic sig.X = ...` lines define the same LHS in the
    /// same scope. The synth pass picks the first definition; the duplicate
    /// is flagged with a `first declared here` note pointing at the winner.
    LogicMultipleDrivers,
    /// A `logic` binding graph has at least one cycle
    /// (`logic sig.a = sig.b; logic sig.b = sig.a`). Cycles cannot lower to
    /// a combinational DAG and would require a latch macro (out of scope
    /// for the M6-PR1 combinational-only slice).
    LogicCycle,
    /// A `logic sig.X = ...` binding is unreachable — no actuator, no
    /// downstream logic references its LHS. Warning-severity because the
    /// synthesised DAG is still valid; an unused signal is usually a typo
    /// on the reference side but occasionally an intentional scratch.
    LogicUnusedSignal,
}

impl DiagnosticCode {
    /// Stable string form used in text output and matched by downstream
    /// tooling. Errors take the `E_` prefix, warnings take `W_`; the split
    /// mirrors `cairn-lang-core::check::DiagnosticCode`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LogicUnboundSignal => "E_LOGIC_UNBOUND_SIGNAL",
            Self::LogicMultipleDrivers => "E_LOGIC_MULTIPLE_DRIVERS",
            Self::LogicCycle => "E_LOGIC_CYCLE",
            Self::LogicUnusedSignal => "W_LOGIC_UNUSED_SIGNAL",
        }
    }

    /// Severity of this code. Errors participate in the `cairn synth`
    /// exit code; warnings do not.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::LogicUnboundSignal | Self::LogicMultipleDrivers | Self::LogicCycle => {
                Severity::Error
            }
            Self::LogicUnusedSignal => Severity::Warning,
        }
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialise as the stable string form so JSON consumers see the
        // same contract surface the text format uses.
        serializer.serialize_str(self.as_str())
    }
}

/// Secondary location attached to a [`Diagnostic`]. Renders as an indented
/// `note: ...` line under the primary finding. `span` may be `None` when
/// the note is a footer such as "valid alternatives: sig.step" that does
/// not point at a distinct byte range.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagnosticNote {
    /// Byte range the note refers to, when the note points at a distinct
    /// secondary location.
    #[serde(skip)]
    pub span: Option<Span>,
    /// Human-readable note text.
    pub message: String,
}

/// One finding emitted by the redstone synth pass.
///
/// `#[non_exhaustive]` so external crates cannot construct a [`Diagnostic`]
/// by struct literal — future fields (structured payload, quick-fix data)
/// can land without breaking downstream consumers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Severity of the finding.
    pub severity: Severity,
    /// Byte range the primary message points at.
    #[serde(skip)]
    pub span: Span,
    /// Primary message rendered after the code on the first line.
    pub primary: String,
    /// Additional locations relevant to this finding. Emitted as indented
    /// `note: ...` lines in the text format.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    /// Build a [`Diagnostic`] from a code (severity is derived) plus the
    /// primary span and message. The most common construction path — used
    /// by every non-cycle site in the synth pass.
    #[must_use]
    pub fn new(code: DiagnosticCode, span: Span, primary: String) -> Self {
        Self {
            code,
            severity: code.severity(),
            span,
            primary,
            notes: Vec::new(),
        }
    }

    /// Attach a note pointing at a distinct secondary source location.
    /// Chainable so the synth pass reads top-to-bottom.
    #[must_use]
    pub fn with_note(mut self, span: Span, message: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            span: Some(span),
            message: message.into(),
        });
        self
    }

    /// Attach an informational note with no distinct secondary span
    /// (footer-style: "valid alternatives: ...", "suggested fix: ...").
    /// Chainable so the synth pass reads top-to-bottom.
    #[must_use]
    pub fn with_footer(mut self, message: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            span: None,
            message: message.into(),
        });
        self
    }
}
