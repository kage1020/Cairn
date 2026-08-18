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
    /// `circuit` blocks"). Two shapes reach it — the reserved volume is
    /// short of the netlist's estimated footprint, or the reserved row
    /// is shorter than the cell count the v1 single-row layout needs.
    /// §14.5 names area shortage as the example rather than as the only
    /// shape, so both take this code and differ in what they say:
    /// raising `void` fixes the first and cannot fix the second.
    RouteCongestion,
    /// A routed driver segment (source pad or driver cell → sink coord,
    /// where the sink is either a downstream cell coord or an actuator
    /// output-pad coord, measured along the routed path rather than as
    /// the straight-line distance between its ends) exceeds the v1
    /// sanity cap
    /// for implicit buffer-repeater insertion. `spec/redstone` §14.5
    /// stage 3 lets segments longer than the 15-block dust attenuation
    /// limit be covered by buffer repeaters silently; this code fires
    /// only when the segment is so long that materialising it needs a
    /// stage-4 crossing-legalization escape (`RouteLayer::Bridge` /
    /// `Via`) that v1 does not implement, so the pass refuses instead
    /// of quietly counting an unrealisable buffer chain into
    /// `delay_ticks`. Fires on both driver-to-cell and driver-to-
    /// output-pad segments — a wide `circuit region=` reservation can
    /// trip either edge depending on which side sits farther from the
    /// driver. Fix: enlarge the `circuit region=` footprint so no
    /// driver segment exceeds the cap, split the logic across multiple
    /// `circuit` blocks, or pin cell / actuator placement closer to
    /// its drivers.
    AttenuationLimit,
    /// The crossing-legalization pass found a cross-net plane overlap
    /// and no layer to escape it to. `spec/redstone` §14.5 stage 4
    /// lifts a wire onto a bridge coord whenever two nets would
    /// otherwise share a `Plane` coord; the escape layer draws from the
    /// same `void=<N>` service-layer height the placement / routing
    /// passes already consume, and a bridge needs at least `y = 1`,
    /// which needs `void >= 2`. So the v1 test is whether that layer
    /// exists, not how many crossings it would have to carry: any
    /// crossing under `void < 2` fires this, and `void >= 2` accepts
    /// them all. There is nothing downstream for a per-crossing
    /// capacity model to constrain yet — v1 does not lift the wire
    /// itself, and no pass downstream reads the crossing set: the
    /// block-array lowering does not take the Placement IR at all, so
    /// whichever pass eventually voxelises these wires will derive the
    /// crossings itself. Fix: increase `void`, enlarge the
    /// `circuit region=` footprint so fewer wires cross, or split the
    /// logic across multiple `circuit` blocks so each block routes
    /// with fewer overlaps.
    CrossingCongestion,
    /// The crossing-legalization pass could not place a required
    /// implicit buffer repeater — every candidate coord along the
    /// driver segment already carries a cell, pad, or another buffer,
    /// and no `Bridge` layer coord was free either. Rare in practice:
    /// the delay pass already caps segments at
    /// `MAX_ATTENUATION_SEGMENT` (16 buffers) and the routing pass
    /// caps footprint at the reservation area, so this only fires on a
    /// pathological packing (a very tight `void=1` region with many
    /// short cascades where every plane coord is a cell). Fix:
    /// increase `void` so buffers can fall onto a bridge layer, or
    /// enlarge the `circuit region=` footprint so buffer candidates
    /// have room on the plane.
    BufferCoordCollision,
    /// Lowering a `logic` binding descended past
    /// [`crate::synth::MAX_LOWERING_DEPTH`]. A binding is lowered by descending into
    /// whatever it references, so a chain declared in the reverse of its
    /// dependency order costs one level per binding. Past the limit the
    /// native stack would overflow, which aborts the process instead of
    /// producing a diagnostic. Fix: declare the chain in dependency order —
    /// the same graph written that way lowers at any length, because each
    /// reference is already resolved when it is reached.
    LogicNestingTooDeep,
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
            Self::CrossingCongestion => "E_CROSSING_CONGESTION",
            Self::BufferCoordCollision => "E_BUFFER_COORD_COLLISION",
            Self::LogicNestingTooDeep => "E_LOGIC_NESTING_TOO_DEEP",
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
            | Self::CrossingCongestion
            | Self::BufferCoordCollision
            | Self::LogicNestingTooDeep => Severity::Error,
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
