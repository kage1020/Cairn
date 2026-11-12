//! The findings of the redstone passes, shaped like core's `check::Diagnostic` with a code enum of
//! their own.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::check::{LineStarts, RenderedDiagnostic, Severity};
use cairn_lang_core::error::Span;
use serde::{Serialize, Serializer};

use crate::geometry::Voxel;

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
    /// A `logic` binding uses an expression form the logic lowering has no gate for.
    LogicUnsupportedPrimitive,
    /// A `logic` binding nothing reads.
    LogicUnusedSignal,
    /// A scope with something to place has no `circuit region=` reservation, or no `size=WxH`.
    NoCircuitRegion,
    /// A scope's netlist does not fit its `circuit region=` reservation, the layers take the struct
    /// past what one structure holds, or a stage lays the circuit wrong, which is a compiler bug.
    RouteCongestion,
    /// A sensor or actuator stands where the circuit under the floor cannot reach it.
    RouteUnreachableComponent,
    /// A part of a circuit is built of a token the target's pack declares no block for.
    UnknownAbstractToken,
    /// A macro has more stages than its dust carries, or a wire past
    /// [`crate::delay::DUST_ATTENUATION_LIMIT`] blocks has nowhere a buffer repeater fits.
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
    /// An `assert` the circuit does not meet.
    SimAssertionFailed,
    /// An `assert truth` or `assert always` whose signals depend on more sensors than a check
    /// runs every combination of.
    SimAssertionUnchecked,
    /// A component of the built struct that reads another through a block update.
    NoPortableImpl,
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
            Self::RouteUnreachableComponent => "E_ROUTE_UNREACHABLE_COMPONENT",
            Self::UnknownAbstractToken => "E_UNKNOWN_ABSTRACT_TOKEN",
            Self::AttenuationLimit => "E_ATTENUATION_LIMIT",
            Self::LogicNestingTooDeep => "E_LOGIC_NESTING_TOO_DEEP",
            Self::LogicMisplacedBinding => "E_LOGIC_MISPLACED_BINDING",
            Self::LogicInvalidSignal => "E_LOGIC_INVALID_SIGNAL",
            Self::LogicUnknownBindingKey => "E_LOGIC_UNKNOWN_BINDING_KEY",
            Self::LogicUnresolvedPatch => "E_LOGIC_UNRESOLVED_PATCH",
            Self::LogicDuplicateBinding => "E_LOGIC_DUPLICATE_BINDING",
            Self::SimAssertionFailed => "E_SIM_ASSERTION_FAILED",
            Self::SimAssertionUnchecked => "W_SIM_ASSERTION_UNCHECKED",
            Self::NoPortableImpl => "E_NO_PORTABLE_IMPL",
        }
    }

    /// Severity of this code: an error refuses the command that reports it, a warning does not.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::LogicUnboundSignal
            | Self::LogicMultipleDrivers
            | Self::LogicCycle
            | Self::LogicUnsupportedPrimitive
            | Self::NoCircuitRegion
            | Self::RouteCongestion
            | Self::RouteUnreachableComponent
            | Self::UnknownAbstractToken
            | Self::AttenuationLimit
            | Self::LogicNestingTooDeep
            | Self::LogicMisplacedBinding
            | Self::LogicInvalidSignal
            | Self::LogicUnknownBindingKey
            | Self::LogicUnresolvedPatch
            | Self::LogicDuplicateBinding
            | Self::SimAssertionFailed
            | Self::NoPortableImpl => Severity::Error,
            Self::LogicUnusedSignal | Self::SimAssertionUnchecked => Severity::Warning,
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

    /// Whether the primary span or a note's span lies inside one of `lines`.
    #[must_use]
    pub fn touches(&self, lines: &[&Span]) -> bool {
        std::iter::once(&self.span)
            .chain(self.notes.iter().filter_map(|note| note.span.as_ref()))
            .any(|span| {
                lines
                    .iter()
                    .any(|line| line.start <= span.start && span.end <= line.end)
            })
    }

    /// Render against `source` in the shape `spec/lint` "Machine-readable payload" gives.
    #[must_use]
    pub fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic {
        RenderedDiagnostic::new(
            self.code.as_str(),
            self.severity(),
            &self.span,
            &self.primary,
            &self.notes,
            source,
            lines,
        )
    }
}

/// `assert latency(FROM -> TO) <= N`, each name in its canonical spelling.
pub(crate) fn latency_text(from: &DottedRef, to: &DottedRef, bound: u32) -> String {
    format!("`assert latency({from} -> {to}) <= {bound}`")
}

/// The fix of a refusal no layout the stages before accept should reach.
pub(crate) const COMPILER_BUG_FIX: &str = "Fix: report this as a compiler bug, with the struct; meanwhile widen the struct's `size=WxH` so the circuit is laid differently";

/// An `E_ROUTE_CONGESTION` at the `circuit` line `span` for a circuit an earlier stage laid wrong.
pub(crate) fn compiler_bug(span: Span, primary: String) -> Diagnostic {
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        span,
        primary,
        COMPILER_BUG_FIX,
    )
}

/// The finding of `scope`'s circuit, under the `circuit` line `span`, with a part at `at` where
/// none fits.
pub(crate) fn overlapping(scope: &str, span: Span, at: Voxel) -> Diagnostic {
    compiler_bug(
        span,
        format!(
            "{scope} the circuit under this struct has a part at ({x}, {y}, {z}) where none fits",
            x = at.x,
            y = at.y,
            z = at.z,
        ),
    )
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
