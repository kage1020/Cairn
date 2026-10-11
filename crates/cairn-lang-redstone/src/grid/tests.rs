//! The coordinate conventions asked directly: how far apart two coords
//! are, the orders nets and coords are walked in, where each pad stands,
//! and which coords neighbour one another.

use std::collections::HashSet;

use super::{PadColumn, beside, coord_key, input_pad, manhattan, net_ref_key, output_pad, under};
use crate::netlist_ir::NetRef;
use crate::placement_ir::CellCoord;
use crate::test_fixtures::reservation;

#[test]
fn manhattan_sums_absolute_axis_deltas() {
    assert_eq!(
        manhattan(CellCoord::new(0, 0, 0), CellCoord::new(3, 4, 5)),
        12
    );
    assert_eq!(
        manhattan(CellCoord::new(5, 5, 5), CellCoord::new(5, 5, 5)),
        0
    );
    assert_eq!(
        manhattan(CellCoord::new(3, 4, 5), CellCoord::new(0, 0, 0)),
        12
    );
}

#[test]
fn net_ref_key_orders_input_before_cell_then_by_index() {
    assert!(net_ref_key(NetRef::Input(u32::MAX)) < net_ref_key(NetRef::Cell(0)));
    assert!(net_ref_key(NetRef::Input(0)) < net_ref_key(NetRef::Input(1)));
    assert!(net_ref_key(NetRef::Cell(0)) < net_ref_key(NetRef::Cell(1)));
}

/// A scope with cells, whose pad columns step over the cell row.
const WITH_CELLS: PadColumn = PadColumn {
    steps_over_cell_row: true,
};
/// A scope with none, whose pad columns take every row.
const CELL_LESS: PadColumn = PadColumn {
    steps_over_cell_row: false,
};

#[test]
fn the_column_steps_over_the_cell_row_only_when_there_is_one() {
    assert_eq!(PadColumn::for_cell_count(0), CELL_LESS);
    assert_eq!(PadColumn::for_cell_count(1), WITH_CELLS);
    assert_eq!(PadColumn::for_cell_count(7), WITH_CELLS);
}

#[test]
fn input_pad_saturates_at_depth_minus_one() {
    let region = reservation(10, 4, 1);
    assert_eq!(input_pad(0, WITH_CELLS, &region), CellCoord::new(0, 0, 0));
    // Input #1 steps over the cell row, so no pad stands face to
    // face with the cell at the end of the row.
    assert_eq!(input_pad(1, WITH_CELLS, &region), CellCoord::new(0, 0, 2));
    assert_eq!(input_pad(2, WITH_CELLS, &region), CellCoord::new(0, 0, 3));
    // depth-1 = 3 ceilings anything past the third input.
    assert_eq!(input_pad(5, WITH_CELLS, &region), CellCoord::new(0, 0, 3));
}

#[test]
fn output_pad_sits_on_right_edge_and_saturates_z() {
    let region = reservation(4, 4, 1);
    assert_eq!(output_pad(0, WITH_CELLS, &region), CellCoord::new(3, 0, 0));
    assert_eq!(output_pad(1, WITH_CELLS, &region), CellCoord::new(3, 0, 2));
    assert_eq!(output_pad(2, WITH_CELLS, &region), CellCoord::new(3, 0, 3));
    assert_eq!(output_pad(5, WITH_CELLS, &region), CellCoord::new(3, 0, 3));
}

/// With no cell row to keep off, the pads take every row from
/// `z = 0`, and saturate the same way.
#[test]
fn a_cell_less_column_takes_every_row() {
    let region = reservation(4, 3, 1);
    for (k, z) in [(0, 0), (1, 1), (2, 2), (5, 2)] {
        assert_eq!(input_pad(k, CELL_LESS, &region), CellCoord::new(0, 0, z));
        assert_eq!(output_pad(k, CELL_LESS, &region), CellCoord::new(3, 0, z));
    }
}

/// The rows the pad-row refusal reserves are the rows the pads
/// stand in: the highest `z` of `n` pads, plus one, in a region deep
/// enough not to saturate. Asked of both columns, because the
/// refusal reads whichever one the pads were laid by.
#[test]
fn rows_counts_the_rows_the_pads_span() {
    let region = reservation(4, 100, 1);
    for column in [WITH_CELLS, CELL_LESS] {
        assert_eq!(column.rows(0), 0, "{column:?}");
        for n in 1..10 {
            let deepest = output_pad(n - 1, column, &region).z;
            assert_eq!(column.rows(n), deepest + 1, "{column:?}, {n} pads");
        }
    }
    assert_eq!(WITH_CELLS.rows(2), 3, "two pads and the cell row");
    assert_eq!(CELL_LESS.rows(2), 2, "two pads and no cell row");
}

/// In a region exactly as deep as [`PadColumn::rows`] asks, every
/// pad has a row of its own. This is the half a refusal reading one
/// rule and coordinates laid by another would break: a cell-less
/// column refused or accepted by the stepping rule would either ask
/// for a row it does not use or stack two pads on the last one.
#[test]
fn a_region_as_deep_as_rows_asks_gives_every_pad_its_own_row() {
    for column in [WITH_CELLS, CELL_LESS] {
        for n in 1..8 {
            let region = reservation(4, column.rows(n), 1);
            let pads: HashSet<CellCoord> = (0..n).map(|k| output_pad(k, column, &region)).collect();
            assert_eq!(
                pads.len(),
                n,
                "{column:?}, {n} pads, depth {}",
                region.depth
            );
        }
    }
}

/// What a strand of dust reaches is its own plane.
///
/// The four in-plane steps and no others: up and down are left to
/// the physical tile layer, because whether dust at `y + 1` reads
/// the dust below it depends on what is standing between them and
/// the pseudo-2.5D model does not carry that. Spelled out here
/// rather than derived from [`STEPS`], so widening the rule to six
/// steps has to be written down twice.
#[test]
fn a_strand_reaches_the_four_coords_in_its_own_plane() {
    let mut reached: Vec<CellCoord> = beside(CellCoord::new(2, 1, 3)).collect();
    reached.sort_by_key(|coord| coord_key(*coord));
    assert_eq!(
        reached,
        vec![
            CellCoord::new(1, 1, 3),
            CellCoord::new(2, 1, 2),
            CellCoord::new(2, 1, 4),
            CellCoord::new(3, 1, 3),
        ],
    );
    assert!(
        reached.iter().all(|coord| coord.y == 1),
        "a strand does not reach the layer above or below it: {reached:?}",
    );
    assert_eq!(
        beside(CellCoord::new(0, 0, 0)).count(),
        2,
        "and the steps off the `0` edge are not coords at all",
    );
}

/// What the tile layer is handed is one layer down, and five
/// coords wide.
///
/// The depth is the half [`beside`]'s own test cannot see: every
/// fixture in this crate fits inside `void=2`, so a rule reaching
/// two layers down would name the same pairs in all of them and
/// nothing would fail. Spelled out as coords rather than derived
/// from the deltas, for the reason the [`beside`] test is: the
/// rule the tile layer is handed has to be written down twice
/// before it can change.
#[test]
fn the_layer_handed_to_the_tile_layer_is_the_one_directly_under() {
    let mut reached: Vec<CellCoord> = under(CellCoord::new(2, 3, 3)).collect();
    reached.sort_by_key(|coord| coord_key(*coord));
    assert_eq!(
        reached,
        vec![
            CellCoord::new(1, 2, 3),
            CellCoord::new(2, 2, 2),
            CellCoord::new(2, 2, 3),
            CellCoord::new(2, 2, 4),
            CellCoord::new(3, 2, 3),
        ],
        "the coord under it and the four diagonally under it, and nothing \
         two layers down",
    );
    assert_eq!(
        under(CellCoord::new(0, 0, 0)).count(),
        0,
        "and there is no layer under the floor",
    );
}
