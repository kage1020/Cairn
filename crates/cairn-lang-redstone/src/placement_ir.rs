//! Placement IR — edition-tagged cells laid out inside their scope's
//! `circuit region` reservation.
//!
//! Sits one lowering step above the [Edition Netlist IR](crate::edition_netlist_ir)
//! and one below the routed block-array IR that later passes will grow
//! on top. The placement pass ([`crate::placement::compile_placement`])
//! walks every [`crate::edition_netlist_ir::ScopedEditionNetlistIrEntry`]
//! once, looks up the scope's [`cairn_lang_core::CircuitRegion`], and
//! assigns each [`crate::edition_netlist_ir::EditionCellNode`] an
//! integer [`CellCoord`] inside the reservation — the first stage of
//! the five-stage place-and-route pipeline `spec/redstone` §14.5 lays
//! out (Placement → Steiner routing → Delay insertion → Crossing
//! legalization → Edition legalization).
//!
//! Delay is not carried at the placement stage itself. Per
//! `spec/redstone` §14.4 delay is determined "for the first time in
//! the Placement IR" once wire length is known; wire length is the
//! output of the routing pass (Steiner routing, stage 2 of §14.5) and
//! is folded into `delay_ticks` by the delay-insertion pass
//! ([`crate::delay::compile_delay`], stage 3), so
//! [`PlacedCellNode::wire_length`] and [`PlacedCellNode::delay_ticks`]
//! are reserved as `Option`s that the placement pass leaves `None`.
//! Adding values in a follow-up pass is a field-write, not a schema
//! change, so downstream JSON consumers see a stable wire shape. Which
//! pass a given dump came out of is named outright by the `stage` tag
//! every cell carries ([`PlacementStage`]) rather than inferred from
//! which optional keys are present, because that inference cannot
//! separate a stage-4 dump with nothing to legalize from its stage-3
//! input.
//!
//! `circuit region=... void=N` congestion detection (`E_ROUTE_CONGESTION`)
//! and missing-reservation refusal (`E_NO_CIRCUIT_REGION`) fire here —
//! this is the first pass with a physical footprint to measure against.
//! Attenuation limits (dust segments exceeding 15 blocks) belong to the
//! routing pass and stay a follow-up.

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};

/// Which of `spec/redstone` §14.5's three pseudo-2.5D layers a
/// coordinate lives on. `Plane` is the ground layer every cell coord
/// and every un-escaped wire segment sits on; `Bridge` is the
/// horizontal escape layer the crossing-legalization pass lifts a
/// buffer repeater onto when its plane candidate is taken; `Via` is
/// the vertical tap between the two.
///
/// Cell coords are `Plane` by construction — the placement pass never
/// stamps `Bridge` / `Via` on a cell body. Buffer-repeater coords the
/// crossing-legalization pass writes may carry either escape layer;
/// wire coords themselves are not lifted onto `Bridge` in v1 (a plane
/// crossing that cannot be absorbed by the buffer-escape path is
/// refused via [`crate::DiagnosticCode::CrossingCongestion`]).
/// Serialising as the enum's stable lowercase string (`plane` /
/// `bridge` / `via`) keeps the JSON wire form small and matches the
/// vocabulary spec §14.5 uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RouteLayer {
    /// Ground layer — every cell coord and every un-escaped wire
    /// segment sits here. Default so that any code path which forgets
    /// to set the layer explicitly defaults to the ground layer rather
    /// than an escape layer.
    #[default]
    Plane,
    /// Horizontal escape layer for buffer repeaters the
    /// crossing-legalization pass lifted off the `Plane` to avoid a
    /// collision with a cell / pad / plane crossing.
    Bridge,
    /// Reserved for the vertical `Plane` ↔ `Bridge` tap. No producer
    /// materialises `Via` in v1: buffer repeaters are treated as their
    /// own `Bridge` cell without an explicit ramp coord, and wire
    /// bridges themselves are not lifted. Kept in the enum so a
    /// downstream consumer can match exhaustively against the full
    /// §14.5 vocabulary; a subsequent pass that grows `Via` producers
    /// is `#[non_exhaustive]`-safe.
    Via,
}

impl RouteLayer {
    /// `true` for the default [`Self::Plane`] layer. Used by the
    /// `serde(skip_serializing_if = "…")` attribute on
    /// [`CellCoord::layer`] so the JSON wire form of a `Plane` coord
    /// omits the `layer` field entirely — the routed / delayed IR
    /// shape is an additive subset of the legalized IR shape apart
    /// from the `stage` tag ([`PlacementStage`]).
    #[must_use]
    pub const fn is_plane(&self) -> bool {
        matches!(self, Self::Plane)
    }

    /// Stable lowercase string form used in the JSON wire format and
    /// matched by downstream tooling. Mirrors the vocabulary spec
    /// §14.5 uses when it introduces the three concepts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Bridge => "bridge",
            Self::Via => "via",
        }
    }
}

impl Serialize for RouteLayer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Coordinate inside a scope's `circuit region` reservation.
///
/// `Copy` because a coordinate is a value type consumers pass around
/// by value. Cell coords stamped by [`crate::placement::compile_placement`]
/// use `x = topological index`, `y = 0`, `z = 0`, [`RouteLayer::Plane`].
/// Pad coords derived by the routing pass at the reservation edges
/// use `y = 0` on the plane with `z = 1 + i` (saturating at `depth-1`
/// for pathological regions); buffer-repeater coords stamped by
/// [`crate::crossing::compile_crossing`] may use `y >= 1` and
/// [`RouteLayer::Bridge`] when the plane candidate is taken.
///
/// The `layer` field participates in `Eq` and `Hash`, so
/// `(x, y, z, Plane)` and `(x, y, z, Bridge)` are distinct map keys.
/// This matches the pseudo-2.5D model — a plane wire and a bridge
/// wire at the same `(x, y, z)` do not collide in the voxel view —
/// but a downstream consumer that only cares about voxel identity
/// should compare `(x, y, z)` explicitly rather than relying on the
/// derived `Eq`.
///
/// `layer` serialises only when it differs from [`RouteLayer::Plane`]
/// so a placement / routing / delay JSON dump omits the `layer` key
/// entirely: the legalized IR shape is an additive superset of the
/// earlier stages' shape apart from the `stage` tag
/// ([`PlacementStage`]), whose value changes rather than appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Row along the region's y-axis. `0` for `Plane` coords; the
    /// crossing pass may set `y >= 1` for buffer coords lifted onto a
    /// `Bridge` layer inside the reservation's `void=<N>` budget.
    pub y: u32,
    /// Row along the region's z-axis. `0` for cell coords stamped by
    /// the placement pass; pad and buffer coords may sit at
    /// `z = 1 + i` (saturating at `depth-1`).
    pub z: u32,
    /// Pseudo-2.5D layer this coord lives on. Cell coords are
    /// [`RouteLayer::Plane`] by construction; the crossing-legalization
    /// pass may stamp [`RouteLayer::Bridge`] on a buffer-repeater
    /// coord that collides with a cell / pad / plane crossing.
    /// Serialised only when it differs from the default so a
    /// `Plane` coord's JSON omits the `layer` field.
    #[serde(skip_serializing_if = "RouteLayer::is_plane")]
    pub layer: RouteLayer,
}

impl CellCoord {
    /// [`RouteLayer::Plane`] coord at the given axis position. The
    /// most common construction path — used by every placement /
    /// routing / delay call site.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        Self {
            x,
            y,
            z,
            layer: RouteLayer::Plane,
        }
    }

    /// Coord with an explicit [`RouteLayer`]. Crate-internal: only
    /// [`crate::crossing::compile_crossing`] needs to stamp a
    /// non-`Plane` layer, and pinning it to the crate keeps external
    /// consumers from building bridge / via coords the pipeline does
    /// not know how to consume.
    #[must_use]
    pub(crate) const fn with_layer(x: u32, y: u32, z: u32, layer: RouteLayer) -> Self {
        Self { x, y, z, layer }
    }
}

/// One implicit buffer repeater the crossing-legalization pass
/// materialised, tagged with the driver port on the owning cell it
/// belongs to.
///
/// The driver attribution is copied verbatim from
/// [`PlacedCellNode::drivers`]`[i].port` at push time, so a downstream
/// consumer can group buffers by their source segment without
/// recomputing `floor((s - 1) / DUST_ATTENUATION_LIMIT)` from scratch.
///
/// **Order contract on `Vec<BufferCoord>`** (as returned by
/// [`PlacedCellNode::buffer_coords`]): entries appear in
/// [`PlacedCellNode::drivers`] iteration order, and within one
/// driver's segment in the order the routed path visits them. A
/// consumer that reads the vector positionally therefore sees driver
/// 0's buffers before driver 1's, and closer-to-source buffers before
/// closer-to-sink ones. That order is *not* one x → z → y sweep: the
/// route is a chain of L-shapes between tree terminals, so it turns as
/// often as the tree does.
///
/// **Coord layer.** [`Self::coord`]`.layer` records the crossing
/// pass's placement decision — see [`RouteLayer`] for the full
/// vocabulary and which variants a producer emits today.
///
/// Serialises as `{"port": "<a|b|sel|out>", "coord": {...}}`, matching the
/// `{port, ...}` shape [`CellPortDriver`] uses on the netlist side.
/// The nested `coord` still elides its `layer` field when it stays on
/// [`RouteLayer::Plane`], so the JSON footprint of a plane buffer stays
/// compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct BufferCoord {
    /// Which segment this buffer stands on: a driver port of the owning
    /// cell, or the wire out to an actuator pad.
    pub port: BufferSegment,
    /// Coordinate the crossing pass chose for the buffer. The
    /// `layer` field records the placement decision — see
    /// [`RouteLayer`] for the vocabulary.
    pub coord: CellCoord,
}

impl BufferCoord {
    /// New `(segment, coord)` attribution pair. Mirrors the
    /// [`CellCoord::new`] constructor convention so the crossing pass
    /// and its tests build entries without struct-literal noise.
    #[must_use]
    pub const fn new(port: BufferSegment, coord: CellCoord) -> Self {
        Self { port, coord }
    }
}

/// Which wire a buffer repeater stands on.
///
/// Separate from [`PortName`] rather than a variant of it, because a
/// [`CellPortDriver`] must not be able to say `out`: a cell has no such
/// input, and public fields plus no validation is all it would take.
/// Here the two are the same kind of answer — "which segment" — so they
/// share one flat wire form: `"a"` / `"b"` / `"sel"` for a driver port,
/// `"out"` for the wire to an actuator.
///
/// `#[non_exhaustive]` for the reason [`PortName`] is: a future cell
/// with more ports, or a future segment kind, is additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BufferSegment {
    /// A driver port feeding the owning cell. Copies
    /// [`CellPortDriver::port`] from the driver the crossing pass was
    /// walking when it emitted the entry.
    Port(PortName),
    /// The wire leaving a driver for an actuator's output pad. Has no
    /// owning cell: it is attributed to the [`PlacedOutputNode`] whose
    /// pad it runs to.
    Out,
}

// Hand-written so the two cases share one flat string vocabulary
// instead of the externally-tagged shape a derive would give `Port(..)`.
// The wire form is the contract `BufferCoord`'s doc states.
impl Serialize for BufferSegment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Port(port) => port.serialize(serializer),
            Self::Out => serializer.serialize_str("out"),
        }
    }
}

/// The `circuit region=<label> void=<N>` reservation the enclosing
/// scope declared, together with the footprint that reservation sits
/// inside.
///
/// Copied verbatim onto every non-empty [`PlacementIr`] so a JSON dump
/// self-describes and downstream routing does not re-derive the
/// budget. Extending this struct with a new field (region origin,
/// per-region attenuation budget, ...) is `#[non_exhaustive]`-safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CircuitRegionReservation {
    /// `region=<label>` value the circuit member declared.
    pub label: String,
    /// `void=<N>` service-layer height (`>= 1`).
    pub void: u32,
    /// Width of the enclosing scope's footprint, copied from `size=WxH`.
    pub width: u32,
    /// Depth of the enclosing scope's footprint, copied from `size=WxH`.
    pub depth: u32,
    /// Byte range of the originating `circuit region=...` line.
    #[serde(skip)]
    pub span: Span,
}

impl CircuitRegionReservation {
    /// Total blocks reserved for routing: `width * depth * void`. Uses
    /// `u64` so a large-but-legal reservation cannot overflow when
    /// multiplied against a large cell budget elsewhere in the pass.
    #[must_use]
    pub const fn reserved_area(&self) -> u64 {
        (self.width as u64) * (self.depth as u64) * (self.void as u64)
    }
}

/// Which place-and-route pass last wrote to a [`PlacedCellNode`],
/// projected out of its [`PlacementPhase`] and emitted as the `stage`
/// key of the JSON wire form.
///
/// Without it a JSON consumer has to infer the stage from which
/// optional keys are present, and that inference is not total:
/// [`PlacementPhase::Delayed`] and a [`PlacementPhase::Legalized`]
/// whose crossing pass materialised zero buffers carry exactly the
/// same keys, because an empty `buffer_coords` serde-skips. The tag
/// makes the two distinguishable without turning the empty vector
/// into a sentinel.
///
/// The strings are the same vocabulary `cairn synth --stage <s>`
/// accepts, so a dump names the flag that produced it and a consumer
/// can round-trip the two. Keep them in step with the CLI's
/// `SynthStage` value names — the CLI test suite asserts the
/// equality per stage.
///
/// `#[non_exhaustive]` for the same reason [`PlacementPhase`] is: the
/// fifth stage (Edition legalization) has no variant yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlacementStage {
    /// Produced by [`crate::placement::compile_placement`] — paired
    /// with [`PlacementPhase::Unrouted`].
    Placement,
    /// Produced by [`crate::routing::compile_routing`] — paired with
    /// [`PlacementPhase::Routed`].
    Route,
    /// Produced by [`crate::delay::compile_delay`] — paired with
    /// [`PlacementPhase::Delayed`].
    Delay,
    /// Produced by [`crate::crossing::compile_crossing`] — paired with
    /// [`PlacementPhase::Legalized`], whether or not that pass had any
    /// buffer to materialise.
    Crossing,
}

impl PlacementStage {
    /// Stable lowercase string form used in the JSON wire format and
    /// accepted by `cairn synth --stage <s>`. Mirrors
    /// [`RouteLayer::as_str`], which fixes the other wire-level enum in
    /// this module to one authoritative spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Route => "route",
            Self::Delay => "delay",
            Self::Crossing => "crossing",
        }
    }
}

impl Serialize for PlacementStage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Progressive state of a [`PlacedCellNode`] as it moves through the
/// first four of the five stages of the place-and-route pipeline
/// (`spec/redstone` §14.5 — Placement → Steiner routing → Delay
/// insertion → Crossing legalization → Edition legalization; the
/// fifth stage is future work and does not yet have a variant).
///
/// Every legal `(wire_length, delay_ticks, buffer_coords)` combination
/// the pipeline can produce is one of these variants:
///
/// | Producer                                             | Variant                                                                     | `stage` tag   |
/// |------------------------------------------------------|-----------------------------------------------------------------------------|---------------|
/// | [`crate::placement::compile_placement`] (Stage 1)    | [`Self::Unrouted`]                                                          | `placement`   |
/// | [`crate::routing::compile_routing`]     (Stage 2)    | [`Self::Routed`] `{ wire_length }`                                          | `route`       |
/// | [`crate::delay::compile_delay`]         (Stage 3)    | [`Self::Delayed`] `{ wire_length, delay_ticks }`                            | `delay`       |
/// | [`crate::crossing::compile_crossing`]   (Stage 4)    | [`Self::Legalized`] `{ wire_length, delay_ticks, buffer_coords }`           | `crossing`    |
///
/// The rightmost column is [`Self::stage`] — see [`PlacementStage`]
/// for why the JSON dump names its stage outright instead of leaving
/// a consumer to infer it from which optional keys are present.
///
/// Illegal shapes such as "have `delay_ticks` but no `wire_length`" or
/// "carry `buffer_coords` before the delay pass has run" are
/// unrepresentable — each transition is expressed by the mutation
/// methods [`Self::route`], [`Self::delay`], and [`Self::legalize`],
/// which pattern-match the current variant and panic on any out-of-order
/// call. Each has a fallible mirror ([`Self::try_route`],
/// [`Self::try_delay`], [`Self::try_legalize`]) that returns
/// [`PlacementPhaseTransitionError`] instead, for the callers to whom
/// an out-of-order transition is recoverable input rather than a bug.
/// `buffer_coords` on [`Self::Legalized`] is allowed to be empty:
/// the crossing pass materialises one entry per implicit buffer the
/// delay pass counted, and a scope whose delay pass counted zero
/// buffers is still legalized (transitions to [`Self::Legalized`] with
/// an empty vector).
///
/// `#[non_exhaustive]` so a future Stage-5 `EditionLegalized` variant
/// is additive: downstream `match` sites in other crates must carry a
/// `_ => …` arm today.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhase {
    /// Fresh out of the placement pass — no wire, no delay, no buffers.
    Unrouted,
    /// After Steiner routing: wire length is known, delay and buffers
    /// are not yet.
    Routed {
        /// Manhattan wire length from every driver into this cell, per
        /// [`crate::routing::compile_routing`].
        wire_length: u32,
    },
    /// After delay insertion: delay ticks have been folded in over the
    /// routed wire length, per [`crate::delay::compile_delay`].
    Delayed {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// Tick count implied by the routed wire length + this cell's
        /// physical realisation per `spec/redstone` §14.4.
        delay_ticks: u32,
    },
    /// After crossing legalization: buffer coordinates for the implicit
    /// repeaters the delay pass counted have been materialised, per
    /// [`crate::crossing::compile_crossing`].
    Legalized {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// Preserved from [`Self::Delayed`].
        delay_ticks: u32,
        /// One entry per implicit buffer repeater the delay pass
        /// counted; the crossing pass populates each entry with the
        /// coord it chose and the driver port that buffer belongs to.
        /// Empty when the delay pass counted zero buffers.
        ///
        /// See [`BufferCoord`] for the per-entry shape, ordering
        /// contract, and layer semantics.
        buffer_coords: Vec<BufferCoord>,
    },
}

impl PlacementPhase {
    /// Wire length once routing has run, `None` otherwise.
    #[must_use]
    pub const fn wire_length(&self) -> Option<u32> {
        match self {
            Self::Unrouted => None,
            Self::Routed { wire_length }
            | Self::Delayed { wire_length, .. }
            | Self::Legalized { wire_length, .. } => Some(*wire_length),
        }
    }

    /// Delay ticks once delay insertion has run, `None` otherwise.
    #[must_use]
    pub const fn delay_ticks(&self) -> Option<u32> {
        match self {
            Self::Unrouted | Self::Routed { .. } => None,
            Self::Delayed { delay_ticks, .. } | Self::Legalized { delay_ticks, .. } => {
                Some(*delay_ticks)
            }
        }
    }

    /// Buffer coordinates once crossing legalization has run, an empty
    /// slice otherwise. Legalized-with-zero-buffers also returns an
    /// empty slice so callers can treat "nothing to place" and "not
    /// yet legalized" identically for read-only purposes; use the
    /// enum discriminant if the distinction matters.
    ///
    /// All variants are matched explicitly rather than via `_ => &[]`
    /// so that adding a Stage-5 variant becomes a compile error here
    /// instead of a silent empty-slice return that would drop the new
    /// stage's data from the JSON dump.
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        match self {
            Self::Unrouted | Self::Routed { .. } | Self::Delayed { .. } => &[],
            Self::Legalized { buffer_coords, .. } => buffer_coords,
        }
    }

    /// Which pass last wrote to this cell — the discriminant projected
    /// onto the [`PlacementStage`] the JSON wire form carries. Unlike
    /// the three value accessors above this one is total: every phase
    /// belongs to exactly one stage, including a [`Self::Legalized`]
    /// that has no buffer coords to show for itself.
    ///
    /// All variants are matched explicitly rather than via a `_ =>`
    /// catch-all so that adding a Stage-5 variant becomes a compile
    /// error here instead of a silent mislabelling of the new stage's
    /// dumps as stage 4.
    #[must_use]
    pub const fn stage(&self) -> PlacementStage {
        match self {
            Self::Unrouted => PlacementStage::Placement,
            Self::Routed { .. } => PlacementStage::Route,
            Self::Delayed { .. } => PlacementStage::Delay,
            Self::Legalized { .. } => PlacementStage::Crossing,
        }
    }

    /// [`Self::Unrouted`] → [`Self::Routed`].
    ///
    /// Carries no caller context, so an out-of-order call names only
    /// the phase it tripped on. Prefer [`Self::route_at`] from a
    /// pipeline pass, which names the offending cell as well.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Unrouted`]. Routing must run
    /// exactly once per placement, and the producer↔variant table on
    /// this enum forbids re-routing. Use [`Self::try_route`] where an
    /// out-of-order call is a recoverable input condition rather than
    /// a caller-side bug.
    #[track_caller]
    pub fn route(&mut self, wire_length: u32) {
        self.route_inner(wire_length, None);
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], naming `context` in the
    /// panic an out-of-order call raises.
    ///
    /// `#[track_caller]` alone puts the calling `.rs:line` in the
    /// backtrace but says nothing about *which* cell tripped the
    /// guard, leaving the operator to walk back from the backtrace
    /// into the IR. Pipeline passes pass a [`CellIdentity`]; any
    /// [`fmt::Display`] works.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::route`] does.
    #[track_caller]
    pub fn route_at(&mut self, wire_length: u32, context: impl fmt::Display) {
        self.route_inner(wire_length, Some(&context));
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], refusing an out-of-order
    /// call with an [`Err`] instead of a panic.
    ///
    /// [`Self::route`] is the form the pipeline passes use: a
    /// wrong-order transition in a fresh compile is a caller-side bug
    /// with no recovery path, and panicking is what surfaces it. This
    /// form is for the consumers that do have one — a cache validator
    /// that rebuilds from scratch, an IR ingest that refuses a
    /// malformed dump with a diagnostic, a language server that cannot
    /// take a process down over one bad call.
    ///
    /// A refused call leaves the phase untouched, so a caller that
    /// recovers can go on using it.
    ///
    /// All non-`Unrouted` variants are listed explicitly (rather than a
    /// `_ =>` catch-all) so a Stage-5 variant addition becomes a
    /// compile error here — the author must decide whether Stage 5 can
    /// be re-routed rather than silently refusing.
    ///
    /// # Errors
    ///
    /// Returns [`PlacementPhaseTransitionError::RouteOnNonUnrouted`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Unrouted`].
    pub fn try_route(&mut self, wire_length: u32) -> Result<(), PlacementPhaseTransitionError> {
        match self {
            Self::Unrouted => {
                *self = Self::Routed { wire_length };
                Ok(())
            }
            Self::Routed { .. } | Self::Delayed { .. } | Self::Legalized { .. } => {
                Err(PlacementPhaseTransitionError::RouteOnNonUnrouted {
                    current: self.clone(),
                })
            }
        }
    }

    /// The panicking forms are [`Self::try_route`] plus a panic, so the
    /// guard itself is stated once and the two forms cannot disagree
    /// about which transitions are legal.
    #[track_caller]
    fn route_inner(&mut self, wire_length: u32, context: Option<&dyn fmt::Display>) {
        if let Err(error) = self.try_route(wire_length) {
            transition_panic(&error, context);
        }
    }

    /// [`Self::Routed`] → [`Self::Delayed`].
    ///
    /// Carries no caller context — see [`Self::delay_at`] for the form
    /// a pipeline pass should use.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Routed`]. Delay insertion
    /// must run exactly once per routed IR, and the producer↔variant
    /// table on this enum forbids re-writing a `delay_ticks` that was
    /// already committed. See [`Self::try_delay`] for the fallible
    /// mirror.
    #[track_caller]
    pub fn delay(&mut self, delay_ticks: u32) {
        self.delay_inner(delay_ticks, None);
    }

    /// [`Self::Routed`] → [`Self::Delayed`], naming `context` in the
    /// panic an out-of-order call raises. See [`Self::route_at`] for
    /// why the context is worth carrying.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::delay`] does.
    #[track_caller]
    pub fn delay_at(&mut self, delay_ticks: u32, context: impl fmt::Display) {
        self.delay_inner(delay_ticks, Some(&context));
    }

    /// [`Self::Routed`] → [`Self::Delayed`], refusing an out-of-order
    /// call with an [`Err`] instead of a panic. See [`Self::try_route`]
    /// for which callers want this form and why the arms are
    /// enumerated explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`PlacementPhaseTransitionError::DelayOnNonRouted`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Routed`].
    pub fn try_delay(&mut self, delay_ticks: u32) -> Result<(), PlacementPhaseTransitionError> {
        let wire_length = match self {
            Self::Routed { wire_length } => *wire_length,
            Self::Unrouted | Self::Delayed { .. } | Self::Legalized { .. } => {
                return Err(PlacementPhaseTransitionError::DelayOnNonRouted {
                    current: self.clone(),
                });
            }
        };
        *self = Self::Delayed {
            wire_length,
            delay_ticks,
        };
        Ok(())
    }

    /// See [`Self::route_inner`] for why the panicking forms delegate.
    #[track_caller]
    fn delay_inner(&mut self, delay_ticks: u32, context: Option<&dyn fmt::Display>) {
        if let Err(error) = self.try_delay(delay_ticks) {
            transition_panic(&error, context);
        }
    }

    /// [`Self::Delayed`] → [`Self::Legalized`].
    ///
    /// Carries no caller context — see [`Self::legalize_at`] for the
    /// form a pipeline pass should use.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Delayed`]. Crossing
    /// legalization must run exactly once per delayed IR, so a caller
    /// who chained `compile_crossing(&legalized.scoped)` trips here
    /// rather than silently producing a stale-but-plausible IR, and one
    /// who reached for it before delay insertion ran trips here too.
    /// See [`Self::try_legalize`] for the fallible mirror.
    #[track_caller]
    pub fn legalize(&mut self, buffer_coords: Vec<BufferCoord>) {
        self.legalize_inner(buffer_coords, None);
    }

    /// [`Self::Delayed`] → [`Self::Legalized`], naming `context` in the
    /// panic an out-of-order call raises. See [`Self::route_at`] for
    /// why the context is worth carrying.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::legalize`] does.
    #[track_caller]
    pub fn legalize_at(&mut self, buffer_coords: Vec<BufferCoord>, context: impl fmt::Display) {
        self.legalize_inner(buffer_coords, Some(&context));
    }

    /// [`Self::Delayed`] → [`Self::Legalized`], refusing an
    /// out-of-order call with an [`Err`] instead of a panic. See
    /// [`Self::try_route`] for which callers want this form and why the
    /// arms are enumerated explicitly.
    ///
    /// A refused call drops `buffer_coords` unwritten and leaves the
    /// phase exactly as it was found. The vector is taken by value, so
    /// a caller that needs it after a refusal has to clone it before
    /// calling.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`PlacementPhaseTransitionError::LegalizeOnNonDelayed`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Delayed`].
    pub fn try_legalize(
        &mut self,
        buffer_coords: Vec<BufferCoord>,
    ) -> Result<(), PlacementPhaseTransitionError> {
        let (wire_length, delay_ticks) = match self {
            Self::Delayed {
                wire_length,
                delay_ticks,
            } => (*wire_length, *delay_ticks),
            Self::Unrouted | Self::Routed { .. } | Self::Legalized { .. } => {
                return Err(PlacementPhaseTransitionError::LegalizeOnNonDelayed {
                    current: self.clone(),
                });
            }
        };
        *self = Self::Legalized {
            wire_length,
            delay_ticks,
            buffer_coords,
        };
        Ok(())
    }

    /// See [`Self::route_inner`] for why the panicking forms delegate.
    #[track_caller]
    fn legalize_inner(
        &mut self,
        buffer_coords: Vec<BufferCoord>,
        context: Option<&dyn fmt::Display>,
    ) {
        if let Err(error) = self.try_legalize(buffer_coords) {
            transition_panic(&error, context);
        }
    }
}

/// Why a [`PlacementPhase`] transition refused: the transition that
/// was attempted, plus the phase it found instead of the one it
/// consumes.
///
/// Returned by [`PlacementPhase::try_route`] /
/// [`PlacementPhase::try_delay`] / [`PlacementPhase::try_legalize`],
/// and the same value the panicking forms render into their payload —
/// so a recovered refusal and a release-loud one read alike.
///
/// The whole offending phase is carried rather than just its variant
/// name: a consumer diagnosing a stale cache entry or a malformed IR
/// dump wants the `wire_length` / `delay_ticks` the phase got as far
/// as, and it is what lets [`fmt::Display`] reproduce the panic
/// wording exactly.
///
/// `#[non_exhaustive]` for the same reason [`PlacementPhase`] is: a
/// Stage-5 transition brings a fourth refusal with it, and that must
/// be an additive change for downstream `match` sites.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhaseTransitionError {
    /// [`PlacementPhase::try_route`] was called on a phase that had
    /// already been routed, delayed, or legalized.
    RouteOnNonUnrouted {
        /// The phase found instead of [`PlacementPhase::Unrouted`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_delay`] was called on a phase that had not
    /// been routed yet, or had already been delayed or legalized.
    DelayOnNonRouted {
        /// The phase found instead of [`PlacementPhase::Routed`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_legalize`] was called on a phase that had
    /// not been delayed yet, or had already been legalized.
    LegalizeOnNonDelayed {
        /// The phase found instead of [`PlacementPhase::Delayed`].
        current: PlacementPhase,
    },
}

impl PlacementPhaseTransitionError {
    /// The phase the refused transition found.
    ///
    /// Total across every variant, so a consumer can read the offending
    /// phase without matching — which under `#[non_exhaustive]` would
    /// otherwise cost it a `_ =>` arm.
    #[must_use]
    pub const fn current(&self) -> &PlacementPhase {
        match self {
            Self::RouteOnNonUnrouted { current }
            | Self::DelayOnNonRouted { current }
            | Self::LegalizeOnNonDelayed { current } => current,
        }
    }

    /// Render this refusal with a caller-supplied identity clause
    /// spliced in, in the same place and format
    /// [`PlacementPhase::route_at`] and friends put theirs.
    ///
    /// A consumer that recovered from the refusal still knows what it
    /// was working on — the index of the cell in the dump it is
    /// ingesting, the path of the cache entry it is validating, the
    /// document position it is reporting on — and this is how it says
    /// so without hand-assembling a message that would drift from the
    /// panicking forms'. Any [`fmt::Display`] works, and it is taken by
    /// value like the context the `*_at` transitions take.
    #[must_use]
    pub fn with_context<'a, C: fmt::Display + 'a>(&'a self, context: C) -> impl fmt::Display + 'a {
        TransitionMessage {
            error: self,
            context: Some(context),
        }
    }

    /// The transition method that refused, as it is spelled in the
    /// message.
    const fn method(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "route",
            Self::DelayOnNonRouted { .. } => "delay",
            Self::LegalizeOnNonDelayed { .. } => "legalize",
        }
    }

    /// The pipeline pass that transition belongs to.
    const fn pass(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "routing",
            Self::DelayOnNonRouted { .. } => "delay insertion",
            Self::LegalizeOnNonDelayed { .. } => "crossing legalization",
        }
    }

    /// What that pass consumes exactly one of.
    const fn source(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "placement",
            Self::DelayOnNonRouted { .. } => "routed IR",
            Self::LegalizeOnNonDelayed { .. } => "delayed IR",
        }
    }
}

impl fmt::Display for PlacementPhaseTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        TransitionMessage::<&dyn fmt::Display> {
            error: self,
            context: None,
        }
        .fmt(f)
    }
}

impl std::error::Error for PlacementPhaseTransitionError {}

/// The cardinality clause every refused [`PlacementPhase`] transition
/// carries, between the pass that tripped the guard and the phase that
/// pass consumes.
///
/// Spelled once so a transition added beside `route` / `delay` /
/// `legalize` cannot invent its own strength, which holds only as long
/// as every transition reports through [`TransitionMessage`] rather
/// than writing its own message.
///
/// "Exactly" rather than "at most": each transition accepts one source
/// variant and refuses every other, so a phase that never reached that
/// source trips the same guard a re-run does. A skipped stage is as
/// much a bug as a repeated one, and only "exactly once" says so.
const TRANSITION_CARDINALITY: &str = "must run exactly once per";

/// The one rendering of a [`PlacementPhaseTransitionError`], with the
/// identity clause optional.
///
/// Every message this module produces — the `Display` of the error, the
/// `with_context` form, and both shapes of transition panic — goes
/// through here, so the fallible and panicking forms cannot drift
/// apart. An absent context drops the whole ` for {context}` clause
/// rather than rendering an empty one, so no stray separator or double
/// space reaches the message.
///
/// The context is a type parameter rather than a `&dyn fmt::Display` so
/// that the public [`PlacementPhaseTransitionError::with_context`] can
/// own the value its caller passes, matching how the `*_at` transitions
/// take theirs. The panic path instantiates it at `&dyn fmt::Display`,
/// which is what it already holds.
struct TransitionMessage<'a, C: fmt::Display> {
    error: &'a PlacementPhaseTransitionError,
    context: Option<C>,
}

impl<C: fmt::Display> fmt::Display for TransitionMessage<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PlacementPhase::{method} called on {current:?}",
            method = self.error.method(),
            current = self.error.current(),
        )?;
        if let Some(context) = &self.context {
            write!(f, " for {context}")?;
        }
        write!(
            f,
            " — {pass} {TRANSITION_CARDINALITY} {source}",
            pass = self.error.pass(),
            source = self.error.source(),
        )
    }
}

/// Raise the release-loud panic an out-of-order [`PlacementPhase`]
/// transition owes its caller.
///
/// The panicking transition forms are their `try_*` mirror plus this
/// call, so the message is assembled from the same error value a
/// recovering caller would have received.
///
/// `#[track_caller]` on every layer between the pass and here means
/// the reported location is still the pipeline pass's `.rs:line`, not
/// this function's.
#[track_caller]
fn transition_panic(
    error: &PlacementPhaseTransitionError,
    context: Option<&dyn fmt::Display>,
) -> ! {
    panic!("{}", TransitionMessage { error, context });
}

/// Identity breadcrumb a pipeline pass hands to
/// [`PlacementPhase::route_at`] / [`PlacementPhase::delay_at`] /
/// [`PlacementPhase::legalize_at`] so an out-of-order transition panic
/// names the cell that tripped it.
///
/// [`PlacedCellNode`] carries no source-level name, so a cell's only
/// stable identity is its position in [`PlacementIr::cells`], the
/// coord the placement pass stamped on it, and the scope that owns it.
/// Rendered in the same vocabulary the pass diagnostics already use
/// (`cell #{index}`, `({x},{y},{z})`, ``{kind} `{name}` ``) so a panic
/// and an `E_*` diagnostic about the same cell read alike.
///
/// [`CellCoord::layer`] renders only when it is not
/// [`RouteLayer::Plane`], which for a cell coord is never: the
/// placement pass stamps `Plane` and no later pass moves a cell body
/// off it — only the buffer coords the crossing pass allocates can be
/// lifted onto a `Bridge` layer. Suppressing the default rather than
/// dropping the field outright keeps the common rendering short
/// without letting a hand-built IR that breaks the invariant print a
/// coord that silently reads as a plane coord. Enforcing the
/// invariant with an assertion here instead was rejected: this type
/// is built on the happy path for every cell of every pass, and a
/// breadcrumb whose whole purpose is to improve a panic must not be
/// able to raise one of its own.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellIdentity<'a> {
    index: usize,
    coord: CellCoord,
    subject: PlacedSubject,
    scope: &'a ScopedPlacementIrEntry,
}

/// Which list the index in a [`CellIdentity`] indexes into. A panic
/// that says "cell #0" about an actuator pad sends the reader to the
/// wrong line of the source, and both lists start at zero.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PlacedSubject {
    /// An entry of [`PlacementIr::cells`].
    Cell,
    /// An entry of [`PlacementIr::outputs`].
    Output,
}

impl PlacedSubject {
    const fn label(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::Output => "output",
        }
    }
}

impl<'a> CellIdentity<'a> {
    /// Breadcrumb for the cell at position `index` of the scope being
    /// transitioned, whose placement coord is `coord`.
    ///
    /// `coord` is passed in rather than read back out of
    /// `scope.ir.cells[index]` because a pass transitions the cells of
    /// its own working clone of the IR, not the entry's: taking the
    /// coord from the very cell being transitioned cannot go stale or
    /// index out of bounds, whereas reaching back into `scope.ir`
    /// could do both if a pass ever adds or drops a cell.
    pub(crate) const fn new(
        index: usize,
        coord: CellCoord,
        scope: &'a ScopedPlacementIrEntry,
    ) -> Self {
        Self {
            index,
            coord,
            subject: PlacedSubject::Cell,
            scope,
        }
    }

    /// Breadcrumb for the actuator pad at position `index`, whose
    /// placement coord is `pad`.
    pub(crate) const fn output(
        index: usize,
        pad: CellCoord,
        scope: &'a ScopedPlacementIrEntry,
    ) -> Self {
        Self {
            index,
            coord: pad,
            subject: PlacedSubject::Output,
            scope,
        }
    }
}

impl fmt::Display for CellIdentity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{subject} #{index} at ({x},{y},{z}",
            subject = self.subject.label(),
            index = self.index,
            x = self.coord.x,
            y = self.coord.y,
            z = self.coord.z,
        )?;
        if !self.coord.layer.is_plane() {
            write!(f, " on {layer}", layer = self.coord.layer.as_str())?;
        }
        write!(
            f,
            ") in {kind} `{name}`",
            kind = self.scope.kind.label(),
            name = self.scope.name,
        )
    }
}

/// One placed cell — an [`EditionCell`] plus its assigned [`CellCoord`]
/// inside the reservation.
///
/// The progressive fields the routing, delay-insertion, and
/// crossing-legalization passes produce (`wire_length`, `delay_ticks`,
/// `buffer_coords`) live inside [`Self::phase`] as a
/// [`PlacementPhase`] — see that enum's docstring for the four
/// legitimate states and the transition methods each pass calls.
/// Read-only convenience accessors [`Self::wire_length`],
/// [`Self::delay_ticks`], and [`Self::buffer_coords`] project the
/// phase back onto the three flat fields the JSON wire form exposes,
/// so consumers that only care about the values do not need to match
/// on the phase enum; [`Self::stage`] projects the discriminant
/// itself.
///
/// The custom [`Serialize`] impl flattens [`Self::phase`] onto
/// `{stage, cell, drivers, coord[, wire_length][, delay_ticks][, buffer_coords]}`.
/// The three optionals carry the shape earlier revisions of this
/// struct produced via `skip_serializing_if`, so the JSON dump of a
/// stage-N output is an additive subset of the stage-(N+1) dump *apart
/// from the `stage` tag* on scopes whose stage-(N+1) pass had nothing
/// to write. The tag is the one field whose value changes rather than
/// appears, and it exists precisely because the subset relation made
/// a zero-buffer [`PlacementPhase::Legalized`] indistinguishable from
/// a [`PlacementPhase::Delayed`] — see [`PlacementStage`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedCellNode {
    /// Edition-specific realisation chosen for this cell (copied verbatim
    /// from the source [`crate::edition_netlist_ir::EditionCellNode`]).
    pub cell: EditionCell,
    /// Port → net drivers, in canonical port order carried across from
    /// the Netlist IR (`[A, B]` for two-input gates, `[A]` for `Not`,
    /// `[Sel, A, B]` for `Mux`).
    pub drivers: Vec<CellPortDriver>,
    /// Coordinate assigned by the placement pass.
    pub coord: CellCoord,
    /// Progressive pipeline state — see [`PlacementPhase`].
    ///
    /// Crate-visible so the three pipeline passes (routing, delay,
    /// crossing) can call the phase-transition methods, but hidden from
    /// downstream crates: writes must go through
    /// [`PlacementPhase::route`] / [`PlacementPhase::delay`] /
    /// [`PlacementPhase::legalize`], and reads through the flat
    /// accessor methods on this struct. Direct field assignment would
    /// bypass the phase-transition guards and re-open the illegal
    /// states the enum exists to forbid.
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `logic ...` sub-expression, inherited
    /// from the source [`crate::edition_netlist_ir::EditionCellNode`].
    pub span: Span,
}

impl PlacedCellNode {
    /// Wire length once routing has run. See
    /// [`PlacementPhase::wire_length`].
    #[must_use]
    pub const fn wire_length(&self) -> Option<u32> {
        self.phase.wire_length()
    }

    /// Delay ticks once delay insertion has run. See
    /// [`PlacementPhase::delay_ticks`].
    #[must_use]
    pub const fn delay_ticks(&self) -> Option<u32> {
        self.phase.delay_ticks()
    }

    /// Buffer coordinates once crossing legalization has run. See
    /// [`PlacementPhase::buffer_coords`].
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        self.phase.buffer_coords()
    }

    /// Which pass last wrote to this cell. See
    /// [`PlacementPhase::stage`].
    #[must_use]
    pub const fn stage(&self) -> PlacementStage {
        self.phase.stage()
    }
}

// If [`PlacedCellNode`] grows a new visible field — or [`PlacementPhase`]
// gains a Stage-5 variant whose payload the JSON must expose — add it to
// both the field-count tally and the `serialize_field` calls below, and
// give the new stage a [`PlacementStage`] variant so the `stage` tag
// keeps naming the pass that produced the dump.
// `serde_json` tolerates a field-count mismatch, but binary formats
// (bincode, postcard, msgpack) rely on the announced count being exact;
// the `debug_assert_eq!` at the end catches a divergence in tests. Do not
// `#[derive(Serialize)]` this type — the derived output would tag the
// enum variant and reshape the flat wire form locked in by
// `routing_leaves_placement_fields_byte_identical_apart_from_wire_length_and_stage`
// et al.
impl Serialize for PlacedCellNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire_length = self.wire_length();
        let delay_ticks = self.delay_ticks();
        let buffer_coords = self.buffer_coords();

        let mut field_count = 4; // stage, cell, drivers, coord
        if wire_length.is_some() {
            field_count += 1;
        }
        if delay_ticks.is_some() {
            field_count += 1;
        }
        if !buffer_coords.is_empty() {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("PlacedCellNode", field_count)?;
        let mut written = 0_usize;
        // First so a truncated or eyeballed dump still says which pass
        // produced it, and so the tag reads ahead of the values it
        // qualifies.
        state.serialize_field("stage", &self.stage())?;
        written += 1;
        state.serialize_field("cell", &self.cell)?;
        written += 1;
        state.serialize_field("drivers", &self.drivers)?;
        written += 1;
        state.serialize_field("coord", &self.coord)?;
        written += 1;
        if let Some(wl) = wire_length {
            state.serialize_field("wire_length", &wl)?;
            written += 1;
        }
        if let Some(dt) = delay_ticks {
            state.serialize_field("delay_ticks", &dt)?;
            written += 1;
        }
        if !buffer_coords.is_empty() {
            state.serialize_field("buffer_coords", buffer_coords)?;
            written += 1;
        }
        debug_assert_eq!(
            written, field_count,
            "PlacedCellNode Serialize: announced field_count ({field_count}) diverges from serialize_field call count ({written}); binary formats such as bincode / postcard would produce malformed output",
        );
        state.end()
    }
}

/// An actuator-driven net sink, placed at its output pad.
///
/// The placement counterpart of [`crate::netlist_ir::NetlistOutput`],
/// and the reason it
/// exists is that the wire from a driver to an actuator is wire like any
/// other: it attenuates, it needs buffer repeaters past
/// [`crate::DUST_ATTENUATION_LIMIT`], and those repeaters occupy coords
/// inside the reservation. Before this node existed, the pass counted
/// buffers on the segments *into* cells and none on the segment out of
/// the last one, so a driver sitting a hundred blocks from its actuator
/// reported the same delay as one sitting next to it.
///
/// It carries the same [`PlacementPhase`] the cells do, so the four
/// stages fill it by the same rules and a dump reads alike on both
/// sides: `wire_length` after routing, `delay_ticks` after delay
/// insertion, `buffer_coords` after legalization. `delay_ticks` here is
/// the wire's own contribution — the buffers standing on the segment —
/// with no base delay to add, because a pad is not a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedOutputNode {
    /// Dotted signal reference the actuator consumes, carried across
    /// from [`crate::netlist_ir::NetlistOutput::name`].
    pub name: DottedRef,
    /// Which net drives this port, carried across from
    /// [`crate::netlist_ir::NetlistOutput::driver`].
    pub driver: NetRef,
    /// Coordinate the placement pass assigned this actuator's pad, per
    /// the `x = width - 1` convention the routing geometry uses.
    pub pad: CellCoord,
    /// Progressive pipeline state — see [`PlacementPhase`]. Crate-
    /// visible for the same reason [`PlacedCellNode::phase`] is: writes
    /// go through the transition methods so the illegal shapes stay
    /// unrepresentable.
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `opened_by=` argument.
    pub span: Span,
}

impl PlacedOutputNode {
    /// A freshly placed actuator pad, before routing has run. Mirrors
    /// the [`CellCoord::new`] / [`BufferCoord::new`] convention so the
    /// placement pass and the fixtures build one without struct-literal
    /// noise, and so the starting phase is chosen in one place rather
    /// than at every construction site.
    ///
    /// Crate-visible: the pad is the placement pass's to assign, from
    /// the reservation, and a coordinate handed in from outside is one
    /// nothing checks against the region. [`PlacedCellNode`] is
    /// unconstructible from another crate for the same reason — it has
    /// no constructor and the struct is `#[non_exhaustive]`.
    #[must_use]
    pub(crate) fn new(name: DottedRef, driver: NetRef, pad: CellCoord, span: Span) -> Self {
        Self {
            name,
            driver,
            pad,
            phase: PlacementPhase::Unrouted,
            span,
        }
    }

    /// Routed length of the segment from this output's driver to its
    /// pad, once routing has run. See [`PlacementPhase::wire_length`].
    #[must_use]
    pub const fn wire_length(&self) -> Option<u32> {
        self.phase.wire_length()
    }

    /// Ticks the buffer repeaters on this output's segment add, once
    /// delay insertion has run. See [`PlacementPhase::delay_ticks`].
    #[must_use]
    pub const fn delay_ticks(&self) -> Option<u32> {
        self.phase.delay_ticks()
    }

    /// Buffer coordinates once crossing legalization has run. See
    /// [`PlacementPhase::buffer_coords`].
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        self.phase.buffer_coords()
    }

    /// Which pass last wrote to this output. See
    /// [`PlacementPhase::stage`].
    #[must_use]
    pub const fn stage(&self) -> PlacementStage {
        self.phase.stage()
    }
}

// Hand-written for the same reasons [`PlacedCellNode`]'s is: the derived
// form would tag the phase enum and reshape the flat wire form, and
// binary formats rely on the announced field count being exact. Keep the
// two impls in step — a consumer reading a dump should not have to learn
// two shapes for "the same stage wrote this".
impl Serialize for PlacedOutputNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire_length = self.wire_length();
        let delay_ticks = self.delay_ticks();
        let buffer_coords = self.buffer_coords();

        let mut field_count = 4; // stage, name, driver, pad
        if wire_length.is_some() {
            field_count += 1;
        }
        if delay_ticks.is_some() {
            field_count += 1;
        }
        if !buffer_coords.is_empty() {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("PlacedOutputNode", field_count)?;
        let mut written = 0_usize;
        state.serialize_field("stage", &self.stage())?;
        written += 1;
        state.serialize_field("name", &self.name)?;
        written += 1;
        state.serialize_field("driver", &self.driver)?;
        written += 1;
        state.serialize_field("pad", &self.pad)?;
        written += 1;
        if let Some(wl) = wire_length {
            state.serialize_field("wire_length", &wl)?;
            written += 1;
        }
        if let Some(dt) = delay_ticks {
            state.serialize_field("delay_ticks", &dt)?;
            written += 1;
        }
        if !buffer_coords.is_empty() {
            state.serialize_field("buffer_coords", buffer_coords)?;
            written += 1;
        }
        debug_assert_eq!(
            written, field_count,
            "PlacedOutputNode Serialize: announced field_count ({field_count}) diverges from serialize_field call count ({written}); binary formats such as bincode / postcard would produce malformed output",
        );
        state.end()
    }
}

/// The Placement IR for one struct/def body.
///
/// One instance per [`crate::edition_netlist_ir::EditionNetlistIr`]
/// handed to the pass whose scope produced at least one cell or one
/// actuator. The target edition is pinned so a JSON dump makes clear
/// which library the cells were selected from before placement ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PlacementIr {
    /// Edition this IR was compiled for. Equal to every cell's
    /// [`EditionCell::edition`] by construction.
    pub edition: Edition,
    /// Reservation the cells were placed into. Absent only when the
    /// scope had zero cells (an empty scope is elided by
    /// [`ScopedPlacementIr::push`], so a serialised entry always has a
    /// reservation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<CircuitRegionReservation>,
    /// Sensor-driven nets, copied verbatim from the source Edition
    /// Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, each placed at its output pad. Carried
    /// across from the source Edition Netlist IR and given a coordinate
    /// by the placement pass, so the wire out to an actuator is routed,
    /// delayed, and buffered by the same rules as the wire into a cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PlacedOutputNode>,
    /// Placed cells, in the same topological order as the source
    /// [`crate::edition_netlist_ir::EditionNetlistIr::cells`]
    /// (`NetRef::Cell(j)` in `cells[i]` still satisfies `j < i`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<PlacedCellNode>,
    /// `sig.NAME` → the net that defines it, copied verbatim from the
    /// source Edition Netlist IR.
    ///
    /// Serialises as a JSON object keyed by the flattened dotted name —
    /// the same custom serialisation the Logic / Netlist / Edition
    /// Netlist IRs use — so consumers see a uniform shape across every
    /// IR in the pipeline.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

fn serialize_signal_defs<S: Serializer>(
    defs: &IndexMap<DottedRef, NetRef>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(defs.len()))?;
    for (name, net) in defs {
        map.serialize_entry(&name.to_string(), net)?;
    }
    map.end()
}

impl PlacementIr {
    /// Empty Placement IR for the given edition — no reservation, no
    /// inputs, no outputs, no cells.
    #[must_use]
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            region: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            cells: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when this scope produced zero inputs, zero outputs, and
    /// zero cells. Matches [`crate::edition_netlist_ir::EditionNetlistIr::is_empty`]
    /// so scope elision behaves the same across every IR in the
    /// pipeline.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.cells.is_empty()
    }
}

/// Ordered list of `(scope kind, scope name)` → [`PlacementIr`] entries
/// covering an entire `.crn` module.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ScopedPlacementIr {
    /// Per-scope Placement IR, in source order across the whole module.
    pub scopes: Vec<ScopedPlacementIrEntry>,
}

impl ScopedPlacementIr {
    /// Empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one scope's Placement IR. Empty scopes are elided so a
    /// module without any redstone renders as `[]`, matching the
    /// sibling IRs' elision.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: PlacementIr) {
        if ir.is_empty() {
            debug_assert!(
                ir.signal_defs.is_empty(),
                "empty PlacementIr carries orphan signal_defs entries: {:?}",
                ir.signal_defs.keys().collect::<Vec<_>>(),
            );
            debug_assert!(
                ir.region.is_none(),
                "empty PlacementIr carries an orphan region: {:?}",
                ir.region,
            );
            return;
        }
        self.scopes.push(ScopedPlacementIrEntry { kind, name, ir });
    }

    /// `true` when no scope produced a non-empty Placement IR.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`ScopedPlacementIr`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedPlacementIrEntry {
    /// Which family of scope this Placement IR came from.
    pub kind: ScopeKind,
    /// Source-level name of the scope.
    pub name: String,
    /// Placement IR laid out from the scope's Edition Netlist IR.
    pub ir: PlacementIr,
}

#[cfg(test)]
mod tests {
    //! Direct coverage for [`PlacementPhase`] — the four legal
    //! transitions, the nine illegal ones the transition methods must
    //! reject (with a release-loud panic, or with a
    //! [`PlacementPhaseTransitionError`] from the `try_*` mirrors), and
    //! the flat accessor projections. Each pipeline pass (`routing.rs`
    //! / `delay.rs` / `crossing.rs`) has its own end-to-end coverage;
    //! this module pins the state-machine invariants of the enum itself
    //! so a regression that loosens a transition guard is caught here
    //! rather than at the far end of the pipeline.
    use super::*;

    fn routed() -> PlacementPhase {
        PlacementPhase::Routed { wire_length: 3 }
    }

    fn delayed() -> PlacementPhase {
        PlacementPhase::Delayed {
            wire_length: 3,
            delay_ticks: 1,
        }
    }

    fn legalized() -> PlacementPhase {
        // `PortName::A` is fixture noise — none of the state-machine
        // tests below assert on the driver port; they only care about
        // the buffer-coord vector's shape and identity round-trip.
        PlacementPhase::Legalized {
            wire_length: 3,
            delay_ticks: 1,
            buffer_coords: vec![BufferCoord::new(
                BufferSegment::Port(PortName::A),
                CellCoord::new(5, 0, 0),
            )],
        }
    }

    #[test]
    fn route_from_unrouted_transitions_to_routed() {
        let mut phase = PlacementPhase::Unrouted;
        phase.route(7);
        assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
    }

    #[test]
    #[should_panic(expected = "routing must run exactly once per placement")]
    fn route_from_routed_panics() {
        let mut phase = routed();
        phase.route(5);
    }

    #[test]
    #[should_panic(expected = "routing must run exactly once per placement")]
    fn route_from_delayed_panics() {
        let mut phase = delayed();
        phase.route(5);
    }

    #[test]
    #[should_panic(expected = "routing must run exactly once per placement")]
    fn route_from_legalized_panics() {
        let mut phase = legalized();
        phase.route(5);
    }

    #[test]
    fn delay_from_routed_transitions_to_delayed_and_preserves_wire_length() {
        let mut phase = PlacementPhase::Routed { wire_length: 9 };
        phase.delay(4);
        assert_eq!(
            phase,
            PlacementPhase::Delayed {
                wire_length: 9,
                delay_ticks: 4,
            },
        );
    }

    #[test]
    #[should_panic(expected = "delay insertion must run exactly once per routed IR")]
    fn delay_from_unrouted_panics() {
        let mut phase = PlacementPhase::Unrouted;
        phase.delay(2);
    }

    #[test]
    #[should_panic(expected = "delay insertion must run exactly once per routed IR")]
    fn delay_from_delayed_panics() {
        let mut phase = delayed();
        phase.delay(2);
    }

    #[test]
    #[should_panic(expected = "delay insertion must run exactly once per routed IR")]
    fn delay_from_legalized_panics() {
        let mut phase = legalized();
        phase.delay(2);
    }

    #[test]
    fn legalize_from_delayed_transitions_and_preserves_prior_fields() {
        let mut phase = PlacementPhase::Delayed {
            wire_length: 12,
            delay_ticks: 3,
        };
        let coords = vec![
            BufferCoord::new(BufferSegment::Port(PortName::A), CellCoord::new(1, 0, 0)),
            BufferCoord::new(BufferSegment::Port(PortName::A), CellCoord::new(2, 0, 0)),
        ];
        phase.legalize(coords.clone());
        assert_eq!(
            phase,
            PlacementPhase::Legalized {
                wire_length: 12,
                delay_ticks: 3,
                buffer_coords: coords,
            },
        );
    }

    #[test]
    fn legalize_from_delayed_with_empty_buffers_stays_legalized() {
        // A scope whose delay pass counted zero buffers must still
        // transition to Legalized — an empty buffer_coords does not
        // mean "not yet legalized".
        let mut phase = PlacementPhase::Delayed {
            wire_length: 4,
            delay_ticks: 0,
        };
        phase.legalize(Vec::new());
        assert!(matches!(
            phase,
            PlacementPhase::Legalized {
                buffer_coords: ref bc,
                ..
            } if bc.is_empty()
        ));
    }

    #[test]
    #[should_panic(expected = "crossing legalization must run exactly once per delayed IR")]
    fn legalize_from_unrouted_panics() {
        let mut phase = PlacementPhase::Unrouted;
        phase.legalize(vec![]);
    }

    #[test]
    #[should_panic(expected = "crossing legalization must run exactly once per delayed IR")]
    fn legalize_from_routed_panics() {
        let mut phase = routed();
        phase.legalize(vec![]);
    }

    #[test]
    #[should_panic(expected = "crossing legalization must run exactly once per delayed IR")]
    fn legalize_from_legalized_panics() {
        let mut phase = legalized();
        phase.legalize(vec![]);
    }

    fn probe_scope() -> ScopedPlacementIrEntry {
        ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "probe".to_string(),
            ir: PlacementIr::new(Edition::Java),
        }
    }

    fn probe_identity(scope: &ScopedPlacementIrEntry) -> CellIdentity<'_> {
        CellIdentity::new(7, CellCoord::new(1, 0, 2), scope)
    }

    #[test]
    fn cell_identity_renders_index_coord_and_scope() {
        let scope = probe_scope();
        assert_eq!(
            probe_identity(&scope).to_string(),
            "cell #7 at (1,0,2) in struct `probe`",
        );
    }

    /// Every scope family has to render, not just the `struct` the
    /// other fixtures use — a `def` or `site` scope that fell through
    /// to a placeholder label would leave an operator unable to tell
    /// two same-named scopes apart.
    #[test]
    fn cell_identity_renders_every_scope_kind() {
        for (kind, expected) in [
            (ScopeKind::Struct, "cell #7 at (1,0,2) in struct `probe`"),
            (ScopeKind::Def, "cell #7 at (1,0,2) in def `probe`"),
            (ScopeKind::Site, "cell #7 at (1,0,2) in site `probe`"),
        ] {
            let scope = ScopedPlacementIrEntry {
                kind,
                ..probe_scope()
            };
            assert_eq!(probe_identity(&scope).to_string(), expected);
        }
    }

    /// A cell coord is `Plane` by construction, so the layer stays
    /// suppressed on every real breadcrumb. A hand-built IR that
    /// breaks that invariant must not print a coord that reads as a
    /// plane coord, so the escape layer surfaces rather than being
    /// dropped.
    #[test]
    fn cell_identity_surfaces_a_non_plane_layer() {
        let scope = probe_scope();
        let bridged = CellCoord {
            layer: RouteLayer::Bridge,
            ..CellCoord::new(1, 0, 2)
        };
        assert_eq!(
            CellIdentity::new(7, bridged, &scope).to_string(),
            "cell #7 at (1,0,2 on bridge) in struct `probe`",
        );
    }

    #[test]
    fn route_at_from_unrouted_transitions_to_routed() {
        let scope = probe_scope();
        let mut phase = PlacementPhase::Unrouted;
        phase.route_at(7, probe_identity(&scope));
        assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — routing must run exactly once per placement"
    )]
    fn route_at_from_routed_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = routed();
        phase.route_at(5, probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — routing must run exactly once per placement"
    )]
    fn route_at_from_delayed_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = delayed();
        phase.route_at(5, probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — routing must run exactly once per placement"
    )]
    fn route_at_from_legalized_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = legalized();
        phase.route_at(5, probe_identity(&scope));
    }

    #[test]
    fn delay_at_from_routed_transitions_and_preserves_wire_length() {
        let scope = probe_scope();
        let mut phase = PlacementPhase::Routed { wire_length: 9 };
        phase.delay_at(4, probe_identity(&scope));
        assert_eq!(
            phase,
            PlacementPhase::Delayed {
                wire_length: 9,
                delay_ticks: 4,
            },
        );
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — delay insertion must run exactly once per routed IR"
    )]
    fn delay_at_from_unrouted_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = PlacementPhase::Unrouted;
        phase.delay_at(2, probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — delay insertion must run exactly once per routed IR"
    )]
    fn delay_at_from_delayed_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = delayed();
        phase.delay_at(2, probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — delay insertion must run exactly once per routed IR"
    )]
    fn delay_at_from_legalized_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = legalized();
        phase.delay_at(2, probe_identity(&scope));
    }

    #[test]
    fn legalize_at_from_delayed_transitions_and_preserves_prior_fields() {
        let scope = probe_scope();
        let mut phase = PlacementPhase::Delayed {
            wire_length: 12,
            delay_ticks: 3,
        };
        let coords = vec![BufferCoord::new(
            BufferSegment::Port(PortName::A),
            CellCoord::new(1, 0, 0),
        )];
        phase.legalize_at(coords.clone(), probe_identity(&scope));
        assert_eq!(
            phase,
            PlacementPhase::Legalized {
                wire_length: 12,
                delay_ticks: 3,
                buffer_coords: coords,
            },
        );
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — crossing legalization must run exactly once per delayed IR"
    )]
    fn legalize_at_from_unrouted_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = PlacementPhase::Unrouted;
        phase.legalize_at(vec![], probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — crossing legalization must run exactly once per delayed IR"
    )]
    fn legalize_at_from_routed_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = routed();
        phase.legalize_at(vec![], probe_identity(&scope));
    }

    #[test]
    #[should_panic(
        expected = "for cell #7 at (1,0,2) in struct `probe` — crossing legalization must run exactly once per delayed IR"
    )]
    fn legalize_at_from_legalized_panics_with_cell_identity() {
        let scope = probe_scope();
        let mut phase = legalized();
        phase.legalize_at(vec![], probe_identity(&scope));
    }

    /// One of the three transitions, as a value, so a test can drive
    /// every refusal through one loop instead of picking a
    /// representative per transition and leaving the other two
    /// rejected phases unrendered.
    ///
    /// The fixture arguments are baked in: which `wire_length` or
    /// buffer vector a refused call was handed never reaches the
    /// message, so varying them here would only obscure the pairing.
    #[derive(Clone, Copy)]
    enum Transition {
        Route,
        Delay,
        Legalize,
    }

    impl Transition {
        /// The fallible form.
        fn tried(self, phase: &mut PlacementPhase) -> Result<(), PlacementPhaseTransitionError> {
            match self {
                Self::Route => phase.try_route(5),
                Self::Delay => phase.try_delay(2),
                Self::Legalize => phase.try_legalize(vec![]),
            }
        }

        /// The panicking form.
        fn panicking(self, phase: &mut PlacementPhase) {
            match self {
                Self::Route => phase.route(5),
                Self::Delay => phase.delay(2),
                Self::Legalize => phase.legalize(vec![]),
            }
        }

        /// The panicking form that names a context.
        fn panicking_at(self, phase: &mut PlacementPhase, context: impl fmt::Display) {
            match self {
                Self::Route => phase.route_at(5, context),
                Self::Delay => phase.delay_at(2, context),
                Self::Legalize => phase.legalize_at(vec![], context),
            }
        }

        /// Drive this transition against a phase that refuses it,
        /// against a throwaway clone so the caller's phase stays
        /// available for the next rendering of the same refusal.
        fn refusal(self, phase: &PlacementPhase) -> PlacementPhaseTransitionError {
            self.tried(&mut phase.clone())
                .expect_err("out-of-order transition must be refused")
        }
    }

    /// Every (transition, phase it refuses) pair — the nine illegal
    /// edges of the four-state machine.
    ///
    /// `Legalized` appears as a rejected phase for all three, which is
    /// what puts a populated `buffer_coords` through the `{current:?}`
    /// rendering the messages carry.
    fn refusals() -> [(Transition, PlacementPhase); 9] {
        [
            (Transition::Route, routed()),
            (Transition::Route, delayed()),
            (Transition::Route, legalized()),
            (Transition::Delay, PlacementPhase::Unrouted),
            (Transition::Delay, delayed()),
            (Transition::Delay, legalized()),
            (Transition::Legalize, PlacementPhase::Unrouted),
            (Transition::Legalize, routed()),
            (Transition::Legalize, legalized()),
        ]
    }

    #[test]
    fn try_route_from_unrouted_transitions_to_routed() {
        let mut phase = PlacementPhase::Unrouted;
        assert_eq!(phase.try_route(7), Ok(()));
        assert_eq!(phase, PlacementPhase::Routed { wire_length: 7 });
    }

    #[test]
    fn try_delay_from_routed_transitions_and_preserves_wire_length() {
        let mut phase = PlacementPhase::Routed { wire_length: 9 };
        assert_eq!(phase.try_delay(4), Ok(()));
        assert_eq!(
            phase,
            PlacementPhase::Delayed {
                wire_length: 9,
                delay_ticks: 4,
            },
        );
    }

    #[test]
    fn try_legalize_from_delayed_transitions_and_preserves_prior_fields() {
        let mut phase = PlacementPhase::Delayed {
            wire_length: 12,
            delay_ticks: 3,
        };
        let coords = vec![BufferCoord::new(
            BufferSegment::Port(PortName::A),
            CellCoord::new(1, 0, 0),
        )];
        assert_eq!(phase.try_legalize(coords.clone()), Ok(()));
        assert_eq!(
            phase,
            PlacementPhase::Legalized {
                wire_length: 12,
                delay_ticks: 3,
                buffer_coords: coords,
            },
        );
    }

    /// The fallible form owes the same "empty is still legalized"
    /// promise the panicking one does — a scope whose delay pass
    /// counted zero buffers must not read as "not yet legalized".
    #[test]
    fn try_legalize_from_delayed_with_empty_buffers_stays_legalized() {
        let mut phase = PlacementPhase::Delayed {
            wire_length: 4,
            delay_ticks: 0,
        };
        assert_eq!(phase.try_legalize(Vec::new()), Ok(()));
        assert!(matches!(
            phase,
            PlacementPhase::Legalized {
                buffer_coords: ref bc,
                ..
            } if bc.is_empty()
        ));
    }

    /// A refused transition must leave the subject exactly as it found
    /// it. The panicking forms never had to promise this — the process
    /// is going down — but a caller that recovers from the `Err` goes
    /// on to use the phase, so a half-applied write here would hand it
    /// a state the producer↔variant table says cannot exist.
    #[test]
    fn try_route_from_every_other_phase_errs_without_mutating() {
        for phase in [routed(), delayed(), legalized()] {
            let mut subject = phase.clone();
            let error = subject
                .try_route(5)
                .expect_err("routing a non-Unrouted phase must be refused");
            assert_eq!(subject, phase, "a refused transition mutated the phase");
            assert_eq!(
                error,
                PlacementPhaseTransitionError::RouteOnNonUnrouted { current: phase },
            );
        }
    }

    #[test]
    fn try_delay_from_every_other_phase_errs_without_mutating() {
        for phase in [PlacementPhase::Unrouted, delayed(), legalized()] {
            let mut subject = phase.clone();
            let error = subject
                .try_delay(2)
                .expect_err("delaying a non-Routed phase must be refused");
            assert_eq!(subject, phase, "a refused transition mutated the phase");
            assert_eq!(
                error,
                PlacementPhaseTransitionError::DelayOnNonRouted { current: phase },
            );
        }
    }

    #[test]
    fn try_legalize_from_every_other_phase_errs_without_mutating() {
        for phase in [PlacementPhase::Unrouted, routed(), legalized()] {
            let mut subject = phase.clone();
            let error = subject
                .try_legalize(vec![])
                .expect_err("legalizing a non-Delayed phase must be refused");
            assert_eq!(subject, phase, "a refused transition mutated the phase");
            assert_eq!(
                error,
                PlacementPhaseTransitionError::LegalizeOnNonDelayed { current: phase },
            );
        }
    }

    /// A phase that refused a transition must be more than *equal* to
    /// what it was — it must still work. Post-call equality alone would
    /// survive a refactor that moved out of `self` and rebuilt an equal
    /// value; carrying on into the next legal transition is what shows
    /// the subject itself came through.
    #[test]
    fn a_refused_transition_leaves_the_phase_usable() {
        let mut phase = routed();
        assert!(phase.try_route(5).is_err());
        phase.delay(2);
        assert_eq!(
            phase,
            PlacementPhase::Delayed {
                wire_length: 3,
                delay_ticks: 2,
            },
        );
    }

    /// The fallible and panicking forms are two renderings of one
    /// refusal, so the error's own `Display` must reproduce the
    /// context-free panic byte for byte. Comparing against the live
    /// panic payload rather than a hard-coded string means the two can
    /// only drift by drifting together.
    ///
    /// Every refusal is walked, not one per transition: the phase is
    /// rendered through `{current:?}`, so a `Legalized` subject is the
    /// only case that puts a `Vec<BufferCoord>` through it.
    #[test]
    fn try_transition_errors_render_as_the_context_free_panic() {
        for (transition, phase) in refusals() {
            assert_eq!(
                transition.refusal(&phase).to_string(),
                panic_message(|| transition.panicking(&mut phase.clone())),
            );
        }
    }

    /// A caller that recovered from the `Err` still knows what it was
    /// working on, so splicing that in must land in the same place and
    /// read the same way as the `*_at` forms — otherwise an ingest
    /// diagnostic and a pipeline panic about the same cell would not
    /// look alike.
    #[test]
    fn try_transition_errors_with_context_render_as_the_at_panic() {
        let scope = probe_scope();
        for (transition, phase) in refusals() {
            let error = transition.refusal(&phase);
            assert_eq!(
                error.with_context(probe_identity(&scope)).to_string(),
                panic_message(
                    || transition.panicking_at(&mut phase.clone(), probe_identity(&scope))
                ),
            );
        }
    }

    /// The consumers this fallible mirror exists for — cache
    /// validators, IR ingest, the language server — put the refusal on
    /// a `?` or into a `Box<dyn Error>`, and compare or stash it
    /// alongside the phase it names.
    ///
    /// Walked over every refusal so [`PlacementPhaseTransitionError::current`]
    /// is exercised on all three variants: it reads them through one
    /// merged arm, which a fourth variant added without a matching arm
    /// would silently leave out.
    #[test]
    fn transition_error_is_a_comparable_std_error() {
        fn assert_std_error<E: std::error::Error + Clone + Eq>(_: &E) {}

        for (transition, phase) in refusals() {
            let error = transition.refusal(&phase);
            assert_std_error(&error);
            assert_eq!(error.clone(), error);
            assert_eq!(error.current(), &phase);
            assert!(std::error::Error::source(&error).is_none());
        }
    }

    /// The context-free transition forms must not grow a dangling
    /// separator now that the `*_at` variants splice one in. Pinned by
    /// reading the panic payload directly rather than through
    /// `#[should_panic]`, which can only assert on a substring that is
    /// present — never on one that is absent.
    #[test]
    fn context_free_transitions_panic_without_an_identity_clause() {
        let payloads = [
            panic_message(|| routed().route(5)),
            panic_message(|| PlacementPhase::Unrouted.delay(2)),
            panic_message(|| PlacementPhase::Unrouted.legalize(vec![])),
        ];
        for payload in &payloads {
            assert!(
                !payload.contains(" for "),
                "context-free panic grew an identity clause: {payload}",
            );
            assert!(
                !payload.contains("  "),
                "context-free panic grew a double space: {payload}",
            );
        }
        // Exact match on all three, not just one: the context-free
        // wording is the wire format the `#[should_panic]` sites
        // across this crate key on.
        assert_eq!(
            payloads,
            [
                "PlacementPhase::route called on Routed { wire_length: 3 } — routing must run exactly once per placement",
                "PlacementPhase::delay called on Unrouted — delay insertion must run exactly once per routed IR",
                "PlacementPhase::legalize called on Unrouted — crossing legalization must run exactly once per delayed IR",
            ],
        );
    }

    /// Pin that all nine entry points quote one shared clause, so a
    /// transition added beside these three cannot spell its own.
    ///
    /// Asserting on the const puts it on both sides, so this test
    /// cannot tell what the clause *says*; the exact-match array in
    /// [`context_free_transitions_panic_without_an_identity_clause`]
    /// is what pins the wording itself. The entry points are listed
    /// by hand, so a transition that bypassed [`TransitionMessage`]
    /// would escape both — the const is the structural guard, and
    /// these two tests only keep its reach honest.
    #[test]
    fn every_transition_message_shares_one_cardinality_clause() {
        let scope = probe_scope();
        let messages = [
            panic_message(|| routed().route(5)),
            panic_message(|| PlacementPhase::Unrouted.delay(2)),
            panic_message(|| PlacementPhase::Unrouted.legalize(vec![])),
            panic_message(|| routed().route_at(5, probe_identity(&scope))),
            panic_message(|| PlacementPhase::Unrouted.delay_at(2, probe_identity(&scope))),
            panic_message(|| PlacementPhase::Unrouted.legalize_at(vec![], probe_identity(&scope))),
            Transition::Route.refusal(&routed()).to_string(),
            Transition::Delay
                .refusal(&PlacementPhase::Unrouted)
                .to_string(),
            Transition::Legalize
                .refusal(&PlacementPhase::Unrouted)
                .to_string(),
        ];
        for message in &messages {
            assert!(
                message.contains(TRANSITION_CARDINALITY),
                "transition message does not quote `{TRANSITION_CARDINALITY}`: {message}",
            );
        }
    }

    fn panic_message(body: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let payload = std::panic::catch_unwind(body).expect_err("transition must panic");
        std::panic::set_hook(previous);
        payload
            .downcast_ref::<String>()
            .cloned()
            .expect("transition panics carry a formatted String payload")
    }

    #[test]
    fn wire_length_accessor_covers_every_variant() {
        assert_eq!(PlacementPhase::Unrouted.wire_length(), None);
        assert_eq!(routed().wire_length(), Some(3));
        assert_eq!(delayed().wire_length(), Some(3));
        assert_eq!(legalized().wire_length(), Some(3));
    }

    #[test]
    fn delay_ticks_accessor_covers_every_variant() {
        assert_eq!(PlacementPhase::Unrouted.delay_ticks(), None);
        assert_eq!(routed().delay_ticks(), None);
        assert_eq!(delayed().delay_ticks(), Some(1));
        assert_eq!(legalized().delay_ticks(), Some(1));
    }

    #[test]
    fn buffer_coords_accessor_covers_every_variant() {
        assert!(PlacementPhase::Unrouted.buffer_coords().is_empty());
        assert!(routed().buffer_coords().is_empty());
        assert!(delayed().buffer_coords().is_empty());
        assert_eq!(
            legalized().buffer_coords(),
            &[BufferCoord::new(
                BufferSegment::Port(PortName::A),
                CellCoord::new(5, 0, 0)
            )],
        );
    }

    #[test]
    fn stage_accessor_covers_every_variant() {
        assert_eq!(PlacementPhase::Unrouted.stage(), PlacementStage::Placement);
        assert_eq!(routed().stage(), PlacementStage::Route);
        assert_eq!(delayed().stage(), PlacementStage::Delay);
        assert_eq!(legalized().stage(), PlacementStage::Crossing);
    }

    /// The whole point of the tag: a scope whose delay pass counted
    /// zero buffers still reports the crossing stage, so a consumer
    /// never has to infer "did stage 4 run?" from the absence of
    /// `buffer_coords`.
    #[test]
    fn legalized_with_zero_buffers_still_reports_the_crossing_stage() {
        let empty = PlacementPhase::Legalized {
            wire_length: 3,
            delay_ticks: 1,
            buffer_coords: Vec::new(),
        };
        assert_eq!(empty.stage(), PlacementStage::Crossing);
        assert_eq!(empty.stage(), legalized().stage());
        assert_ne!(empty.stage(), delayed().stage());
    }

    fn probe_cell(phase: PlacementPhase) -> PlacedCellNode {
        PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: Vec::new(),
            coord: CellCoord::new(0, 0, 0),
            phase,
            span: Span::default(),
        }
    }

    #[test]
    fn placed_cell_stage_accessor_projects_the_phase() {
        for phase in [PlacementPhase::Unrouted, routed(), delayed(), legalized()] {
            let expected = phase.stage();
            assert_eq!(probe_cell(phase).stage(), expected);
        }
    }

    /// The four strings are the JSON wire form and must equal the
    /// `cairn synth --stage <s>` flag values, so they are pinned here
    /// rather than left to whatever `as_str` happens to return.
    #[test]
    fn stage_renders_the_cli_flag_vocabulary() {
        assert_eq!(PlacementStage::Placement.as_str(), "placement");
        assert_eq!(PlacementStage::Route.as_str(), "route");
        assert_eq!(PlacementStage::Delay.as_str(), "delay");
        assert_eq!(PlacementStage::Crossing.as_str(), "crossing");
    }

    /// Every variant serialises as a bare string, not as a tagged
    /// object — a derived `Serialize` on a fieldless enum would also
    /// produce a string, but pinning it here keeps a future refactor
    /// from silently reshaping the tag. All four are covered rather
    /// than one representative: `Serialize` routes through `as_str`'s
    /// `match`, so a single sample cannot catch a mis-wired arm.
    #[test]
    fn every_stage_serialises_as_a_bare_string() {
        for (stage, expected) in [
            (PlacementStage::Placement, "\"placement\""),
            (PlacementStage::Route, "\"route\""),
            (PlacementStage::Delay, "\"delay\""),
            (PlacementStage::Crossing, "\"crossing\""),
        ] {
            assert_eq!(
                serde_json::to_string(&stage).expect("stage serialises"),
                expected,
            );
        }
    }

    /// The reason the tag exists: before it, these two phases produced
    /// identical JSON.
    #[test]
    fn delayed_and_legalized_with_zero_buffers_serialise_differently() {
        let delayed_json =
            serde_json::to_string(&probe_cell(delayed())).expect("delayed cell serialises");
        let legalized_json = serde_json::to_string(&probe_cell(PlacementPhase::Legalized {
            wire_length: 3,
            delay_ticks: 1,
            buffer_coords: Vec::new(),
        }))
        .expect("legalized cell serialises");

        assert_ne!(delayed_json, legalized_json);
        assert!(
            delayed_json.contains("\"stage\":\"delay\""),
            "delayed cell must carry the delay tag: {delayed_json}",
        );
        assert!(
            legalized_json.contains("\"stage\":\"crossing\""),
            "legalized cell must carry the crossing tag: {legalized_json}",
        );
        // The tag is the *only* difference: `buffer_coords` still
        // serde-skips when empty, so the sentinel-array alternative
        // stays unadopted.
        assert!(
            !legalized_json.contains("buffer_coords"),
            "empty buffer_coords must still serde-skip: {legalized_json}",
        );
        // Rewriting the tag's key/value pair — not the bare word
        // `"delay"`, which also prefixes the `delay_ticks` key — turns
        // one dump into the other exactly, so nothing else moved.
        assert_eq!(
            delayed_json.replace("\"stage\":\"delay\"", "\"stage\":\"crossing\""),
            legalized_json,
        );
    }
}
