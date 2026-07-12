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
//! Delay is still not carried at this stage. Per `spec/redstone` §14.4
//! delay is determined "for the first time in the Placement IR" once
//! wire length is known; wire length is the output of the routing pass
//! (Steiner routing, stage 2 of §14.5), so [`PlacedCellNode::wire_length`]
//! and [`PlacedCellNode::delay_ticks`] stay reserved as `Option`s that
//! this pass always leaves `None`. Adding real values in a follow-up
//! pass is a field-write, not a schema change, so downstream JSON
//! consumers see a stable wire shape today.
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

/// Coordinate of a placed cell inside its scope's `circuit region`
/// reservation.
///
/// 1D today — the placement pass assigns `x = topological index`,
/// `y = 0`, `z = 0`. The two extra axes are reserved so the routing
/// pass can lift the layout to pseudo-2.5D (`spec/redstone` §14.5's
/// `plane` / `via` / `bridge` internal concepts) without a wire-format
/// change. `Copy` because a coordinate is a value type consumers pass
/// around by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Reserved for the routing pass. Always `0` at this stage.
    pub y: u32,
    /// Reserved for the routing pass. Always `0` at this stage.
    pub z: u32,
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
/// `wire_length` and `delay_ticks` are reserved for the routing and
/// delay-insertion follow-up passes. They stay `None` at this stage and
/// serialise out via `skip_serializing_if` so the JSON wire form does
/// not grow empty `"wire_length": null` noise before the values matter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Reserved for the routing pass (Steiner routing, stage 2 of
    /// `spec/redstone` §14.5). Always `None` at this stage; the routing
    /// pass will fill it with the Manhattan wire length from each
    /// driver to this cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wire_length: Option<u32>,
    /// Reserved for the delay-insertion pass (stage 3 of `spec/redstone`
    /// §14.5). Always `None` at this stage; the delay pass will fill it
    /// with the tick count implied by the routed wire length + cell
    /// choice per §14.4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ticks: Option<u32>,
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
