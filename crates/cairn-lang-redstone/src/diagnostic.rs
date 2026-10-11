//! The findings of the redstone passes.
//!
//! Shaped like `cairn_lang_core::check::Diagnostic` (code, severity, span, primary, notes), with a
//! code enum of its own so `cairn-lang-core` does not know the redstone taxonomy. A renderer for
//! core's findings renders these through the shared [`Severity`] and `code.as_str()`.
//!
//! Messages follow the self-correction triple of `spec/lint`: the primary says what is wrong, and
//! [`DiagnosticNote`]s give the valid alternatives and the fix. A machine consumer matches on the
//! stable `E_*` / `W_*` string of [`DiagnosticCode`] and reads the notes as prose.

use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use serde::{Serialize, Serializer};

/// Stable identifier for a kind of redstone [`Diagnostic`].
///
/// The string form (`E_LOGIC_UNBOUND_SIGNAL`, ...) is the contract: tooling matches on it rather
/// than on the prose. `#[non_exhaustive]` so a new code does not break an external match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// `EnumIter` lets the tests walk every variant rather than a list that can miss the newest one;
// `cfg(test)` keeps the proc macro out of shipped builds.
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A `logic sig.X = <expr>` references a signal no sensor emits and no earlier `logic` line in
    /// the scope defines, or an actuator's `opened_by=`, `powered_by=`, `lit_by=` or `fired_by=`
    /// names an undefined signal. Dropping the reference would leave the actuator wired to nothing
    /// with no word to the author.
    LogicUnboundSignal,
    /// Two sources drive one signal: two `logic` lines with one left-hand side in a scope, or a
    /// `logic` left-hand side a sensor already emits. The first source wins, so references still
    /// resolve; the others carry `first declared here` or `sensor emits this signal here` notes.
    LogicMultipleDrivers,
    /// The `logic` bindings form a cycle (`logic sig.a = sig.b; logic sig.b = sig.a`), which
    /// cannot lower to a combinational DAG.
    LogicCycle,
    /// A `logic` binding uses an expression form the combinational lowering has no gate for.
    LogicUnsupportedPrimitive,
    /// A `logic sig.X = ...` binding nothing reads, neither an actuator nor another `logic` line.
    /// A warning, since the DAG is still valid: an unused signal is usually a typo on the reading
    /// side, occasionally an intentional scratch.
    LogicUnusedSignal,
    /// A scope has cells or actuator pads to place but no `circuit region=<label> void=<N>`
    /// reservation, or no `size=WxH` for the reservation to sit in. `spec/redstone`
    /// "Place-and-route" fails loud here rather than place voxels outside the declared footprint.
    ///
    /// The label is the author's to choose (the spec's own example is `region=basement`), so it is
    /// checked only for being present and non-empty. Fix: add a `circuit region=` line with a
    /// non-empty label and a `void=` of at least 1, in a scope with a `size=WxH` header.
    NoCircuitRegion,
    /// A scope's netlist does not fit its `circuit region=<label> void=<N>` reservation: the
    /// canonical failure of `spec/redstone` "Place-and-route", answered with raising `void`,
    /// enlarging the region, or splitting the logic across several scopes.
    ///
    /// Six shapes take this code (see [`crate::placement`] for why one code): the volume is short
    /// of the estimated footprint; the row is shorter than the spaced layout's `2n + 1`; the
    /// reservation is too shallow for a clear row either side of the cell row; too shallow for the
    /// I/O pads; one column wide in a scope with no cells and both sensors and actuators; or the
    /// router proves a sink walled in by components, other nets' dust, and the coords beside that
    /// dust. Raising `void` fixes only the first and the last. A sink with no route within the
    /// attenuation cap that the router cannot prove walled in is [`Self::AttenuationLimit`].
    RouteCongestion,
    /// Two of a scope's nets run within one step of each other across layers: one strand directly
    /// over another, or over it and one step across, the staircase dust climbs.
    ///
    /// A warning, because `spec/redstone` "Place-and-route" keeps nets apart only in one plane and
    /// makes separating them across layers the physical tile layer's obligation; whether the upper
    /// strand reads the lower depends on what stands between them. The finding lists the coords
    /// and nets that obligation covers. Fix: none in the source; enlarging the region only
    /// lengthens the run a net makes on the upper layer.
    RouteCrossLayerClearance,
    /// A driver segment's routed length (driver to a cell or an actuator pad, along the route)
    /// exceeds the sanity cap [`crate::delay::MAX_ATTENUATION_SEGMENT`]. Buffer repeaters cover
    /// any length up to the cap; past it the pass refuses rather than charge an unrealisable chain
    /// into `local_delay_ticks`. Fix: enlarge the `circuit region=` footprint, place the cell or
    /// actuator closer to its driver, or split the logic across several scopes.
    ///
    /// Also fires when a run of dust would pass its allowance (15 blocks since the last block
    /// that restored strength, or less on the wire into a cell that passes on the strength it
    /// reads) and every coord close enough for a repeater turns, climbs or branches: a buffer
    /// repeater carries a signal only where the wire runs straight through it at one height.
    ///
    /// Stage 2 raises it before any route is measured for a sink further from its driver than the
    /// cap in a straight line, and for one no path within the cap reaches that the router cannot
    /// prove walled in (see [`Self::RouteCongestion`]). The second claims only that no route within
    /// the cap exists.
    AttenuationLimit,
    /// Lowering a `logic` binding descended past [`crate::synth::MAX_LOWERING_DEPTH`], where the
    /// native stack would overflow and abort the process.
    ///
    /// A binding lowers by descending into what it references, so a chain declared in reverse
    /// dependency order nests each binding inside the one that references it. Reported once per
    /// binding the lowering stops in, on the outermost binding of the chain, with the depth, a note
    /// on each of the next few and a count of the rest. Fix: declare the chain in dependency order,
    /// which lowers at any length because each reference is resolved when it is reached.
    LogicNestingTooDeep,
    /// A signal binding is written where nothing reads it: on a member whose kind cannot host it,
    /// or inside the `[selector]` of a line whose binding belongs after the brackets.
    ///
    /// The first is a `-> value` sensor tail, or one of the actuator keys `spec/redstone` "Signal
    /// binding" lists, on the wrong component. It is asked before the value, since no edit to the
    /// value makes `walls` carry a tail. That section pairs each binding with its component:
    /// `opened_by=` with `door`, `lit_by=` with `lamp`, `powered_by=` with `piston`, `fired_by=`
    /// with `dispenser`, and the tail with a sensor. Of those, only `door` and `pressure_plate` are
    /// keywords the surface accepts, so the three actuator keys other than `opened_by=` have no
    /// legal host.
    ///
    /// The second is `door[id=front,opened_by=sig.x]`: the brackets pick an existing member and
    /// the binding goes after them, as that section writes it. Fix: move the binding onto the
    /// component that carries it.
    LogicMisplacedBinding,
    /// A position that has to name a signal does not. A signal name is `sig.` and exactly one
    /// segment after it, since sensors emit into the `sig.` namespace and actuators read one
    /// segment from it. Three positions carry one:
    ///
    /// - a `logic` line's left-hand side;
    /// - a sensor's `-> value` tail;
    /// - the value under an actuator key on the component that reads it.
    ///
    /// Fix: name the signal `sig.<name>`. For a bare identifier the message offers that spelling,
    /// the one shape with a single reading; `opened_by=3` names nothing `sig.` would repair. A
    /// left-hand side of `sig` alone is told to add a name, and one with several segments after
    /// `sig.` to keep one.
    LogicInvalidSignal,
    /// A `sig.`-headed value sits under a key that is not one of the actuator keys in
    /// `spec/redstone` "Signal binding", so the author meant a signal and nothing reads it. A typo
    /// (`oepend_by=`) gets a `did you mean` note; any other key gets the list. Fix: correct the
    /// key, or drop the argument if the member is not an actuator.
    ///
    /// Keyed on the value rather than a per-keyword argument schema, since no legal non-actuator
    /// argument takes a `sig.` value. An actuator key with a *non*-`sig.` value is
    /// [`Self::LogicInvalidSignal`] on the component that key is paired with,
    /// [`Self::LogicMisplacedBinding`] anywhere else, and only `E_UNKNOWN_KEYWORD` on a keyword the
    /// role table does not know. A pair inside a `[...]` selector is answered the same three ways,
    /// by whichever fault moving it out of the brackets would not fix.
    LogicUnknownBindingKey,
    /// An actuator binding in the selector form picks no single physical component.
    /// `door[id=front] opened_by=sig.x` binds the door declared as `door id=front ...` in the same
    /// scope, looked up by `cairn_lang_core::intent::actuator_patch_target`; a selector with no
    /// readable `id=`, or an id no door or several doors carry, leaves nothing to act on, and
    /// block-array lowering defers such a patch for the same reason. Fix: set `[id=<label>]` to the
    /// id of one `door` declared in this scope without brackets, or write `opened_by=` on that
    /// door's own line; give doors that share an id distinct ones.
    LogicUnresolvedPatch,
    /// A physical component carries a second binding under an actuator key it already has, on its
    /// own line, through a selector-form patch, or both. Each binding would be an output port with
    /// its own pad, a wired OR the author never wrote; `spec/redstone` "The logic layer is a
    /// dependency DAG" is where signals are combined. The binding on the component's own line
    /// counts as the first. Fix: bind the component once, to a `logic` line that combines the
    /// signals, or delete a duplicate of the same signal.
    LogicDuplicateBinding,
}

impl DiagnosticCode {
    /// Stable string form used in text output and matched by tooling: `E_` for an error, `W_` for
    /// a warning, as in `cairn_lang_core::check::DiagnosticCode`.
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

/// Secondary location attached to a [`Diagnostic`], rendered as an indented `note: ...` line;
/// `span` is `None` for a footer such as "valid alternatives: sig.step".
///
/// `cairn_lang_core`'s type, re-exported so a consumer rendering both crates' findings needs no
/// adapter. [`Diagnostic`] is its own type only because its `code` is.
pub use cairn_lang_core::check::DiagnosticNote;

/// One finding of a redstone pass. `#[non_exhaustive]` so a new field does not break a
/// downstream struct literal.
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
    /// Severity of this finding, read from [`DiagnosticCode::severity`]. A method rather than a
    /// field, as on [`cairn_lang_core::Diagnostic`], so the two cannot disagree.
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

/// An error with one `Fix:` footer, the shape of every place-and-route refusal. Debug-asserts the
/// code is an error, so a pass cannot drop a scope over a warning.
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
