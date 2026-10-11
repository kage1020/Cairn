//! The findings of the redstone passes, shaped like core's `check::Diagnostic` with a code enum of
//! their own.

use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use serde::{Serialize, Serializer};

/// Stable identifier for a kind of redstone [`Diagnostic`]; its `E_*`/`W_*` string is the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A `logic` line or actuator binding names a signal nothing defines.
    LogicUnboundSignal,
    /// Two sources drive one signal; the first one wins.
    LogicMultipleDrivers,
    /// The `logic` bindings form a cycle.
    LogicCycle,
    /// A `logic` binding uses an expression form the combinational lowering has no gate for.
    LogicUnsupportedPrimitive,
    /// A `logic` binding nothing reads.
    LogicUnusedSignal,
    /// A scope with something to place has no `circuit region=` reservation, or no `size=WxH`.
    NoCircuitRegion,
    /// A scope's netlist does not fit its `circuit region=` reservation.
    RouteCongestion,
    /// Two of a scope's nets run within one step of each other across layers.
    RouteCrossLayerClearance,
    /// A driver segment runs past [`crate::delay::MAX_ATTENUATION_SEGMENT`], or a dust run past its
    /// allowance has no coord a buffer repeater can take.
    AttenuationLimit,
    /// Lowering a `logic` binding descended past [`crate::synth::MAX_LOWERING_DEPTH`].
    LogicNestingTooDeep,
    /// A signal binding sits on a member that cannot host it, or inside a `[selector]`.
    LogicMisplacedBinding,
    /// A position that has to name a signal (`sig.<name>`) does not.
    LogicInvalidSignal,
    /// A `sig.` value sits under a key that is not an actuator key.
    LogicUnknownBindingKey,
    /// A selector-form actuator binding picks no single component.
    LogicUnresolvedPatch,
    /// A component carries a second binding under an actuator key it already has.
    LogicDuplicateBinding,
}

impl DiagnosticCode {
    /// Stable string form: `E_` for an error, `W_` for a warning.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LogicUnboundSignal => "E_LOGIC_UNBOUND_SIGNAL",
            Self::LogicMultipleDrivers => "E_LOGIC_MULTIPLE_DRIVERS",
            Self::LogicCycle => "E_LOGIC_CYCLE",
            Self::LogicUnsupportedPrimitive => "E_LOGIC_UNSUPPORTED_PRIMITIVE",
            Self::LogicUnusedSignal => "W_LOGIC_UNUSED_SIGNAL",
            Self::NoCircuitRegion => "E_NO_CIRCUIT_REGION",
            Self::RouteCongestion => "E_ROUTE_CONGESTION",
            Self::RouteCrossLayerClearance => "W_ROUTE_CROSS_LAYER_CLEARANCE",
            Self::AttenuationLimit => "E_ATTENUATION_LIMIT",
            Self::LogicNestingTooDeep => "E_LOGIC_NESTING_TOO_DEEP",
            Self::LogicMisplacedBinding => "E_LOGIC_MISPLACED_BINDING",
            Self::LogicInvalidSignal => "E_LOGIC_INVALID_SIGNAL",
            Self::LogicUnknownBindingKey => "E_LOGIC_UNKNOWN_BINDING_KEY",
            Self::LogicUnresolvedPatch => "E_LOGIC_UNRESOLVED_PATCH",
            Self::LogicDuplicateBinding => "E_LOGIC_DUPLICATE_BINDING",
        }
    }

    /// Severity of this code. Errors set the `cairn synth` exit code; warnings do not.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::LogicUnboundSignal
            | Self::LogicMultipleDrivers
            | Self::LogicCycle
            | Self::LogicUnsupportedPrimitive
            | Self::NoCircuitRegion
            | Self::RouteCongestion
            | Self::AttenuationLimit
            | Self::LogicNestingTooDeep
            | Self::LogicMisplacedBinding
            | Self::LogicInvalidSignal
            | Self::LogicUnknownBindingKey
            | Self::LogicUnresolvedPatch
            | Self::LogicDuplicateBinding => Severity::Error,
            Self::LogicUnusedSignal | Self::RouteCrossLayerClearance => Severity::Warning,
        }
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Secondary location or footer attached to a [`Diagnostic`], re-exported from core.
pub use cairn_lang_core::check::DiagnosticNote;

/// One finding of a redstone pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Byte range the primary message points at.
    #[serde(skip)]
    pub span: Span,
    /// Primary message rendered after the code on the first line.
    pub primary: String,
    /// Additional locations and footers, rendered as indented `note: ...` lines.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    /// Severity of this finding, read from [`DiagnosticCode::severity`].
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// A finding with `code`, its primary span and message, and no notes.
    #[must_use]
    pub fn new(code: DiagnosticCode, span: Span, primary: String) -> Self {
        Self {
            code,
            span,
            primary,
            notes: Vec::new(),
        }
    }

    /// Adds a note pointing at a secondary source location.
    #[must_use]
    pub fn with_note(mut self, span: Span, message: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            span: Some(span),
            message: message.into(),
        });
        self
    }

    /// Adds a footer note with no span, such as "valid alternatives: ..." or "Fix: ...".
    #[must_use]
    pub fn with_footer(mut self, message: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            span: None,
            message: message.into(),
        });
        self
    }
}

/// An error with one `Fix:` footer, the shape of every place-and-route refusal.
pub(crate) fn error_with_footer(
    code: DiagnosticCode,
    span: Span,
    primary: String,
    footer: impl Into<String>,
) -> Diagnostic {
    let diag = Diagnostic::new(code, span, primary).with_footer(footer);
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

#[cfg(test)]
mod tests;
