//! Walkway routing and voxelisation for site `connect` rows.
//!
//! Each `connect from.port to to.port path=@MAT` row lays a 1-block-wide
//! gravel-like strip between the two named ports, once [`super::ports`] has
//! placed them in world space. The work splits into two pieces, each
//! unit-tested in isolation so a coordinate sign error or off-by-one fails
//! loudly here rather than only at the full-village integration boundary:
//!
//! 1. [`l_path`] walks a Manhattan L (x-axis first, then z-axis) between
//!    two world voxels at a constant Y; every coordinate appears once,
//!    the corner included. When the L collides with an
//!    existing structure, [`route_path`] searches the ground plane for a
//!    deterministic shortest detour around the obstacle instead.
//! 2. [`build_walkway_array`] turns the path into a [`BlockArray`] whose
//!    voxel grid bounds the strip's bounding box, returning the world-
//!    space origin so the lockfile can pin where the array lives. Cells
//!    that overlap an existing structure (`blocked` in the signature)
//!    are skipped and counted so the caller can emit one
//!    `W_WALKWAY_BLOCKED` warning per row — with [`route_path`] in
//!    front, that only happens when no unobstructed route exists at all.
//!
//! The walkway always sits at the two ports' shared Y. 3D path search
//! (staircases, multi-level walkways) is intentionally out of scope so
//! the port surface lands in one piece; every shipping example lays its
//! walkways flat against `y = 0`.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::ids::WalkwayScopeKey;

use super::{BlockArray, BlockState, Dims, Palette, PaletteIndex};

/// Output of [`build_walkway_array`].
///
/// Bundles the lowered [`BlockArray`], the world-space origin the array
/// pins to, and the number of cells the lay-pass skipped because they
/// collided with an existing structure. Named struct rather than a
/// bare `(BlockArray, (i32, i32, i32), usize)` so a future axis-order
/// shuffle or extra return value (e.g. per-cell skip mask) cannot
/// silently re-bind callers to the wrong slot.
#[derive(Debug, Clone, PartialEq)]
pub struct WalkwayLayout {
    /// The voxelised walkway.
    pub array: BlockArray,
    /// Absolute `(x, y, z)` origin the [`BlockArray`] lives at — the
    /// `(min_x, port_y, min_z)` corner of the bounding box.
    pub origin: (i32, i32, i32),
    /// Number of cells dropped because they overlapped a placement
    /// floor (one per `W_WALKWAY_BLOCKED` collision).
    pub blocked_count: usize,
}

/// Walk a Manhattan L between two world voxels at a fixed Y, x-axis
/// first then z-axis. Every coordinate, the corner included, appears in
/// the returned `Vec` exactly once.
///
/// The two endpoints are included in the output. Caller is expected to
/// have already validated that `from.1 == to.1`; mismatched Y values
/// would still produce a connected path, just landing at the `from`
/// Y for the whole strip.
#[must_use]
pub fn l_path(from: (i32, i32, i32), to: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
    debug_assert!(
        l_path_area(from, to) <= ROUTE_AREA_CAP,
        "l_path called past the cap; callers must ask `l_path_area` first",
    );
    let y = from.1;
    // The path is exactly `|dx| + |dz| + 1` cells long. Summed in u64 so
    // two i32 spans cannot overflow; the size is only a capacity hint, so
    // a length that does not fit `usize` falls back to growing.
    let len = u64::from(from.0.abs_diff(to.0)) + u64::from(from.2.abs_diff(to.2)) + 1;
    let mut voxels: Vec<(i32, i32, i32)> = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    let (x0, z0) = (from.0, from.2);
    let (x1, z1) = (to.0, to.2);

    // x-axis leg: walk from (x0, z0) to (x1, z0), inclusive.
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

    // z-axis leg: walk from (x1, z0) toward (x1, z1). The cell at
    // (x1, z0) is the corner already laid down at the end of the
    // x-leg, so the loop steps z BEFORE pushing. That order is the
    // whole corner dedup: every x-leg cell has `z == z0` and every
    // z-leg cell has `z != z0`, so no cell can appear twice and no
    // lookup into `voxels` is needed. A lookup would be a linear scan
    // per step, which makes a long north–south strip quadratic.
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

/// Upper bound on the search rectangle, in cells, that [`route_path`]
/// is willing to explore. The rectangle is the bounding box of the
/// blocked cells on the walk plane plus the two endpoints, inflated by
/// one cell — for every shipping example that is a few hundred cells.
/// The cap only exists so a pathological source (two ports megametres
/// apart with a pebble between them) degrades to the skip-and-warn
/// fallback instead of allocating the world.
pub const ROUTE_AREA_CAP: u64 = 4_000_000;

/// Ground-plane cells the straight L between these two ports would span,
/// as a bounding-box area.
///
/// Area, not path length, because area is what gets allocated:
/// `build_walkway_array` sizes its voxel buffer from the bounding box, and
/// `route_path` measures the same quantity against the same cap. A pair
/// `2_000_000` cells apart on each axis has a path length of 4M — inside a
/// length-based bound — and a bounding box of 4x10^12.
///
/// Saturates at `u64::MAX` if the product overflows, which is the sentinel
/// [`RoutePathError::AreaCapExceeded`] already documents for that field.
#[must_use]
pub fn l_path_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let dx = u128::from(from.0.abs_diff(to.0)) + 1;
    let dz = u128::from(from.2.abs_diff(to.2)) + 1;
    u64::try_from(dx * dz).unwrap_or(u64::MAX)
}

/// Direction of travel between two 4-neighbour ground-plane cells.
/// Carried in the search state so the cost function can count turns:
/// among equal-length routes the fewest-turns one wins, which keeps
/// the laid strip looking like a hand-drawn path (long straight runs)
/// instead of a staircase zigzag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StepDir {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl StepDir {
    /// The `(x, z)` cell one step from this cell in this direction, or
    /// `None` when that step leaves `i32`. [`search_rect`] only
    /// guarantees that the inflated rectangle itself fits in `i32`, so
    /// a cell on its edge can sit at `i32::MIN` or `i32::MAX`; the
    /// neighbour past that edge is outside the rectangle anyway, and
    /// the caller skips it like any other out-of-bounds cell.
    ///
    /// Only the expansion loop in [`route_path`] can see `None`.
    /// [`search_rect`] folds both endpoints into the rectangle before
    /// adding the margin, so a start cell is never on the rectangle's
    /// edge and its first steps all stay inside `i32`.
    fn step(self, (x, z): (i32, i32)) -> Option<(i32, i32)> {
        match self {
            Self::PosX => Some((x.checked_add(1)?, z)),
            Self::NegX => Some((x.checked_sub(1)?, z)),
            Self::PosZ => Some((x, z.checked_add(1)?)),
            Self::NegZ => Some((x, z.checked_sub(1)?)),
        }
    }
}

/// Fixed neighbour expansion order. Part of the determinism contract:
/// together with the monotonic queue sequence number it fully orders
/// equal-cost candidates, so the same source always lowers to the same
/// strip and the lockfile stays reproducible. Reordering the variants
/// changes which of two equal-cost detours wins — `+x` first is why
/// `village.crn`'s home1↔home3 walkway rounds home1's *east* face —
/// so a shuffle here breaks the village integration pins and every
/// lockfile that recorded a tie-broken detour.
const STEP_DIRS: [StepDir; 4] = [StepDir::PosX, StepDir::NegX, StepDir::PosZ, StepDir::NegZ];

/// Why [`route_path`] could not produce a detour. Each variant maps to
/// a different author-facing remedy, so the caller can write a
/// `W_WALKWAY_BLOCKED` note that names the actual problem instead of
/// suggesting a gap widening that may not help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePathError {
    /// One or both port cells are themselves inside the blocked set —
    /// a port buried under another placement's floor. No search can
    /// leave (or reach) a buried cell, so the flags say which side to
    /// fix.
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
    /// route. Carries the offending area so the caller can surface both
    /// numbers, and the rectangle it measured so the caller can say what
    /// set its edges.
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

/// Pre-indexed view of the blocked-cell set, built **once** per
/// lowering with [`BlockedIndex::new`] and shared by every `connect`
/// row. [`route_path`] needs the bounding rectangle of the blocked
/// cells on its walk plane; deriving that inside the router would
/// re-scan the whole set per row, letting a large site with many
/// colliding rows multiply one linear scan into billions of iterations
/// — the pre-computed per-plane bounds keep the router's per-row cost
/// bounded by `ROUTE_AREA_CAP` alone.
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
/// can always hug the outside of the outermost obstacle. Rejects a
/// rectangle that steps past the `i32` coordinate space or covers more
/// than [`ROUTE_AREA_CAP`] cells.
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
    // Spans fit u64 by construction (an i32 range is at most 2^32
    // wide); only the *product* can overflow, and an overflowing
    // product is by definition far past the cap — saturating to
    // u64::MAX keeps the reported area honest about "too big".
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
/// Y, avoiding blocked cells. The fallback [`l_path`] cannot route
/// around obstacles; this search can, so `connect` rows whose straight
/// L would cut through a placement floor still lay an unbroken strip.
///
/// The search is Dijkstra over `(cell, incoming direction)` states with
/// the lexicographic cost `(path length, turn count)` — shortest first,
/// and among equal-length routes the one with the fewest direction
/// changes. Ties beyond that are broken by the fixed `STEP_DIRS`
/// expansion order and a monotonic queue sequence number, never by hash
/// iteration order, so the result is fully deterministic (a lockfile
/// requirement).
///
/// The searchable area is the `search_rect` rectangle: blocked cells
/// on other Y planes neither obstruct nor inflate the search. The two
/// endpoints are expected to share a Y (ports are pinned to their
/// placements' shared ground row); a mismatch is a caller bug and
/// trips a `debug_assert`.
///
/// Returns the cell sequence from `from` to `to` inclusive.
///
/// # Errors
///
/// A [`RoutePathError`] naming why no detour exists — a buried
/// endpoint, an enclosed target, the area cap, or coordinate
/// overflow; see the variant docs for the author-facing remedy each
/// one maps to. The caller is expected to fall back to [`l_path`] with
/// skipped cells and a `W_WALKWAY_BLOCKED` warning whose note reflects
/// the variant.
///
/// # Panics
///
/// Panics when an internal search invariant breaks (state count or
/// path length exceeding the `4 * ROUTE_AREA_CAP` bound, or a
/// parent-chain cycle). These are algorithm bugs, not input
/// conditions — degrading them to an `Err` would silently swap the
/// deterministic shortest detour for the skip-and-warn fallback.
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

    // The per-plane bounds come pre-computed from the index so the
    // rectangle stays O(1) per row regardless of how many cells the
    // site blocks.
    let (min_x, max_x, min_z, max_z) = search_rect(from, to, blocked.plane_bounds(y))?;
    let in_bounds = |(x, z): (i32, i32)| x >= min_x && x <= max_x && z >= min_z && z <= max_z;

    // Dijkstra over (cell, dir). `best` keeps the smallest (len, turns)
    // seen per state; on an exact cost tie the first-queued candidate
    // wins (the relaxation below never replaces on equality), which
    // pins the tie-break to the deterministic queue order.
    let mut best: HashMap<(Cell, StepDir), (u32, u32)> = HashMap::new();
    let mut parent: HashMap<(Cell, StepDir), (Cell, StepDir)> = HashMap::new();
    // The heap orders by (len, turns, seq); `states[seq]` carries the
    // matching (cell, dir) payload so the heap entries stay `Copy` and
    // totally ordered without a custom `Ord` impl. Every count below —
    // states, seq, path length — is bounded by 4 directions ×
    // ROUTE_AREA_CAP cells = 16M, comfortably inside u32, so the
    // `expect`s are unreachable unless the cap or the dedup in `best`
    // regresses; that is an algorithm bug and must fail loud (see the
    // `# Panics` section).
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut states: Vec<(Cell, StepDir)> = Vec::new();

    let start = (from.0, from.2);
    let goal = (to.0, to.2);
    for dir in STEP_DIRS {
        // Never `None` (see `StepDir::step`); skipped rather than
        // unwrapped so this loop does not depend on how `search_rect`
        // sizes the rectangle.
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
        // Stale heap entry: a cheaper cost for this state was queued
        // after this one was pushed.
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
            // `len` (and therefore `turns`) is bounded by the state
            // count, so plain `+` cannot overflow u32 — see the bound
            // note above the heap declaration.
            let next_cost = (len + 1, turns + u32::from(next_dir != dir));
            let key = (next, next_dir);
            // Strict `<` is load-bearing for determinism: relaxing on
            // equality (`<=`) would let a later-queued candidate steal
            // an equal-cost state and re-parent the path by heap
            // timing instead of the fixed queue order (see the
            // tie-break note above `best`).
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

/// Build a [`BlockArray`] from a path of world voxels and a palette
/// material, returning the world-space origin and the count of cells
/// skipped because they collided with `blocked`.
///
/// `voxel_world` is a flat list of `(x, y, z)` cells in world
/// coordinates; all cells are assumed to share a Y. `blocked` is the
/// world-space set of cells already occupied by other structures (the
/// walkway should not overwrite a wall or floor it crosses). The
/// returned `BlockArray`'s `voxels` grid is dimensioned to the bounding
/// box of `voxel_world`; collided cells stay air so the lockfile sees a
/// truthful palette.
///
/// # Panics
///
/// Panics when `voxel_world` is empty: a zero-cell walkway has no
/// meaningful bounding box, and silently producing a 1×1 placeholder at
/// `(0, 0, 0)` would let an upstream bug pin walkway IR at the wrong
/// origin. Also panics if the bounding-box span on either axis exceeds
/// `u32::MAX` (i.e. an `i32` subtraction that overflows the cast); paths
/// produced by [`l_path`] cannot exercise either condition.
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
    // Both spans are positive by construction: the min/max sweep above
    // gives `min_x ≤ max_x` and `min_z ≤ max_z`. The `u32::try_from` can
    // only fail if `max - min + 1` overflows `i32` (a span wider than
    // `i32::MAX`), which is unreachable from [`l_path`] for any realistic
    // world; surface that as a panic with the scope so a future caller
    // gets a locatable failure rather than a silent 1×1 strip.
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
        // `wx`/`wz` are members of the same min/max sweep above, so
        // `wx ≥ min_x` and `wz ≥ min_z` by construction. The same
        // overflow story as `dx`/`dz` applies — surface the cast with
        // the scope so an unreachable failure stays locatable.
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
    // A one-material walkway is already in canonical order, so this is a
    // no-op today. It is here so the order `BlockArray::palette` claims
    // holds of every array the pass produces by construction rather than
    // by that argument, and so a walkway that grows a second material
    // (kerbs, railings) does not have to remember to ask.
    array.canonicalize_palette();
    WalkwayLayout {
        array,
        origin,
        blocked_count,
    }
}

#[cfg(test)]
mod tests;
