//! Walkways for site `connect` rows: an L between two ports, or the shortest detour round
//! whatever it hits, one block wide at the ports' Y.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::ids::WalkwayScopeKey;

use super::{BlockArray, BlockState, Dims, Palette, PaletteIndex};

/// Output of [`build_walkway_array`].
#[derive(Debug, Clone, PartialEq)]
pub struct WalkwayLayout {
    /// The voxelised walkway.
    pub array: BlockArray,
    /// World position of the array's `(0, 0, 0)` cell.
    pub origin: (i32, i32, i32),
    /// Number of cells skipped because they overlapped a placement floor.
    pub blocked_count: usize,
}

/// Walk a Manhattan L between two world voxels at `from`'s Y, x first, each cell once.
#[must_use]
pub fn l_path(from: (i32, i32, i32), to: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
    debug_assert!(
        l_path_area(from, to) <= ROUTE_AREA_CAP,
        "l_path called past the cap; callers must ask `l_path_area` first",
    );
    let y = from.1;
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

    // Stepping before pushing skips the corner the x leg laid.
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

/// Most cells a walkway's bounding box, or [`route_path`]'s search rectangle, may cover.
pub const ROUTE_AREA_CAP: u64 = 4_000_000;

/// Bounding-box area of the straight L between two ports, saturating at `u64::MAX`.
#[must_use]
pub fn l_path_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let dx = u128::from(from.0.abs_diff(to.0)) + 1;
    let dz = u128::from(from.2.abs_diff(to.2)) + 1;
    u64::try_from(dx * dz).unwrap_or(u64::MAX)
}

/// Direction of a step, carried in the search state so the cost can count turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StepDir {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl StepDir {
    /// The `(x, z)` cell one step away, or `None` past `i32`.
    fn step(self, (x, z): (i32, i32)) -> Option<(i32, i32)> {
        match self {
            Self::PosX => Some((x.checked_add(1)?, z)),
            Self::NegX => Some((x.checked_sub(1)?, z)),
            Self::PosZ => Some((x, z.checked_add(1)?)),
            Self::NegZ => Some((x, z.checked_sub(1)?)),
        }
    }
}

/// Fixed neighbour expansion order: reordering it changes which equal-cost detour lockfiles pin.
const STEP_DIRS: [StepDir; 4] = [StepDir::PosX, StepDir::NegX, StepDir::PosZ, StepDir::NegZ];

/// Why [`route_path`] found no detour, one variant per remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePathError {
    /// A port cell is under another placement's floor; the flags say which.
    EndpointBlocked {
        /// `from` is a blocked cell.
        from_blocked: bool,
        /// `to` is a blocked cell.
        to_blocked: bool,
    },
    /// The target is fully enclosed by blocked cells.
    TargetUnreachable,
    /// The search rectangle exceeds [`ROUTE_AREA_CAP`].
    AreaCapExceeded {
        /// Cells the rectangle would cover, saturating at `u64::MAX`.
        area: u64,
        /// The cap it exceeded.
        cap: u64,
        /// The rectangle `area` was measured on, margin included.
        rect: SearchRect,
    },
    /// Inflating the search rectangle stepped past `i32`.
    CoordinateOverflow,
}

/// An inclusive rectangle of ground-plane cells, as [`route_path`] searches it.
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

/// The blocked-cell set with each plane's bounding rectangle, built once per lowering.
pub struct BlockedIndex<'a, S: BuildHasher> {
    cells: &'a HashSet<(i32, i32, i32), S>,
    /// Bounds `(min_x, max_x, min_z, max_z)` of each plane's blocked cells.
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

/// The search rectangle: the walk plane's blocked cells and both endpoints, one cell wider.
///
/// # Errors
///
/// When the inflation leaves `i32`, or the rectangle covers more than [`ROUTE_AREA_CAP`] cells.
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

/// Deterministic shortest detour between two world voxels at a shared Y, avoiding blocked cells.
///
/// Ties break on turn count, then `STEP_DIRS` order and queue order, never hash order.
///
/// # Errors
///
/// A [`RoutePathError`] naming why no detour exists.
///
/// # Panics
///
/// When a search invariant breaks, such as a cycle in the parent chain.
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

    let mut best: HashMap<(Cell, StepDir), (u32, u32)> = HashMap::new();
    let mut parent: HashMap<(Cell, StepDir), (Cell, StepDir)> = HashMap::new();
    // Every count (states, seq, path length) is at most 4 × `ROUTE_AREA_CAP`, inside `u32`.
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut states: Vec<(Cell, StepDir)> = Vec::new();

    let start = (from.0, from.2);
    let goal = (to.0, to.2);
    for dir in STEP_DIRS {
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
            let next_cost = (len + 1, turns + u32::from(next_dir != dir));
            let key = (next, next_dir);
            // Strict `<`: on equality a later candidate would re-parent the state by heap timing.
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

/// Lay `material` along `voxel_world` in an array over its bounding box; `blocked` cells stay air.
///
/// # Panics
///
/// When `voxel_world` is empty or its bounding box spans every `i32` on an axis.
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
    let dx = max_x
        .abs_diff(min_x)
        .checked_add(1)
        .unwrap_or_else(|| panic!("walkway `{scope_key}` x span exceeds u32 ({min_x}..={max_x})"));
    let dz = max_z
        .abs_diff(min_z)
        .checked_add(1)
        .unwrap_or_else(|| panic!("walkway `{scope_key}` z span exceeds u32 ({min_z}..={max_z})"));
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
        if let Some(i) = dims.index(wx.abs_diff(min_x), 0, wz.abs_diff(min_z)) {
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
    // A no-op for one material; it keeps the documented palette order should walkways gain more.
    array.canonicalize_palette();
    WalkwayLayout {
        array,
        origin,
        blocked_count,
    }
}

#[cfg(test)]
mod tests;
