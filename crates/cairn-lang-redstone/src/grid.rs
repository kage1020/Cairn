//! The coordinate conventions every place-and-route pass shares: where
//! the cell row and the I/O pads stand in a reservation, the orders nets
//! and coords are walked in, the steps between neighbouring coords, and
//! the cap on how far one driver segment may run.
//!
//! The placement pass lays its blocks by these and the routing, delay and
//! crossing passes measure against them, so each convention is stated
//! once, here, below every pass that reads it.

use crate::netlist_ir::NetRef;
use crate::placement_ir::{CellCoord, CircuitRegionReservation, PlacementIr};
use crate::saturating_index;

/// The row the cells stand on, counted from the near edge of the
/// reservation.
///
/// One row in, so every cell has a clear lane on each side of it rather
/// than only the one — see [`crate::placement`]. Read by the placement
/// pass's coordinate and by its depth refusal that reserves the rows it
/// needs, and by the pad coordinates that step over it, so the three
/// cannot drift.
pub(crate) const CELL_ROW: u32 = 1;

/// How a scope's edge columns lay their pads: whether they step over
/// the cell row.
///
/// The one place that decision is made. [`edge_pad`] reads it for the
/// coordinates and [`Self::rows`] for the rows those coordinates span,
/// so the placement pass's pad-row refusal, which reads the second,
/// cannot disagree with the pads it places from the first. Every pass
/// that derives a pad builds it from the IR it is working on with
/// [`Self::of`], or, in the placement pass, before that IR has cells,
/// from the netlist's cell count with [`Self::for_cell_count`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PadColumn {
    steps_over_cell_row: bool,
}

impl PadColumn {
    /// The column of a scope with `cells` cells.
    ///
    /// A scope with cells has a cell row at [`CELL_ROW`], and a pad on
    /// it would stand face to face with the end cell of the row
    /// whenever the row reaches the edge column, on a net that cell may
    /// have nothing to do with. A scope with no cells has no row to
    /// keep off, so its pads take every row from `z = 0`.
    pub(crate) fn for_cell_count(cells: usize) -> Self {
        Self {
            steps_over_cell_row: cells > 0,
        }
    }

    /// The column of the scope `ir` describes.
    pub(crate) fn of(ir: &PlacementIr) -> Self {
        Self::for_cell_count(ir.cells.len())
    }

    /// The row the `index`th pad stands in before the region clamps it:
    /// `z = index`, plus one from the cell row on when the column steps
    /// over it.
    fn z(self, index: u32) -> u32 {
        if self.steps_over_cell_row && index >= CELL_ROW {
            index.saturating_add(1)
        } else {
            index
        }
    }

    /// Rows a column of `pads` pads needs: the deepest pad's row, plus
    /// one. That is one per pad, and the cell row as well once a column
    /// that steps over it reaches it.
    ///
    /// Read by the placement pass's pad-row refusal, so the refusal and
    /// the coordinates it guards cannot disagree.
    pub(crate) fn rows(self, pads: usize) -> u32 {
        match saturating_index(pads) {
            0 => 0,
            pads => self.z(pads - 1).saturating_add(1),
        }
    }

    /// The depth rule [`Self::rows`] computes, as the fix line of every
    /// pad-row refusal states it, so the placement pass and the passes
    /// after it hand the author one sentence.
    pub(crate) fn depth_rule(self) -> &'static str {
        if self.steps_over_cell_row {
            "`depth >= max(inputs, outputs) + 1` once an edge carries two pads — one row per \
             sensor or actuator, and one for the cell row the pads step over"
        } else {
            "`depth >= max(inputs, outputs)` — one row per sensor or actuator, since a scope \
             with no cells has no cell row for the pads to step over"
        }
    }
}

/// v1 input-pad coordinate: left edge (`x=0`), first service layer
/// (`y=0`), z-axis increasing as the input index grows, laid by
/// `column` (see [`edge_pad`]). Saturates at `depth-1` when the input
/// count would push z past the region; the resulting overlap is what
/// [`collapsed_block`] finds, and every pass that lays nets surfaces it
/// as `E_ROUTE_CONGESTION` rather than a silent misroute.
///
/// [`collapsed_block`]: crate::routing_geometry::collapsed_block
pub(crate) fn input_pad(
    i: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(i, 0, column, region)
}

/// v1 output-pad coordinate: right edge (`x=width-1`), same
/// saturating z-axis convention as [`input_pad`].
pub(crate) fn output_pad(
    k: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(k, region.width.saturating_sub(1), column, region)
}

/// The `index`th pad down the edge column at `x`.
///
/// The pads step along `z` from `0`, over the cell row when `column`
/// says the scope has one, and saturate at `depth - 1`. In a scope
/// with cells the saturation can put a pad back on the cell row (at
/// `depth == 2` the second pad lands on `z = 1`), but only in a region
/// the placement pass refuses; no pad that pass emits stands on the
/// cell row.
fn edge_pad(
    index: usize,
    x: u32,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    let z = column.z(saturating_index(index));
    CellCoord::new(x, 0, z.min(region.depth.saturating_sub(1)))
}

/// Deterministic net-order key matching the routing pass's tie-break:
/// `Input(_)` sorts before `Cell(_)`, then by index ascending.
pub(crate) fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// Deterministic coord order: x, then z, then y.
///
/// The same axis order [`STEPS`] gives the search and `step_towards`
/// walks a clear run in, so a list sorted by this reads in the order
/// the router laid it. Height comes last because it is the axis a
/// coord only leaves the ground for, and every pass that anchors a
/// message on "the lowest coord of these" means the one nearest the
/// origin of the plane rather than the one nearest the floor.
pub(crate) fn coord_key(coord: CellCoord) -> (u32, u32, u32) {
    (coord.x, coord.z, coord.y)
}

/// Manhattan (L¹) distance between two cell coordinates.
///
/// The search's heuristic: no clear path between two coords is shorter
/// than this, which is what makes the first sink the search reaches
/// the nearest one.
pub(crate) fn manhattan(a: CellCoord, b: CellCoord) -> u32 {
    let dx = a.x.max(b.x) - a.x.min(b.x);
    let dy = a.y.max(b.y) - a.y.min(b.y);
    let dz = a.z.max(b.z) - a.z.min(b.z);
    dx.saturating_add(dy).saturating_add(dz)
}

/// The six rectilinear steps, in the axis order the passes were built
/// around: x before z before y.
///
/// Both directions of an axis sit next to each other, so the order
/// expresses an axis preference rather than a direction preference —
/// which is what makes the search lay the same L-shaped wire wherever
/// nothing is in the way, whichever way the net runs.
pub(crate) const STEPS: [(i64, i64, i64); 6] = [
    (-1, 0, 0),
    (1, 0, 0),
    (0, 0, -1),
    (0, 0, 1),
    (0, -1, 0),
    (0, 1, 0),
];

/// The four of [`STEPS`] that stay in the coord's own plane.
const IN_PLANE: [(i64, i64, i64); 4] = [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1)];

/// The coord one step from `from`, or `None` when the step runs off the
/// `0` edge, which the unsigned coords cannot represent.
///
/// Says nothing about the reservation: [`Router::step`] adds that, and
/// [`beside`] deliberately does not — a coord outside the region is
/// still a coord no dust of this scope stands on.
///
/// [`Router::step`]: crate::routing_geometry::Router::step
pub(crate) fn stepped(from: CellCoord, (dx, dy, dz): (i64, i64, i64)) -> Option<CellCoord> {
    let axis = |value: u32, delta: i64| -> Option<u32> {
        u32::try_from(i64::from(value).checked_add(delta)?).ok()
    };
    Some(CellCoord::new(
        axis(from.x, dx)?,
        axis(from.y, dy)?,
        axis(from.z, dz)?,
    ))
}

/// The six coords one step from `coord`, every face of the block on
/// it: [`beside`] and the two straight above and below.
///
/// What a block standing on `coord` touches, as against the dust on it
/// joins, which is [`beside`]'s question.
pub(crate) fn faces(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    STEPS
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The coords a strand of dust on `coord` reaches: the four steps that
/// keep it in its own plane.
///
/// Dust joins the dust beside it, so a second net drawing on one of
/// these is one strand with the first as surely as a second net drawing
/// on `coord` itself. Up and down are not here: whether dust at `y + 1`
/// reads the dust below it depends on what stands between them, and
/// `spec/redstone` "Place-and-route" leaves that to the physical tile
/// layer.
pub(crate) fn beside(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    IN_PLANE
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The coords one layer under `coord` that are within one step of it:
/// the one directly under it, and the four diagonally under it.
///
/// Not an obstacle — [`beside`] is what the router routes by, and this
/// is what nothing routes by. Two strands a layer apart are separated
/// by the physical tile layer rather than by stage 2, so what this
/// names is the pair that layer is obliged to keep apart, and
/// [`tile_layer_pairs`] is where the obligation gets written down.
///
/// Derived from [`IN_PLANE`] rather than listed, so the rule the
/// router routes by and the rule the tile layer is handed cannot come
/// to mean different widths.
///
/// [`tile_layer_pairs`]: crate::routing_geometry::tile_layer_pairs
pub(crate) fn under(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    std::iter::once((0, -1, 0))
        .chain(IN_PLANE.map(|(dx, _, dz)| (dx, -1, dz)))
        .filter_map(move |delta| stepped(coord, delta))
}

/// v1 sanity cap on the routed length of a single driver segment. A
/// segment longer than this reads as a placement mistake, so the pass
/// that measures it refuses with `E_ATTENUATION_LIMIT` rather than
/// charge for the repeaters it would need.
///
/// It caps the dust, not the repeater chain: `repeater_sites` stands
/// a repeater earlier wherever a coord turns, climbs or forks, so a
/// route of this length can carry more repeaters than
/// `buffer_count_for_segment` gives for it.
///
/// Three measures are held against it. [`compile_delay`] applies it to
/// the segment's *routed* length — the dust the signal travels — which
/// is the cap proper. `crate::pass::lay_nets`, which all three
/// place-and-route passes call, applies it to the straight line between
/// the segment's ends, before a route is laid. And the router's search
/// stops at it: a path from the net's wire to a sink is part of that
/// sink's segment, so a coord whose cheapest path through it is longer
/// than this is not searched. The straight line is a floor on every
/// route between two coords, and the path from the wire is a floor on
/// the segment, so neither earlier answer turns away anything the delay
/// pass would have kept: they refuse strictly less than it does, and
/// save laying a route — or searching a reservation — first.
///
/// 256 steps is at least 15 buffer repeaters from a fresh source
/// (`(256 - 1) / 16`); anything past that in a single segment reads as
/// a placement mistake rather than a routing corner case in every
/// fixture the crate ships today.
///
/// [`compile_delay`]: crate::delay::compile_delay
pub const MAX_ATTENUATION_SEGMENT: u32 = 256;

#[cfg(test)]
mod tests;
