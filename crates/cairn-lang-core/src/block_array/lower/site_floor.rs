//! The floor cells the placed bodies of a run cover, which a `connect`
//! strip routes around.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::error::Span;

use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockArray, PaletteIndex, Placement};

/// One `place` row that lowered, and the masonry its body lowered with.
///
/// The column travels beside the placement rather than in a map of its
/// own so the two cannot disagree about which bodies exist: a key in one
/// and not the other would be a `connect` port judged against a body
/// that is in no artifact, and two `place` rows sharing an `id=` is the
/// shape that produces it — the resolver refuses the duplicate, the
/// second body still lowers, and only the first is kept.
///
/// Not a field on [`Placement`]: that record is hashed into
/// `resolved_ir_hash`, and the rows a wall occupies must not move it.
/// The lockfile's own `LockPlacement` is a named projection that would
/// not carry them anyway, and [`WallColumn`] derives no `Serialize`, so
/// the question would not compile before it could be decided.
pub(super) struct PlacedBody {
    pub(super) placement: Placement,
    /// The rows this body's `walls` painted — the value the openings
    /// phase cut against. A `connect` row anchoring a port here reads
    /// it, so the port and the cut ask the wall one question.
    pub(super) walls: WallColumn,
    /// The doors and windows the openings phase cut, by member span. A
    /// port on any other opening is refused: whatever stopped the cut, the
    /// strip would end against the wall.
    pub(super) cut: HashSet<Span>,
}

/// The walk plane the placements occupy: every cell a walkway may not
/// overwrite, who lays each one, and how far each placement's share of it
/// reaches.
pub(super) struct FloorPlan {
    /// World-space `(x, y, z)` of every non-air voxel on the y=0 plane of
    /// every placement. The walkway voxeliser uses this set to skip cells
    /// that would overwrite an existing floor tile — a strip ducking under
    /// a corner of a building still completes, but the colliding cell
    /// stays air and the row earns a `W_WALKWAY_BLOCKED` warning.
    pub(super) cells: HashSet<(i32, i32, i32)>,
    /// For each cell in [`Self::cells`], what lays it, in placement
    /// order. Recorded as the cell is
    /// inserted, so a note naming what covers a port reads the same
    /// derivation the router was blocked by.
    pub(super) owners: HashMap<(i32, i32, i32), Vec<FloorOwner>>,
    /// Per placement that put at least one cell in [`Self::cells`], under
    /// its `site::SITE::PLACE_ID` key, the box those cells span. The
    /// router's search rectangle is the union of these boxes on its plane
    /// together with the two port cells, inflated by one cell on every
    /// side (see `walkway::search_rect`), so a note about that rectangle
    /// can name the placements that stretched it.
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
            // INVARIANT: `lower_site` inserts each key into `placed` and
            // into `structures` together, first write winning in both, and
            // nothing removes one before this runs.
            debug_assert!(false, "placement `{key}` has no structure");
            continue;
        };
        // Only the y=0 plane matters: walkways sit at the ports' shared
        // Y (=0 for every example). 3D path search (staircases, multi-level
        // walkways) is intentionally out of scope so the port surface lands
        // in one piece.
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                let Some(i) = ba.dims.index(x, 0, z) else {
                    // INVARIANT: the loop keeps `x` and `z` inside `dims`,
                    // and a lowered body is at least one cell tall, so its
                    // row 0 exists.
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
                // INVARIANT: `PlaceAnchor::origin` refuses a row whose body
                // reaches past `i32`, so every cell of a placed body has a
                // world coordinate. Loud in debug builds; a release build
                // skips the cell. Saturating it instead would fold every
                // column past the range onto the edge cell, which would then
                // read as laid by whichever of them was not air.
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
