//! Walkway routing and voxelisation for site `connect` rows.
//!
//! Each `connect from.port to to.port path=@MAT` row lays a one-block-wide
//! strip between the two ports [`super::ports`] placed in world space:
//!
//! 1. [`l_path`] walks a Manhattan L (x first, then z) at a constant Y.
//!    When the L collides with a structure, [`route_path`] searches the
//!    ground plane for a deterministic shortest detour instead.
//! 2. [`build_walkway_array`] turns the path into a [`BlockArray`] over the
//!    strip's bounding box, with the world origin the lockfile pins. Cells
//!    overlapping a structure are skipped and counted for the row's one
//!    `W_WALKWAY_BLOCKED`, which with [`route_path`] in front happens only
//!    when no clear route exists.
//!
//! The walkway sits at the two ports' shared Y; 3D search (staircases,
//! multi-level walkways) is out of scope.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::ids::WalkwayScopeKey;

use super::{BlockArray, BlockState, Dims, Palette, PaletteIndex};

/// Output of [`build_walkway_array`].
#[derive(Debug, Clone, PartialEq)]
pub struct WalkwayLayout {
    /// The voxelised walkway.
    pub array: BlockArray,
    /// Absolute `(x, y, z)` origin the [`BlockArray`] lives at — the
    /// `(min_x, port_y, min_z)` corner of the bounding box.
    pub origin: (i32, i32, i32),
    /// Number of cells skipped because they overlapped a placement floor.
    pub blocked_count: usize,
}

/// Walk a Manhattan L between two world voxels at a fixed Y, x-axis
/// first then z-axis. Every coordinate, the corner and both endpoints
/// included, appears in the returned `Vec` exactly once.
///
/// The whole strip takes `from`'s Y; the caller checks the two agree, and
/// asks [`l_path_area`] against [`ROUTE_AREA_CAP`] first.
#[must_use]
pub fn l_path(from: (i32, i32, i32), to: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
    debug_assert!(
        l_path_area(from, to) <= ROUTE_AREA_CAP,
        "l_path called past the cap; callers must ask `l_path_area` first",
    );
    let y = from.1;
    // Summed in u64 so two i32 spans cannot overflow. Only a capacity
    // hint, so a length past `usize` falls back to growing.
    let len = u64::from(from.0.abs_diff(to.0)) + u64::from(from.2.abs_diff(to.2)) + 1;
    let mut voxels: Vec<(i32, i32, i32)> = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    let (x0, z0) = (from.0, from.2);
    let (x1, z1) = (to.0, to.2);

    let mut x = x0;
    voxels.push((x, y, z0));
    let step_x: i32 = match x1.cmp(&x0) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
    };
    while x != x1 {
        x += step_x;
        voxels.push((x, y, z0));
    }

    // The z leg steps before it pushes, skipping the corner the x leg
    // laid, so no cell repeats without a lookup into `voxels`, which
    // would make a long north–south strip quadratic.
    let mut z = z0;
    let step_z: i32 = match z1.cmp(&z0) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
    };
    while z != z1 {
        z += step_z;
        voxels.push((x1, y, z));
    }
    voxels
}

/// Most cells a walkway's bounding box, or [`route_path`]'s search
/// rectangle, may cover. It exists so a pathological source (two ports
/// megametres apart with a pebble between them) degrades to the
/// skip-and-warn fallback instead of allocating the world.
pub const ROUTE_AREA_CAP: u64 = 4_000_000;

/// Ground-plane cells the straight L between these two ports would span,
/// as a bounding-box area.
///
/// Area, not path length, because area is what gets allocated: ports
/// `2_000_000` cells apart on each axis are a 4M-cell path over a
/// bounding box of 4x10^12. [`route_path`] measures the same area against
/// the same cap.
///
/// Saturates at `u64::MAX`, as [`RoutePathError::AreaCapExceeded`]
/// documents.
#[must_use]
pub fn l_path_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let dx = u128::from(from.0.abs_diff(to.0)) + 1;
    let dz = u128::from(from.2.abs_diff(to.2)) + 1;
    u64::try_from(dx * dz).unwrap_or(u64::MAX)
}

/// Direction of a step between neighbouring ground-plane cells, carried in
/// the search state so the cost can count turns: among equal-length routes
/// the fewest turns wins, giving long straight runs rather than a
/// staircase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StepDir {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl StepDir {
    /// The `(x, z)` cell one step away, or `None` past `i32`.
    ///
    /// A cell on the search rectangle's edge can sit at `i32::MIN` or
    /// `i32::MAX`; its neighbour beyond is outside the rectangle anyway,
    /// so the caller skips it. A start cell is never on the edge, since
    /// [`search_rect`] adds its margin around both endpoints.
    fn step(self, (x, z): (i32, i32)) -> Option<(i32, i32)> {
        match self {
            Self::PosX => Some((x.checked_add(1)?, z)),
            Self::NegX => Some((x.checked_sub(1)?, z)),
            Self::PosZ => Some((x, z.checked_add(1)?)),
            Self::NegZ => Some((x, z.checked_sub(1)?)),
        }
    }
}

/// Fixed neighbour expansion order. With the queue sequence number it
/// orders equal-cost candidates, so the same source lays the same strip.
/// Reordering changes which equal-cost detour wins (`+x` first is why
/// `village.crn`'s home1↔home3 walkway rounds home1's *east* face),
/// breaking the village pins and every lockfile holding a tie-broken
/// detour.
const STEP_DIRS: [StepDir; 4] = [StepDir::PosX, StepDir::NegX, StepDir::PosZ, StepDir::NegZ];

/// Why [`route_path`] found no detour: one variant per remedy, so the
/// `W_WALKWAY_BLOCKED` note names the actual problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePathError {
    /// A port cell is inside the blocked set, buried under another
    /// placement's floor; the flags say which.
    EndpointBlocked {
        /// `from` is a blocked cell.
        from_blocked: bool,
        /// `to` is a blocked cell.
        to_blocked: bool,
    },
    /// The search exhausted the rectangle without reaching `to` — the
    /// target is fully enclosed by blocked cells.
    TargetUnreachable,
    /// The search rectangle exceeds `ROUTE_AREA_CAP`: the two ports, or
    /// the placement floors on the walk plane, are too spread out to
    /// route. Carries the rectangle so the note can say what set its
    /// edges.
    AreaCapExceeded {
        /// Cells the rectangle would cover (`u64::MAX` when the
        /// span product itself overflowed).
        area: u64,
        /// The cap it exceeded, i.e. `ROUTE_AREA_CAP`.
        cap: u64,
        /// The rectangle `area` was measured on, margin included.
        rect: SearchRect,
    },
    /// Inflating the search rectangle stepped past the `i32` coordinate
    /// space (an endpoint or obstacle at `i32::MIN` / `i32::MAX`).
    CoordinateOverflow,
}

/// An inclusive rectangle of ground-plane cells, as [`route_path`]
/// searches it: bbox(blocked cells on the walk plane ∪ both endpoints),
/// inflated by one cell on every side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRect {
    /// Lowest `x` in the rectangle.
    pub min_x: i32,
    /// Highest `x` in the rectangle.
    pub max_x: i32,
    /// Lowest `z` in the rectangle.
    pub min_z: i32,
    /// Highest `z` in the rectangle.
    pub max_z: i32,
}

/// The blocked-cell set with each plane's bounding rectangle precomputed,
/// built once per lowering and shared by every `connect` row, so a row's
/// routing cost is bounded by [`ROUTE_AREA_CAP`] rather than by a scan of
/// the whole set.
pub struct BlockedIndex<'a, S: BuildHasher> {
    cells: &'a HashSet<(i32, i32, i32), S>,
    /// `y → (min_x, max_x, min_z, max_z)` over the blocked cells on
    /// that plane. Planes with no blocked cells are absent.
    plane_bounds: std::collections::HashMap<i32, (i32, i32, i32, i32)>,
}

impl<'a, S: BuildHasher> BlockedIndex<'a, S> {
    /// Index `cells` with a single linear scan.
    #[must_use]
    pub fn new(cells: &'a HashSet<(i32, i32, i32), S>) -> Self {
        let mut plane_bounds: std::collections::HashMap<i32, (i32, i32, i32, i32)> =
            std::collections::HashMap::new();
        for &(x, y, z) in cells {
            plane_bounds
                .entry(y)
                .and_modify(|(min_x, max_x, min_z, max_z)| {
                    *min_x = (*min_x).min(x);
                    *max_x = (*max_x).max(x);
                    *min_z = (*min_z).min(z);
                    *max_z = (*max_z).max(z);
                })
                .or_insert((x, x, z, z));
        }
        Self {
            cells,
            plane_bounds,
        }
    }

    fn contains(&self, cell: (i32, i32, i32)) -> bool {
        self.cells.contains(&cell)
    }

    fn plane_bounds(&self, y: i32) -> Option<(i32, i32, i32, i32)> {
        self.plane_bounds.get(&y).copied()
    }
}

/// Inflated search rectangle for [`route_path`]: bbox(blocked cells on
/// the walk plane ∪ both endpoints) + 1 cell on every side, so a route
/// can always hug the outside of the outermost obstacle.
///
/// # Errors
///
/// [`RoutePathError::CoordinateOverflow`] when the one-cell inflation
/// leaves `i32`; [`RoutePathError::AreaCapExceeded`] when the
/// rectangle covers more than [`ROUTE_AREA_CAP`] cells.
fn search_rect(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    plane_bounds: Option<(i32, i32, i32, i32)>,
) -> Result<(i32, i32, i32, i32), RoutePathError> {
    let mut min_x = from.0.min(to.0);
    let mut max_x = from.0.max(to.0);
    let mut min_z = from.2.min(to.2);
    let mut max_z = from.2.max(to.2);
    if let Some((blocked_min_x, blocked_max_x, blocked_min_z, blocked_max_z)) = plane_bounds {
        min_x = min_x.min(blocked_min_x);
        max_x = max_x.max(blocked_max_x);
        min_z = min_z.min(blocked_min_z);
        max_z = max_z.max(blocked_max_z);
    }
    let (Some(min_x), Some(max_x), Some(min_z), Some(max_z)) = (
        min_x.checked_sub(1),
        max_x.checked_add(1),
        min_z.checked_sub(1),
        max_z.checked_add(1),
    ) else {
        return Err(RoutePathError::CoordinateOverflow);
    };
    // Only the product can overflow, and saturated it still reads as
    // past the cap.
    let span_x = u64::try_from(i64::from(max_x) - i64::from(min_x) + 1)
        .expect("i32 bbox span is non-negative and fits u64");
    let span_z = u64::try_from(i64::from(max_z) - i64::from(min_z) + 1)
        .expect("i32 bbox span is non-negative and fits u64");
    let area = span_x.saturating_mul(span_z);
    if area > ROUTE_AREA_CAP {
        return Err(RoutePathError::AreaCapExceeded {
            area,
            cap: ROUTE_AREA_CAP,
            rect: SearchRect {
                min_x,
                max_x,
                min_z,
                max_z,
            },
        });
    }
    Ok((min_x, max_x, min_z, max_z))
}

/// Deterministic shortest detour between two world voxels at a shared
/// Y, avoiding blocked cells: the cells from `from` to `to` inclusive.
///
/// Dijkstra over `(cell, incoming direction)` states with the
/// lexicographic cost `(path length, turn count)`. Remaining ties go to
/// the fixed `STEP_DIRS` order and a monotonic queue sequence number,
/// never to hash order, so the lockfile is reproducible.
///
/// Only the walk plane is searched, within [`SearchRect`]: blocked cells
/// on other planes neither obstruct nor widen it. The endpoints must
/// share a Y, which a `debug_assert` checks.
///
/// # Errors
///
/// A [`RoutePathError`] naming why no detour exists. The caller falls
/// back to [`l_path`], skipping blocked cells, with a `W_WALKWAY_BLOCKED`
/// note for the variant.
///
/// # Panics
///
/// Panics when a search invariant breaks: more states or a longer path
/// than `4 * ROUTE_AREA_CAP`, or a cycle in the parent chain. Those are
/// algorithm bugs, and an `Err` would quietly swap the shortest detour
/// for the fallback.
pub fn route_path<S: BuildHasher>(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    blocked: &BlockedIndex<'_, S>,
) -> Result<Vec<(i32, i32, i32)>, RoutePathError> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};

    type Cell = (i32, i32);

    let y = from.1;
    debug_assert_eq!(
        from.1, to.1,
        "walkway ports must share a Y; the router searches a single ground plane",
    );
    let from_blocked = blocked.contains(from);
    let to_blocked = blocked.contains(to);
    if from_blocked || to_blocked {
        return Err(RoutePathError::EndpointBlocked {
            from_blocked,
            to_blocked,
        });
    }
    if from == to {
        return Ok(vec![from]);
    }

    let (min_x, max_x, min_z, max_z) = search_rect(from, to, blocked.plane_bounds(y))?;
    let in_bounds = |(x, z): (i32, i32)| x >= min_x && x <= max_x && z >= min_z && z <= max_z;

    // `best` holds the smallest `(len, turns)` per state; on a tie the
    // first-queued candidate keeps it (see the strict `<` below).
    let mut best: HashMap<(Cell, StepDir), (u32, u32)> = HashMap::new();
    let mut parent: HashMap<(Cell, StepDir), (Cell, StepDir)> = HashMap::new();
    // The heap holds `(len, turns, seq)` and `states[seq]` the matching
    // state, so entries are `Copy` and ordered without a custom `Ord`.
    // Every count (states, seq, path length) is at most
    // 4 × `ROUTE_AREA_CAP` = 16M, inside `u32`.
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut states: Vec<(Cell, StepDir)> = Vec::new();

    let start = (from.0, from.2);
    let goal = (to.0, to.2);
    for dir in STEP_DIRS {
        // Never `None` (see `StepDir::step`), but skipped rather than
        // unwrapped so this loop does not rely on `search_rect`.
        let Some(cell) = dir.step(start) else {
            continue;
        };
        if !in_bounds(cell) || blocked.contains((cell.0, y, cell.1)) {
            continue;
        }
        // First step off the port costs no turn regardless of heading.
        let cost = (1, 0);
        best.insert((cell, dir), cost);
        let seq = u32::try_from(states.len()).expect("state count bounded by 4 * ROUTE_AREA_CAP");
        states.push((cell, dir));
        heap.push(Reverse((cost.0, cost.1, seq)));
    }

    let mut goal_state: Option<(Cell, StepDir)> = None;
    while let Some(Reverse((len, turns, seq))) = heap.pop() {
        let (cell, dir) = states[usize::try_from(seq).expect("u32 fits usize")];
        // Stale: a cheaper cost for this state was queued since.
        if best.get(&(cell, dir)) != Some(&(len, turns)) {
            continue;
        }
        if cell == goal {
            goal_state = Some((cell, dir));
            break;
        }
        for next_dir in STEP_DIRS {
            let Some(next) = next_dir.step(cell) else {
                continue;
            };
            if !in_bounds(next) || blocked.contains((next.0, y, next.1)) {
                continue;
            }
            // Bounded by the state count, so `+` cannot overflow.
            let next_cost = (len + 1, turns + u32::from(next_dir != dir));
            let key = (next, next_dir);
            // Strict `<`: relaxing on equality would let a later
            // candidate re-parent an equal-cost state by heap timing.
            if best.get(&key).is_none_or(|&c| next_cost < c) {
                best.insert(key, next_cost);
                parent.insert(key, (cell, dir));
                let next_seq =
                    u32::try_from(states.len()).expect("state count bounded by 4 * ROUTE_AREA_CAP");
                states.push(key);
                heap.push(Reverse((next_cost.0, next_cost.1, next_seq)));
            }
        }
    }

    let Some(mut state) = goal_state else {
        return Err(RoutePathError::TargetUnreachable);
    };
    let mut cells = vec![(state.0.0, y, state.0.1)];
    while let Some(&prev) = parent.get(&state) {
        assert!(
            cells.len() <= states.len(),
            "walkway route reconstruction exceeded the state count — parent chain has a cycle",
        );
        cells.push((prev.0.0, y, prev.0.1));
        state = prev;
    }
    cells.push(from);
    cells.reverse();
    Ok(cells)
}

/// Lay `material` along `voxel_world`, world cells sharing one Y, in a
/// [`BlockArray`] over their bounding box. Cells in `blocked`, occupied by
/// other structures, stay air and are counted.
///
/// # Panics
///
/// Panics when `voxel_world` is empty, rather than placing an empty
/// walkway at a made-up origin, or when its bounding box is wider than
/// `i32::MAX` on an axis. Paths from [`l_path`] and [`route_path`] do
/// neither.
#[must_use]
pub fn build_walkway_array<S: BuildHasher>(
    voxel_world: &[(i32, i32, i32)],
    material: BlockState,
    blocked: &HashSet<(i32, i32, i32), S>,
    scope_key: &WalkwayScopeKey,
) -> WalkwayLayout {
    let first = voxel_world
        .first()
        .copied()
        .unwrap_or_else(|| panic!("walkway voxel_world is empty for scope `{scope_key}`"));
    let mut min_x = first.0;
    let mut max_x = first.0;
    let mut min_z = first.2;
    let mut max_z = first.2;
    for &(x, _, z) in voxel_world {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    // Unreachable panics, but they name the scope so one is locatable.
    let dx = u32::try_from(max_x - min_x + 1)
        .unwrap_or_else(|_| panic!("walkway `{scope_key}` x span exceeds u32 ({min_x}..={max_x})"));
    let dz = u32::try_from(max_z - min_z + 1)
        .unwrap_or_else(|_| panic!("walkway `{scope_key}` z span exceeds u32 ({min_z}..={max_z})"));
    let dims = Dims { x: dx, y: 1, z: dz };
    let origin = (min_x, first.1, min_z);

    let mut palette = Palette::new_with_air();
    let mat_idx = palette.intern(material);
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    let mut blocked_count: usize = 0;
    for &(wx, wy, wz) in voxel_world {
        if blocked.contains(&(wx, wy, wz)) {
            blocked_count += 1;
            continue;
        }
        let lx = u32::try_from(wx - min_x)
            .unwrap_or_else(|_| panic!("walkway `{scope_key}` cell x={wx} below min={min_x}"));
        let lz = u32::try_from(wz - min_z)
            .unwrap_or_else(|_| panic!("walkway `{scope_key}` cell z={wz} below min={min_z}"));
        if let Some(i) = dims.index(lx, 0, lz) {
            voxels[i] = mat_idx;
        }
    }
    let mut array = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: scope_key.as_str().to_owned(),
    };
    // A no-op for one material, but it keeps the order
    // `BlockArray::palette` documents true by construction should a
    // walkway gain a second (kerbs, railings).
    array.canonicalize_palette();
    WalkwayLayout {
        array,
        origin,
        blocked_count,
    }
}

#[cfg(test)]
mod tests;
