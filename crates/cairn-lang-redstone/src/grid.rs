//! The coordinate conventions every place-and-route pass shares: where the cell row and the pads
//! stand, the orders nets and coords are walked in, neighbour steps, and the segment cap.

use crate::netlist_ir::NetRef;
use crate::placement_ir::{CellCoord, CircuitRegionReservation, PlacementIr};
use crate::saturating_index;

/// The row the cells stand on: one in from the near edge, leaving each cell a lane either side.
pub(crate) const CELL_ROW: u32 = 1;

/// Whether a scope's edge pad columns step over the cell row, which they do whenever it has cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PadColumn {
    steps_over_cell_row: bool,
}

impl PadColumn {
    pub(crate) fn for_cell_count(cells: usize) -> Self {
        Self {
            steps_over_cell_row: cells > 0,
        }
    }

    pub(crate) fn of(ir: &PlacementIr) -> Self {
        Self::for_cell_count(ir.cells.len())
    }

    fn z(self, index: u32) -> u32 {
        if self.steps_over_cell_row && index >= CELL_ROW {
            index.saturating_add(1)
        } else {
            index
        }
    }

    /// Rows a column of `pads` pads needs.
    pub(crate) fn rows(self, pads: usize) -> u32 {
        match saturating_index(pads) {
            0 => 0,
            pads => self.z(pads - 1).saturating_add(1),
        }
    }

    /// The depth rule [`Self::rows`] computes, as a refusal's fix line states it.
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

/// Input-pad coordinate: left edge, ground layer, z by input index. Saturates at `depth - 1`, an
/// overlap the net-laying passes refuse.
pub(crate) fn input_pad(
    i: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(i, 0, column, region)
}

/// Output-pad coordinate: right edge (`x=width-1`), z as in [`input_pad`].
pub(crate) fn output_pad(
    k: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(k, region.width.saturating_sub(1), column, region)
}

fn edge_pad(
    index: usize,
    x: u32,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    let z = column.z(saturating_index(index));
    CellCoord::new(x, 0, z.min(region.depth.saturating_sub(1)))
}

pub(crate) fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// Coord order x, z, y: the axis order of [`STEPS`], so a sorted list reads as the router laid it.
pub(crate) fn coord_key(coord: CellCoord) -> (u32, u32, u32) {
    (coord.x, coord.z, coord.y)
}

/// Manhattan (L¹) distance between two cell coordinates.
pub(crate) fn manhattan(a: CellCoord, b: CellCoord) -> u32 {
    let dx = a.x.max(b.x) - a.x.min(b.x);
    let dy = a.y.max(b.y) - a.y.min(b.y);
    let dz = a.z.max(b.z) - a.z.min(b.z);
    dx.saturating_add(dy).saturating_add(dz)
}

/// The six rectilinear steps by axis, x, z, then y: the search's tie-break, so it lays the same
/// L-shaped wire whichever way a net runs.
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

/// The coord one step from `from`, or `None` off the `0` edge; the reservation is not checked.
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

/// The six coords one step from `coord`: every face of a block on it.
pub(crate) fn faces(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    STEPS
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The four in-plane coords dust on `coord` joins; up and down are left to the physical tile layer.
pub(crate) fn beside(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    IN_PLANE
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The coords one layer under `coord` within one step of it: directly under and the four
/// diagonals.
pub(crate) fn under(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    std::iter::once((0, -1, 0))
        .chain(IN_PLANE.map(|(dx, _, dz)| (dx, -1, dz)))
        .filter_map(move |delta| stepped(coord, delta))
}

/// Sanity cap on the routed length of a single driver segment; a longer one is refused with
/// `E_ATTENUATION_LIMIT`.
pub const MAX_ATTENUATION_SEGMENT: u32 = 256;

#[cfg(test)]
mod tests;
