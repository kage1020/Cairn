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
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

/// Which of `spec/redstone` §14.5's three pseudo-2.5D layers a
/// coordinate lives on. `Plane` is the ground layer every cell coord
/// and every un-escaped wire segment sits on; `Bridge` is the
/// horizontal escape layer the crossing-legalization pass lifts a wire
/// onto when two nets would otherwise share a `Plane` coord; `Via` is
/// the vertical tap between the two, materialised where a bridge
/// segment enters or leaves the `Plane`.
///
/// Cell coords are `Plane` by construction — the placement pass never
/// stamps `Bridge` / `Via` on a cell body. Buffer-repeater coords and
/// wire coords the crossing-legalization pass writes may carry either
/// escape layer; a bridge coord marks a segment lifted off the plane,
/// a via coord marks the transition rung. Serialising as the enum's
/// stable lowercase string (`plane` / `bridge` / `via`) keeps the JSON
/// wire form small and matches the vocabulary spec §14.5 uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RouteLayer {
    /// Ground layer — every cell coord and every un-escaped wire
    /// segment sits here. Default so a follow-up pass that grows the
    /// enum with more layers cannot silently reclassify existing
    /// `Plane` coords.
    #[default]
    Plane,
    /// Horizontal escape layer for wires the crossing-legalization pass
    /// lifted off the `Plane` to avoid a cross-net overlap.
    Bridge,
    /// Vertical transition between `Plane` and `Bridge`.
    Via,
}

impl RouteLayer {
    /// `true` for the default [`Self::Plane`] layer. Used by the
    /// `serde(skip_serializing_if = "…")` attribute on
    /// [`CellCoord::layer`] so the routed / delayed IR JSON stays
    /// byte-identical to today when no coord has escaped the plane.
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

/// Coordinate of a placed cell inside its scope's `circuit region`
/// reservation.
///
/// 1D today — the placement pass assigns `x = topological index`,
/// `y = 0`, `z = 0`, [`RouteLayer::Plane`]. The two extra axes and the
/// [`RouteLayer`] tag are reserved so the routing pass can lift the
/// layout to pseudo-2.5D (`spec/redstone` §14.5's `plane` / `via` /
/// `bridge` internal concepts) without a wire-format change. `Copy`
/// because a coordinate is a value type consumers pass around by value.
///
/// `layer` serialises only when it differs from [`RouteLayer::Plane`]
/// so pre-stage-4 JSON output (placement / routing / delay) stays
/// byte-identical to what those passes produced before the crossing-
/// legalization pass landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Reserved for the routing pass. Always `0` at this stage.
    pub y: u32,
    /// Reserved for the routing pass. Always `0` at this stage.
    pub z: u32,
    /// Pseudo-2.5D layer this coord lives on. Cell coords are
    /// [`RouteLayer::Plane`] by construction; the crossing-legalization
    /// pass sets buffer / wire coords to [`RouteLayer::Bridge`] or
    /// [`RouteLayer::Via`] when it escapes cross-net overlaps.
    /// Serialised only when it differs from the default so the routed /
    /// delayed IR JSON is unchanged for scopes with no crossings.
    #[serde(skip_serializing_if = "RouteLayer::is_plane")]
    pub layer: RouteLayer,
}

impl CellCoord {
    /// [`RouteLayer::Plane`] coord at the given axis position. The
    /// most common construction path — used by every placement /
    /// routing / delay call site so a follow-up pass that grows
    /// [`RouteLayer`] with more variants does not need to touch every
    /// literal.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        Self {
            x,
            y,
            z,
            layer: RouteLayer::Plane,
        }
    }

    /// Coord with an explicit [`RouteLayer`]. Reserved for the
    /// crossing-legalization pass — nothing else in the pipeline needs
    /// to write a non-`Plane` layer.
    #[must_use]
    pub const fn with_layer(x: u32, y: u32, z: u32, layer: RouteLayer) -> Self {
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

/// One placed cell — an [`EditionCell`] plus its assigned [`CellCoord`]
/// inside the reservation.
///
/// `wire_length`, `delay_ticks`, and `buffer_coords` are progressive
/// fields written by the routing, delay-insertion, and
/// crossing-legalization passes. The four legitimate phase states are:
///
/// | Producer                                             | `wire_length` | `delay_ticks` | `buffer_coords`           |
/// |------------------------------------------------------|---------------|---------------|---------------------------|
/// | [`crate::placement::compile_placement`] alone (Stage 1) | `None`     | `None`     | empty                     |
/// | [`crate::routing::compile_routing`]   (Stage 2)          | `Some(_)`  | `None`     | empty                     |
/// | [`crate::delay::compile_delay`]       (Stage 3)          | `Some(_)`  | `Some(_)`  | empty                     |
/// | [`crate::crossing::compile_crossing`] (Stage 4)          | `Some(_)`  | `Some(_)`  | one entry per buffer tick |
///
/// `(None, Some(_))` and any state where `buffer_coords.len()` differs
/// from the buffer tick contribution the delay pass folded into
/// `delay_ticks` are illegal by contract (delay follows routed wire
/// length per `spec/redstone` §14.4; buffer coords materialise what the
/// delay pass counted). Both `Option` fields serialise via
/// `skip_serializing_if = "Option::is_none"` and `buffer_coords` via
/// `skip_serializing_if = "Vec::is_empty"` so `--stage placement` /
/// `--stage route` / `--stage delay` JSON stays byte-identical to what
/// each of those passes produced before stage 4 landed. A follow-up PR
/// that lands routing + delay + crossing together may collapse the
/// triple into a phase-typed enum so the illegal states cannot be
/// represented; that migration is `#[non_exhaustive]`-safe because
/// every progressive field is absent from today's JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Manhattan wire length from every driver of this cell into it.
    /// `None` until the routing pass (Steiner routing, stage 2 of
    /// `spec/redstone` §14.5) fills it in;
    /// [`crate::routing::compile_routing`] rewrites it to `Some(sum of
    /// segments)` once the Steiner tree for every incoming net has been
    /// laid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wire_length: Option<u32>,
    /// Tick count implied by the routed wire length + this cell's
    /// physical realisation per `spec/redstone` §14.4. `None` until the
    /// delay-insertion pass ([`crate::delay::compile_delay`], stage 3
    /// of §14.5) fills it in; routing on its own leaves this `None`
    /// while promoting [`Self::wire_length`] to `Some(_)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ticks: Option<u32>,
    /// Coordinates of the implicit buffer repeaters the delay pass
    /// counted for this cell's driver segments. `None` until the
    /// crossing-legalization pass (stage 4 of `spec/redstone` §14.5)
    /// walks the routed IR, at which point every entry pairs a coord
    /// on the driver segment with the [`RouteLayer`] the pass chose —
    /// `Plane` when the coord fit the ground layer and `Bridge` when
    /// it had to escape a cross-net overlap. The count matches what
    /// the delay pass folded into [`Self::delay_ticks`]. Absent from
    /// the JSON wire form when empty so `--stage placement` /
    /// `--stage route` / `--stage delay` output stays byte-identical
    /// to what those passes produced before stage 4 landed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buffer_coords: Vec<CellCoord>,
    /// Byte range of the originating `logic ...` sub-expression, inherited
    /// from the source [`crate::edition_netlist_ir::EditionCellNode`].
    #[serde(skip)]
    pub span: Span,
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
