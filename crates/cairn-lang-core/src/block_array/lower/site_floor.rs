//! The cells the placed bodies of a run hold on one walk plane, which a `connect` strip on that
//! plane routes around.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::error::Span;

use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockArray, PaletteIndex, Placement};

use super::anchor::PlaceAnchor;

/// One `place` row that lowered, with the masonry and openings its body lowered with.
///
/// Not on [`Placement`], which is hashed into `resolved_ir_hash`.
pub(super) struct PlacedBody {
    pub(super) placement: Placement,
    /// Byte range of the `place` row.
    pub(super) row: Span,
    /// The origin selector the row was placed by.
    pub(super) anchor: PlaceAnchor,
    /// The rows this body's `walls` painted, as the openings phase cut against them.
    pub(super) walls: WallColumn,
    /// The doors and windows the openings phase cut, by member span.
    pub(super) cut: HashSet<Span>,
}

/// What the placements hold on one walk plane: cells a walkway may not overwrite, and who lays
/// each.
pub(super) struct FloorPlan {
    /// World cells of every non-air voxel of every placement on the plane.
    pub(super) cells: HashSet<(i32, i32, i32)>,
    /// For each cell in [`Self::cells`], what lays it, in placement order.
    pub(super) owners: HashMap<(i32, i32, i32), Vec<FloorOwner>>,
    /// The box each placement's cells span, under its `site::SITE::PLACE_ID` key.
    pub(super) extents: IndexMap<String, FloorExtent>,
}

/// One placement's voxel on a cell of [`FloorPlan::cells`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FloorOwner {
    /// The placement's index in `placed`.
    pub(super) placement: usize,
    /// The voxel, as an index into that placement's palette.
    pub(super) voxel: PaletteIndex,
}

/// The world-space box one placement's cells in [`FloorPlan::cells`] span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FloorExtent {
    /// The walk plane the cells lie on.
    pub(super) y: i32,
    pub(super) min_x: i32,
    pub(super) max_x: i32,
    pub(super) min_z: i32,
    pub(super) max_z: i32,
}

/// Collect the [`FloorPlan`] every lowered placement lays on the world row `plane`: its floor
/// on its ground row, and whatever its body holds on a row above.
pub(super) fn collect_floor_cells(
    structures: &IndexMap<String, BlockArray>,
    placed: &IndexMap<String, PlacedBody>,
    plane: i32,
) -> FloorPlan {
    let mut cells: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut owners: HashMap<(i32, i32, i32), Vec<FloorOwner>> = HashMap::new();
    let mut extents: IndexMap<String, FloorExtent> = IndexMap::new();
    for (index, (key, PlacedBody { placement, .. })) in placed.iter().enumerate() {
        let Some(ba) = structures.get(key) else {
            // INVARIANT: `lower_site` inserts each key into `placed` and `structures` together.
            debug_assert!(false, "placement `{key}` has no structure");
            continue;
        };
        // INVARIANT: a lowered body has a row 0.
        debug_assert!(ba.dims.y > 0, "row 0 of `{key}` is outside {:?}", ba.dims);
        // Only the plane: the router searches the ports' shared row alone.
        let Some(row) = u32::try_from(i64::from(plane) - i64::from(placement.origin.1))
            .ok()
            .filter(|row| *row < ba.dims.y)
        else {
            continue;
        };
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                let Some(i) = ba.dims.index(x, row, z) else {
                    // INVARIANT: the loop stays inside `dims`, `row` among them.
                    debug_assert!(
                        false,
                        "cell ({x}, {row}, {z}) of `{key}` is outside {:?}",
                        ba.dims
                    );
                    continue;
                };
                let voxel = ba.voxels[i];
                if voxel == PaletteIndex::AIR {
                    continue;
                }
                let world = |origin: i32, local: u32| {
                    i32::try_from(local)
                        .ok()
                        .and_then(|local| origin.checked_add(local))
                };
                // INVARIANT: `PlaceAnchor::origin` refused any body reaching past `i32`.
                let (Some(wx), Some(wz)) =
                    (world(placement.origin.0, x), world(placement.origin.2, z))
                else {
                    debug_assert!(
                        false,
                        "cell ({x}, {row}, {z}) of `{key}` at {:?} has no world coordinate; \
                         `PlaceAnchor::origin` should have refused the row",
                        placement.origin,
                    );
                    continue;
                };
                let cell = (wx, plane, wz);
                cells.insert(cell);
                owners.entry(cell).or_default().push(FloorOwner {
                    placement: index,
                    voxel,
                });
                extents
                    .entry(key.clone())
                    .and_modify(|e| {
                        e.min_x = e.min_x.min(wx);
                        e.max_x = e.max_x.max(wx);
                        e.min_z = e.min_z.min(wz);
                        e.max_z = e.max_z.max(wz);
                    })
                    .or_insert(FloorExtent {
                        y: plane,
                        min_x: wx,
                        max_x: wx,
                        min_z: wz,
                        max_z: wz,
                    });
            }
        }
    }
    FloorPlan {
        cells,
        owners,
        extents,
    }
}
