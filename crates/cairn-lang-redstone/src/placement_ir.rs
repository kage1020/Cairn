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
//! change, so downstream JSON consumers see a stable wire shape.
//!
//! `circuit region=... void=N` congestion detection (`E_ROUTE_CONGESTION`)
//! and missing-reservation refusal (`E_NO_CIRCUIT_REGION`) fire here —
//! this is the first pass with a physical footprint to measure against.
//! Attenuation limits (dust segments exceeding 15 blocks) belong to the
//! routing pass and stay a follow-up.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

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
    /// shape is a pure additive subset of the legalized IR shape.
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
/// entirely: the legalized IR shape is a pure additive subset of the
/// earlier stages' shape.
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

/// Progressive state of a [`PlacedCellNode`] as it moves through the
/// four-stage place-and-route pipeline (`spec/redstone` §14.5).
///
/// Every legal `(wire_length, delay_ticks, buffer_coords)` combination
/// that the pipeline can produce corresponds to exactly one variant:
///
/// | Producer                                             | Variant                                                                     |
/// |------------------------------------------------------|-----------------------------------------------------------------------------|
/// | [`crate::placement::compile_placement`] (Stage 1)    | [`Self::Unrouted`]                                                          |
/// | [`crate::routing::compile_routing`]     (Stage 2)    | [`Self::Routed`] `{ wire_length }`                                          |
/// | [`crate::delay::compile_delay`]         (Stage 3)    | [`Self::Delayed`] `{ wire_length, delay_ticks }`                            |
/// | [`crate::crossing::compile_crossing`]   (Stage 4)    | [`Self::Legalized`] `{ wire_length, delay_ticks, buffer_coords }`           |
///
/// Illegal shapes such as "have `delay_ticks` but no `wire_length`" or
/// "carry `buffer_coords` before the delay pass has run" are
/// unrepresentable — each transition is expressed by the mutation
/// methods [`Self::route`], [`Self::delay`], and [`Self::legalize`],
/// which pattern-match the current variant and panic on any out-of-order
/// call. `buffer_coords` on [`Self::Legalized`] is allowed to be empty:
/// the crossing pass materialises one entry per implicit buffer the
/// delay pass counted, and a scope whose delay pass counted zero
/// buffers is still legalized (transitions to [`Self::Legalized`] with
/// an empty vector).
#[derive(Debug, Clone, PartialEq, Eq)]
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
        /// One coord per implicit buffer the delay pass counted. Each
        /// entry carries the [`RouteLayer`] the pass chose — `Plane`
        /// when the coord fit the ground layer, `Bridge` when the
        /// plane candidate collided with a cell / pad / plane crossing /
        /// earlier buffer and had to escape upward. Empty when the
        /// delay pass counted zero buffers.
        buffer_coords: Vec<CellCoord>,
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
    #[must_use]
    pub fn buffer_coords(&self) -> &[CellCoord] {
        match self {
            Self::Legalized { buffer_coords, .. } => buffer_coords,
            _ => &[],
        }
    }

    /// [`Self::Unrouted`] → [`Self::Routed`].
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Unrouted`]. Routing must run
    /// exactly once per placement, and the phase table on
    /// [`PlacedCellNode`] forbids re-routing.
    #[track_caller]
    pub fn route(&mut self, wire_length: u32) {
        match self {
            Self::Unrouted => {}
            other => panic!(
                "PlacementPhase::route called on {other:?} — routing must run once per placement"
            ),
        }
        *self = Self::Routed { wire_length };
    }

    /// [`Self::Routed`] → [`Self::Delayed`].
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Routed`]. Delay insertion
    /// must run exactly once per routed IR, and the phase table on
    /// [`PlacedCellNode`] forbids re-writing a `delay_ticks` that was
    /// already committed.
    #[track_caller]
    pub fn delay(&mut self, delay_ticks: u32) {
        let wire_length = match self {
            Self::Routed { wire_length } => *wire_length,
            other => panic!(
                "PlacementPhase::delay called on {other:?} — delay insertion must run once per routed IR"
            ),
        };
        *self = Self::Delayed {
            wire_length,
            delay_ticks,
        };
    }

    /// [`Self::Delayed`] → [`Self::Legalized`].
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Delayed`]. Crossing
    /// legalization must run at most once per delayed IR — this matches
    /// the earlier release-loud `assert!` on the crossing pass, so a
    /// caller who chained `compile_crossing(&legalized.scoped)` trips
    /// here rather than silently producing a stale-but-plausible IR.
    #[track_caller]
    pub fn legalize(&mut self, buffer_coords: Vec<CellCoord>) {
        let (wire_length, delay_ticks) = match self {
            Self::Delayed {
                wire_length,
                delay_ticks,
            } => (*wire_length, *delay_ticks),
            other => panic!(
                "PlacementPhase::legalize called on {other:?} — crossing legalization must run at most once per delayed IR"
            ),
        };
        *self = Self::Legalized {
            wire_length,
            delay_ticks,
            buffer_coords,
        };
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
/// on the phase enum.
///
/// The custom [`Serialize`] impl flattens [`Self::phase`] back onto
/// `{cell, drivers, coord[, wire_length][, delay_ticks][, buffer_coords]}` —
/// exactly the shape earlier revisions of this struct produced via
/// three `skip_serializing_if` optionals, so the JSON dump of a
/// stage-N output is still a pure additive subset of the stage-(N+1)
/// dump on scopes whose stage-(N+1) pass had nothing to write.
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
    pub phase: PlacementPhase,
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
    pub fn buffer_coords(&self) -> &[CellCoord] {
        self.phase.buffer_coords()
    }
}

impl Serialize for PlacedCellNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire_length = self.wire_length();
        let delay_ticks = self.delay_ticks();
        let buffer_coords = self.buffer_coords();

        let mut field_count = 3; // cell, drivers, coord
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
        state.serialize_field("cell", &self.cell)?;
        state.serialize_field("drivers", &self.drivers)?;
        state.serialize_field("coord", &self.coord)?;
        if let Some(wl) = wire_length {
            state.serialize_field("wire_length", &wl)?;
        }
        if let Some(dt) = delay_ticks {
            state.serialize_field("delay_ticks", &dt)?;
        }
        if !buffer_coords.is_empty() {
            state.serialize_field("buffer_coords", buffer_coords)?;
        }
        state.end()
    }
}

/// The Placement IR for one struct/def body.
///
/// One instance per [`crate::edition_netlist_ir::EditionNetlistIr`]
/// handed to the pass whose scope produced at least one cell. The
/// target edition is pinned so a JSON dump makes clear which library
/// the cells were selected from before placement ran.
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
    /// Actuator-driven nets, copied verbatim from the source Edition
    /// Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<NetlistOutput>,
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
