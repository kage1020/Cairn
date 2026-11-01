//! The floor cells the placed bodies of a run cover, which a `connect`
//! strip routes around.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::error::Span;

use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockArray, PaletteIndex, Placement};

/// One `place` row that lowered, with the masonry and openings its body lowered with.
///
/// Not on [`Placement`], which is hashed into `resolved_ir_hash`.
pub(super) struct PlacedBody {
    pub(super) placement: Placement,
    /// The rows this body's `walls` painted, as the openings phase cut against them.
    pub(super) walls: WallColumn,
    /// The doors and windows the openings phase cut, by member span.
    pub(super) cut: HashSet<Span>,
}

/// The walk plane the placements occupy: cells a walkway may not overwrite, and who lays each.
pub(super) struct FloorPlan {
    /// World cells of every non-air voxel on row 0 of every placement.
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

/// Collect the [`FloorPlan`] of every lowered placement.
pub(super) fn collect_floor_cells(
    structures: &IndexMap<String, BlockArray>,
    placed: &IndexMap<String, PlacedBody>,
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
        // Only row 0: the router searches the ports' shared plane alone.
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                let Some(i) = ba.dims.index(x, 0, z) else {
                    // INVARIANT: the loop stays inside `dims`, and a lowered body has a row 0.
                    debug_assert!(
                        false,
                        "cell ({x}, 0, {z}) of `{key}` is outside {:?}",
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
                        "cell ({x}, 0, {z}) of `{key}` at {:?} has no world coordinate; \
                         `PlaceAnchor::origin` should have refused the row",
                        placement.origin,
                    );
                    continue;
                };
                let cell = (wx, placement.origin.1, wz);
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
                        y: placement.origin.1,
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
