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
//! Message prose follows the self-correction triple from `spec/lint` §11:
//! what is wrong, valid alternatives, suggested fix. The primary string
//! carries the first clause; the alternatives and the suggestion land in
//! [`DiagnosticNote`]s so the human-readable output still reads as three
//! groups. LSP-facing structured payloads are intentionally absent for
//! now — a downstream consumer that needs machine-readable quick-fix data
//! matches on the stable [`DiagnosticCode`] `E_*` / `W_*` string and
//! parses the note prose, mirroring the current `cairn-lang-core` contract.

use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use serde::{Serialize, Serializer};

/// Stable identifier for a kind of redstone synth [`Diagnostic`].
///
/// The string form (`E_LOGIC_UNBOUND_SIGNAL`, ...) is the contract surface;
/// LSP quick-fix logic and CI annotators match on it without inspecting the
/// prose `primary` message. `#[non_exhaustive]` so a follow-up pass adding
/// codes for netlist / placement / route stages does not break external
/// exhaust matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// `EnumIter` lets the in-crate tests walk every variant instead of
// re-listing them, which is the only way a check over "all codes" cannot
// silently omit whichever was added last. `cfg(test)` keeps the proc macro
// out of shipped builds; `cairn_lang_core::check::DiagnosticCode` carries
// the same guard for the same reason.
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A `logic sig.X = <expr>` references a signal name that no sensor
    /// emits and no earlier `logic` line defines in the same scope, or an
    /// actuator's `opened_by=` / `powered_by=` / `lit_by=` / `fired_by=`
    /// points at an undefined signal. Fail-loud: silently dropping the
    /// reference would leave the actuator wired to air (Java) or nothing
    /// (Bedrock) with no signal to the author.
    LogicUnboundSignal,
    /// Two or more sources try to drive the same signal. Fires for
    /// `logic sig.X = ...` lines that share an LHS in the same scope, and
    /// for a `logic` LHS that collides with a sensor already emitting the
    /// signal. The first source wins so downstream references still
    /// resolve; the losers surface with `first declared here` /
    /// `sensor emits this signal here` notes.
    LogicMultipleDrivers,
    /// A `logic` binding graph has at least one cycle
    /// (`logic sig.a = sig.b; logic sig.b = sig.a`). Cycles cannot lower
    /// to a combinational DAG and would require a latch macro (not yet
    /// wired into the synth path).
    LogicCycle,
    /// A `logic` binding uses a boolean primitive the current combinational
    /// lowering does not know how to synthesise. Fires when the AST grows a
    /// new `Expr` variant (e.g. a future function-call form for `xor` /
    /// `mux`) that the synth pass has not yet been extended for.
    LogicUnsupportedPrimitive,
    /// A `logic sig.X = ...` binding is unreachable — no actuator, no
    /// downstream logic references its LHS. Warning-severity because the
    /// synthesised DAG is still valid; an unused signal is usually a typo
    /// on the reference side but occasionally an intentional scratch.
    LogicUnusedSignal,
    /// A scope has redstone cells or actuator pads to place but the
    /// enclosing struct / def
    /// declared no `circuit region=<label> void=<N>` reservation (or the
    /// enclosing scope had no `size=WxH` for the reservation to sit
    /// inside). Fail-loud per `spec/redstone` §14.5 — silently placing
    /// cells "somewhere" would produce voxels outside the author's
    /// declared footprint. Fix: add a `circuit region=` line with a
    /// non-empty `region=` label and a `void=` of at least 1, and give
    /// the enclosing scope a `size=WxH` header. The label names the
    /// reservation and is the author's to choose — §14.5's own example
    /// is `region=basement`, which is not a member keyword — so it is
    /// checked for being present and non-empty, nothing more.
    NoCircuitRegion,
    /// The synthesised netlist for a scope does not fit its
    /// `circuit region=<label> void=<N>` reservation. `spec/redstone`
    /// §14.5's canonical failure: routing cannot be confined to the
    /// reserved region, so the pass fails loud with the self-correction
    /// triple ("increase `void`", "enlarge region", "split into multiple
    /// `circuit` blocks"). Four shapes reach it — the reserved volume is
    /// short of the netlist's estimated footprint; the reserved row is
    /// shorter than the spaced single-row layout needs, which is twice
    /// the cell count and one more; the reservation is too shallow for
    /// the I/O pads to stand off the cell row; or a sink has no route
    /// from its driver that runs through neither a component nor
    /// another net's dust. §14.5 names area shortage as the example
    /// rather than as the only shape, so all four take this code and
    /// differ in what they say: raising `void` fixes the first and the
    /// last, and cannot fix the two in between.
    RouteCongestion,
    /// A routed driver segment (source pad or driver cell → sink coord,
    /// where the sink is either a downstream cell coord or an actuator
    /// output-pad coord, measured along the routed path rather than as
    /// the straight-line distance between its ends) exceeds the v1
    /// sanity cap
    /// for implicit buffer-repeater insertion. `spec/redstone` §14.5
    /// stage 3 lets segments longer than the 15-block dust attenuation
    /// limit be covered by buffer repeaters silently; this code fires
    /// only when the segment is so long that the buffer chain
    /// materialising it would be longer than the cap
    /// [`crate::delay::MAX_ATTENUATION_SEGMENT`] sets, so the pass
    /// refuses instead of quietly counting an unrealisable chain into
    /// `delay_ticks`. Fires on both driver-to-cell and driver-to-
    /// output-pad segments — a wide `circuit region=` reservation can
    /// trip either edge depending on which side sits farther from the
    /// driver. Fix: enlarge the `circuit region=` footprint so no
    /// driver segment exceeds the cap, split the logic across multiple
    /// `circuit` blocks, or pin cell / actuator placement closer to
    /// its drivers.
    AttenuationLimit,
    /// Lowering a `logic` binding descended past
    /// [`crate::synth::MAX_LOWERING_DEPTH`]. A binding is lowered by descending into
    /// whatever it references, so a chain declared in the reverse of its
    /// dependency order costs one level per binding. Past the limit the
    /// native stack would overflow, which aborts the process instead of
    /// producing a diagnostic. Fix: declare the chain in dependency order —
    /// the same graph written that way lowers at any length, because each
    /// reference is already resolved when it is reached.
    LogicNestingTooDeep,
    /// A signal binding is written where nothing reads it — on a member
    /// whose kind cannot host it, or inside the `[selector]` on a line
    /// whose binding belongs after the brackets.
    ///
    /// The first is a `-> value` sensor tail, or one of the actuator
    /// argument keys `spec/redstone` §14.2 lists, on the wrong
    /// component. Asked before the value is looked at: no edit to the
    /// value makes `walls` carry a tail, so reporting the value first
    /// would send the author round the loop.
    ///
    /// The second is `door[id=front,opened_by=sig.x]`. The brackets pick
    /// a member that already exists and the binding is written after
    /// them, which is the shape §14.2 uses;
    /// `block_array::recognize_actuator_patch` refuses any selector
    /// attribute but `id=` for the door patch, and this is the same
    /// answer for every host. §14.2 pairs each binding with the
    /// component that carries it — `opened_by=` with `door`, `lit_by=`
    /// with `lamp`, `powered_by=` with `piston`, `fired_by=` with
    /// `dispenser`, and the sensor tail with a sensor — and the front end
    /// used to read the argument's *value* only, so `walls powered_by=`
    /// and `window -> sig.x` both became live ports on members with no
    /// component behind them. Of the components §14.2 names, only `door`
    /// and `pressure_plate` are keywords the surface accepts today;
    /// `lever`, `button`, `daylight`, `observer`, `lamp`, `piston`, and
    /// `dispenser` are not, so the three actuator keys other than
    /// `opened_by=` have no legal host at all yet. Fix: move the binding
    /// onto the component that carries it.
    LogicMisplacedBinding,
    /// A position that has to name a signal does not. Sensors emit into
    /// the `sig.` namespace and actuators consume from it, so a name
    /// outside it can never be read, and three positions carry one:
    ///
    /// - a `logic` line's left-hand side, which was lowered anyway, so a
    ///   cell took a placement coordinate for a signal with no consumer;
    /// - a sensor's `-> value` tail;
    /// - the value under an actuator key on the component that reads it.
    ///
    /// The second and third used to be recognised by their value, so a
    /// value that named no signal entered no branch and the binding was
    /// dropped in silence — the component reached placement wired to
    /// nothing. Fix: name the signal `sig.<name>`. Where the value is a
    /// bare identifier the message offers the spelling, that being the
    /// one shape with a single reading; `opened_by=3` names nothing that
    /// adding `sig.` would repair.
    LogicInvalidSignal,
    /// An argument whose value is a `sig.`-headed reference sits under a
    /// key that is not one of §14.2's actuator keys. The value says the
    /// author meant to wire a signal; the key means nothing reads it, so
    /// the actuator silently disappears and only the now-unconsumed
    /// signal is mentioned. A typo (`oepend_by=`) gets a `did you mean`
    /// note; a key from another vocabulary entirely gets the list. Fix:
    /// correct the key, or drop the argument if the member is not an
    /// actuator.
    ///
    /// Deliberately keyed on the value rather than on a per-keyword
    /// argument schema — no such schema exists yet, and this closes the
    /// silent-actuator class without one, because no legal non-actuator
    /// argument inside a member's `intent_state` takes a `sig.` value.
    /// The two shapes this once said nothing about are both answered
    /// now, by their own codes. An actuator key holding a *non*-`sig.`
    /// value is [`Self::LogicInvalidSignal`] *on the component that key
    /// is paired with* — anywhere else the host fault wins and it is
    /// [`Self::LogicMisplacedBinding`], and on a keyword the role table
    /// does not know it is `E_UNKNOWN_KEYWORD` and nothing more. A pair
    /// inside a `[...]` selector is answered the same three ways, by
    /// whichever fault moving it out of the brackets would not fix.
    LogicUnknownBindingKey,
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
            Self::LogicUnsupportedPrimitive => "E_LOGIC_UNSUPPORTED_PRIMITIVE",
            Self::LogicUnusedSignal => "W_LOGIC_UNUSED_SIGNAL",
            Self::NoCircuitRegion => "E_NO_CIRCUIT_REGION",
            Self::RouteCongestion => "E_ROUTE_CONGESTION",
            Self::AttenuationLimit => "E_ATTENUATION_LIMIT",
            Self::LogicNestingTooDeep => "E_LOGIC_NESTING_TOO_DEEP",
            Self::LogicMisplacedBinding => "E_LOGIC_MISPLACED_BINDING",
            Self::LogicInvalidSignal => "E_LOGIC_INVALID_SIGNAL",
            Self::LogicUnknownBindingKey => "E_LOGIC_UNKNOWN_BINDING_KEY",
        }
    }

    /// Severity of this code. Errors participate in the `cairn synth`
    /// exit code; warnings do not.
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
            | Self::LogicUnknownBindingKey => Severity::Error,
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
/// `note: ...` line under the primary finding, and `span` is `None` when
/// the note is a footer such as "valid alternatives: sig.step" that does
/// not point at a distinct byte range.
///
/// `cairn_lang_core`'s type, re-exported rather than declared again. A
/// note is the same thing on both sides — a message and an optional span,
/// with the span skipped by `Serialize` — and the copy that used to live
/// here bought nothing but a second type for every consumer that renders
/// both crates' findings to write an adapter for. The redstone
/// [`Diagnostic`] stays its own type because its `code` is.
pub use cairn_lang_core::check::DiagnosticNote;

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
    /// Severity of this finding, read from [`DiagnosticCode::severity`].
    ///
    /// A method rather than a field, matching `cairn_lang_core`'s
    /// [`Diagnostic`](cairn_lang_core::Diagnostic): the two cannot
    /// disagree if only one of them exists.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// Build a [`Diagnostic`] from a code plus the primary span and
    /// message. The most common construction path — used by every
    /// non-cycle site in the synth pass.
    #[must_use]
    pub fn new(code: DiagnosticCode, span: Span, primary: String) -> Self {
        Self {
            code,
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

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::{DiagnosticCode, Severity};

    /// The prefix is the contract downstream tooling filters on: a CI
    /// annotator that treats `E_` as failing and `W_` as advisory reads the
    /// string, not `severity()`. Nothing tied the two together, so a new
    /// code could ship announcing the opposite of what it is — and the
    /// walk is over every variant, not a hand-written list, so the one
    /// added last cannot be the one left out.
    #[test]
    fn the_code_prefix_agrees_with_the_severity() {
        for code in DiagnosticCode::iter() {
            let expected = match code.severity() {
                Severity::Error => "E_",
                Severity::Warning => "W_",
            };
            assert!(
                code.as_str().starts_with(expected),
                "{code:?} is {:?} but its string form is {}",
                code.severity(),
                code.as_str(),
            );
        }
    }

    /// Two codes sharing a string would make them indistinguishable to
    /// every consumer that matches on it.
    #[test]
    fn every_code_has_its_own_string() {
        let mut seen: Vec<&str> = DiagnosticCode::iter().map(DiagnosticCode::as_str).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "two variants share a string form");
    }
}
