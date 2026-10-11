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
mod tests {
    use super::*;

    /// Manhattan distance between two ground-plane cells — the minimal
    /// possible number of steps, so `route_path` output length can be
    /// asserted against `manhattan + 1` cells when no detour is needed.
    fn manhattan(a: (i32, i32, i32), b: (i32, i32, i32)) -> usize {
        usize::try_from((a.0 - b.0).abs() + (a.2 - b.2).abs()).expect("non-negative")
    }

    /// Structural invariants every successful route must satisfy: the
    /// endpoints are the requested ports, consecutive cells are
    /// 4-neighbour adjacent at a constant Y, no cell repeats, and no
    /// cell collides with `blocked`.
    fn assert_route_shape(
        path: &[(i32, i32, i32)],
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        blocked: &HashSet<(i32, i32, i32)>,
    ) {
        assert_eq!(path.first(), Some(&from), "route must start at `from`");
        assert_eq!(path.last(), Some(&to), "route must end at `to`");
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert_eq!(a.1, b.1, "route must stay at a constant Y: {a:?} -> {b:?}");
            assert_eq!(
                (a.0 - b.0).abs() + (a.2 - b.2).abs(),
                1,
                "route cells must be 4-neighbour adjacent: {a:?} -> {b:?}",
            );
        }
        let mut seen = HashSet::new();
        for cell in path {
            assert!(seen.insert(*cell), "route revisits cell {cell:?}");
            assert!(
                !blocked.contains(cell),
                "route crosses blocked cell {cell:?}"
            );
        }
    }

    /// Index-and-route shorthand so each test reads as `(from, to,
    /// blocked)` without repeating the [`BlockedIndex`] construction.
    fn route(
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        blocked: &HashSet<(i32, i32, i32)>,
    ) -> Result<Vec<(i32, i32, i32)>, RoutePathError> {
        route_path(from, to, &BlockedIndex::new(blocked))
    }

    /// Number of direction changes along a path — the second component
    /// of the router's cost, re-derived so a test can pin it.
    fn turn_count(path: &[(i32, i32, i32)]) -> usize {
        path.windows(3)
            .filter(|w| {
                let d0 = (w[1].0 - w[0].0, w[1].2 - w[0].2);
                let d1 = (w[2].0 - w[1].0, w[2].2 - w[1].2);
                d0 != d1
            })
            .count()
    }

    #[test]
    fn route_path_unobstructed_is_shortest() {
        // With nothing in the way the route must not detour: the cell
        // count is exactly the Manhattan distance plus the start cell.
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        let (from, to) = ((0, 0, 0), (3, 0, 2));
        let path = route(from, to, &blocked).expect("open plane routes");
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_detours_around_a_wall() {
        // A solid wall of blocked cells across the straight line forces
        // the route around one end. Wall at x=2, z∈[-2, 2]; endpoints on
        // either side at z=0. Shortest detour: up/down to z=±3 and back
        // → 4 + manhattan extra steps. z=±3 lies *outside* the raw bbox
        // of blocked ∪ endpoints (z∈[-2, 2]) — the detour is only
        // reachable through the one-cell inflation margin, so this test
        // also pins the +1 inflation directly (an off-by-one there
        // leaves the router with no way around and fails the expect).
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("detour exists");
        assert_route_shape(&path, from, to, &blocked);
        // Manhattan is 4; rounding the wall costs 3 extra cells each way
        // (to z=3 or z=-3 and back) → 4 + 6 steps, 11 cells.
        assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
        assert!(
            path.iter().any(|c| c.2.abs() == 3),
            "the only shortest detours run through the inflated margin row, got {path:?}",
        );
    }

    #[test]
    fn route_path_prefers_fewest_turns_among_shortest_routes() {
        // Every shortest detour around the wall is 11 cells, but they
        // differ in turn count: a staircase zigzag has up to 8 turns,
        // the U along the margin row has 2. The cost's second component
        // must pick 2 — dropping `turns` from the cost (len-only
        // Dijkstra) would let heap timing pick a zigzag and the laid
        // gravel would look hand-broken.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
        assert_eq!(
            turn_count(&path),
            2,
            "shortest fewest-turns detour is a single U, got {path:?}",
        );
    }

    #[test]
    fn route_path_breaks_symmetric_ties_toward_positive_x() {
        // A wall across the z axis leaves two mirror-image shortest
        // detours: around the east end (+x) or the west end (-x), equal
        // in both length and turns. The fixed STEP_DIRS order expands
        // `PosX` first, so the east side must win — this is the same
        // tie-break that routes village.crn's home1↔home3 walkway
        // around home1's east face, pinned here in isolation so a
        // STEP_DIRS reorder fails a unit test and not just the village
        // integration pins.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for x in -2..=2 {
            blocked.insert((x, 0, 3));
        }
        let (from, to) = ((0, 0, 0), (0, 0, 6));
        let path = route(from, to, &blocked).expect("detour exists");
        assert_route_shape(&path, from, to, &blocked);
        assert!(
            path.contains(&(3, 0, 3)),
            "the +x-first expansion order must round the east end of the wall, got {path:?}",
        );
    }

    #[test]
    fn route_path_is_deterministic() {
        // Two runs over the same input must produce the identical cell
        // sequence — the lockfile pins walkway origin/dims, so a
        // hash-order-dependent tie-break would break reproducible builds.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let a = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        let b = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        assert_eq!(a, b);
    }

    #[test]
    fn route_path_is_independent_of_blocked_insertion_order() {
        // The determinism contract must not lean on hash-map iteration
        // order: two sets with the same cells but different insertion
        // orders (and therefore different `RandomState` seeds and
        // bucket layouts) must route identically. This is the guard
        // that keeps a future refactor from sneaking an iteration-order
        // dependency into the search.
        let cells: Vec<(i32, i32, i32)> = (-2..=2).map(|z| (2, 0, z)).collect();
        let forward: HashSet<(i32, i32, i32)> = cells.iter().copied().collect();
        let reverse: HashSet<(i32, i32, i32)> = cells.iter().rev().copied().collect();
        let a = route((0, 0, 0), (4, 0, 0), &forward).expect("routes");
        let b = route((0, 0, 0), (4, 0, 0), &reverse).expect("routes");
        assert_eq!(a, b);
    }

    #[test]
    fn route_path_reports_which_endpoint_is_buried() {
        // A port buried under another placement's floor cannot anchor
        // a route; the error must say which side so the caller's
        // W_WALKWAY_BLOCKED note points at the right port.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((0, 0, 0));
        blocked.insert((9, 0, 9));
        assert_eq!(
            route((0, 0, 0), (5, 0, 5), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: true,
                to_blocked: false,
            }),
        );
        assert_eq!(
            route((5, 0, 5), (9, 0, 9), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: false,
                to_blocked: true,
            }),
        );
        assert_eq!(
            route((0, 0, 0), (9, 0, 9), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: true,
                to_blocked: true,
            }),
        );
    }

    #[test]
    fn route_path_reports_enclosed_target_as_unreachable() {
        // A full ring of blocked cells around `to` leaves no route at
        // all — the search must terminate with `TargetUnreachable`
        // rather than spin.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for d in -1..=1 {
            blocked.insert((5 + d, 0, 4));
            blocked.insert((5 + d, 0, 6));
            blocked.insert((4, 0, 5 + d));
            blocked.insert((6, 0, 5 + d));
        }
        assert_eq!(
            route((0, 0, 0), (5, 0, 5), &blocked),
            Err(RoutePathError::TargetUnreachable),
        );
    }

    #[test]
    fn route_path_same_endpoints_yields_single_cell() {
        // Kept graceful rather than asserted away: `route_path` is a
        // public API, and a single-cell "route" is the honest answer
        // for coincident ports even though `lower_connects` never asks
        // (a collision-free 1-cell L never reaches the router).
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        assert_eq!(route((5, 0, 5), (5, 0, 5), &blocked), Ok(vec![(5, 0, 5)]));
    }

    #[test]
    fn route_path_ignores_blocked_cells_on_other_y_planes() {
        // `blocked` is a world-space 3D set; cells at a different Y must
        // neither obstruct the route nor inflate the search bounds.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 7, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("open at y=0");
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_mixed_y_planes_only_walk_plane_obstructs() {
        // Obstacles on the walk plane and a *longer* copy of the same
        // wall on another plane, in one set: the route must detour
        // around the y=0 wall exactly as if the y=7 cells were absent.
        // A regression that inverts (or drops) the `y` filter would see
        // the taller y=7 wall, block the z=±3 margin crossing, and
        // return a longer path.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        for z in -4..=4 {
            blocked.insert((2, 7, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("detour exists at y=0");
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
    }

    #[test]
    fn route_path_gives_up_past_the_area_cap() {
        // Endpoints so far apart that the bounding rectangle exceeds the
        // search cap must fail fast instead of allocating the world.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((1, 0, 0));
        assert!(matches!(
            route((0, 0, 0), (10_000_000, 0, 10_000_000), &blocked),
            Err(RoutePathError::AreaCapExceeded { cap, .. }) if cap == ROUTE_AREA_CAP,
        ));
    }

    #[test]
    fn route_path_area_cap_boundary_is_exclusive() {
        // Pin the `>` in the cap check from both sides. The endpoints
        // are adjacent, so the allowed case resolves in a handful of
        // heap pops even though the rectangle is at the cap — the cap
        // bounds the worst case, not every search. With `from=(0,0,0)`,
        // `to=(1,0,0)` and one far blocked cell at `(a, 0, 1997)`, the
        // inflated rectangle spans `(a+3) × 2000`: `a=1997` lands
        // exactly on the 4-million cap (allowed), `a=1998` is one
        // column past it (refused with both numbers reported).
        let at_cap: HashSet<(i32, i32, i32)> = std::iter::once((1997, 0, 1997)).collect();
        let path = route((0, 0, 0), (1, 0, 0), &at_cap).expect("area == cap is allowed");
        assert_eq!(path, vec![(0, 0, 0), (1, 0, 0)]);

        let past_cap: HashSet<(i32, i32, i32)> = std::iter::once((1998, 0, 1997)).collect();
        assert_eq!(
            route((0, 0, 0), (1, 0, 0), &past_cap),
            Err(RoutePathError::AreaCapExceeded {
                area: 4_002_000,
                cap: ROUTE_AREA_CAP,
                rect: SearchRect {
                    min_x: -1,
                    max_x: 1999,
                    min_z: -1,
                    max_z: 1998,
                },
            }),
        );
    }

    #[test]
    fn route_path_pins_the_exact_tie_broken_detour() {
        // The full cell sequence for the wall fixture, pinned once so
        // any change to the cost function, the STEP_DIRS order, or the
        // equal-cost keep-first rule shows up as a concrete
        // before/after diff instead of a distant integration failure.
        // (The bounding-box pins in `village_lower.rs` survive a
        // tie-break flip because both sides share a bbox; this pin does
        // not.)
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
        assert_eq!(
            path,
            vec![
                (0, 0, 0),
                (0, 0, 1),
                (0, 0, 2),
                (0, 0, 3),
                (1, 0, 3),
                (2, 0, 3),
                (3, 0, 3),
                (4, 0, 3),
                (4, 0, 2),
                (4, 0, 1),
                (4, 0, 0),
            ],
        );
    }

    #[test]
    fn route_path_detours_along_each_edge_of_the_coordinate_space() {
        // `search_rect` refuses a rectangle whose one-cell margin
        // leaves `i32`, so the margin itself may lie on `i32::MIN` or
        // `i32::MAX`, and the router expands the cells there. Each of
        // those cells has a neighbour past the edge, which the router
        // has to skip rather than compute.
        //
        // The fixture is laid out in local coordinates: `a` is the
        // distance from the edge and `b` runs along it. The endpoints
        // are at `a = 1`, and a wall at `b = 2` covers `a ∈ 1..=3`.
        // The only short way round is through `a = 0`, the edge itself
        // (7 cells); the far way round, through `a = 4`, takes 11.
        type Local = fn(i32, i32) -> (i32, i32, i32);
        let edges: [(&str, Local); 4] = [
            ("+x", |a, b| (i32::MAX - a, 0, b)),
            ("-x", |a, b| (i32::MIN + a, 0, b)),
            ("+z", |a, b| (b, 0, i32::MAX - a)),
            ("-z", |a, b| (b, 0, i32::MIN + a)),
        ];
        for (edge, at) in edges {
            let blocked: HashSet<(i32, i32, i32)> = (1..=3).map(|a| at(a, 2)).collect();
            let (from, to) = (at(1, 0), at(1, 4));
            let path = route(from, to, &blocked)
                .unwrap_or_else(|e| panic!("{edge}: a detour along the edge exists, got {e:?}"));
            assert_route_shape(&path, from, to, &blocked);
            assert_eq!(
                path,
                [(1, 0), (0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (1, 4)]
                    .map(|(a, b)| at(a, b))
                    .to_vec(),
                "{edge}: the route must run along the edge of the coordinate space",
            );
        }
    }

    #[test]
    fn l_path_x_then_z_dedupes_corner() {
        let path = l_path((0, 0, 0), (3, 0, 2));
        // Expected order: (0,0,0) (1,0,0) (2,0,0) (3,0,0) — x leg
        //                 (3,0,1) (3,0,2)                  — z leg
        assert_eq!(
            path,
            vec![
                (0, 0, 0),
                (1, 0, 0),
                (2, 0, 0),
                (3, 0, 0),
                (3, 0, 1),
                (3, 0, 2),
            ],
        );
    }

    #[test]
    fn l_path_negative_axes_step_backwards() {
        let path = l_path((2, 0, 1), (0, 0, -2));
        assert_eq!(
            path,
            vec![
                (2, 0, 1),
                (1, 0, 1),
                (0, 0, 1),
                (0, 0, 0),
                (0, 0, -1),
                (0, 0, -2),
            ],
        );
    }

    #[test]
    fn l_path_same_endpoints_yields_single_cell() {
        let path = l_path((5, 0, 5), (5, 0, 5));
        assert_eq!(path, vec![(5, 0, 5)]);
    }

    /// A path with no x leg is all z leg, and its first cell is still the
    /// one the x leg pushed: the stepping order alone keeps it single.
    #[test]
    fn l_path_along_z_alone_lays_each_cell_once() {
        let path = l_path((4, 0, 0), (4, 0, 3));
        assert_eq!(path, vec![(4, 0, 0), (4, 0, 1), (4, 0, 2), (4, 0, 3)]);
    }

    /// A north–south strip must not cost time quadratic in its length. A
    /// lookup into the laid cells on every z step would make it quadratic:
    /// a million-cell strip would then take hours in a debug build, where
    /// the walk without one takes milliseconds. The deadline sits orders of
    /// magnitude from both, so a slow runner cannot trip it and the
    /// quadratic walk cannot meet it; the walk runs on its own thread so a
    /// regression fails here instead of hanging the suite.
    #[test]
    fn l_path_lays_a_long_z_strip_in_less_than_quadratic_time() {
        use std::sync::mpsc::RecvTimeoutError;
        const LEN: i32 = 1_000_000;
        let (tx, rx) = std::sync::mpsc::channel();
        let walk = std::thread::spawn(move || {
            let _ = tx.send(l_path((0, 0, 0), (0, 0, LEN - 1)));
        });
        let path = match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(path) => path,
            Err(RecvTimeoutError::Timeout) => {
                panic!("a million-cell z strip was not laid within a minute")
            }
            Err(RecvTimeoutError::Disconnected) => match walk.join() {
                Err(payload) => std::panic::resume_unwind(payload),
                Ok(()) => panic!("the walk thread ended without sending a path"),
            },
        };
        assert_eq!(path.len(), usize::try_from(LEN).expect("fits"));
        assert_eq!(path.first(), Some(&(0, 0, 0)));
        assert_eq!(path.last(), Some(&(0, 0, LEN - 1)));
    }

    fn sample_key() -> WalkwayScopeKey {
        use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint};
        let site = SiteName::new("s").expect("site");
        let a = WalkwayEndpoint {
            place: PlaceId::new("a").expect("place"),
            port: PortId::new("entry").expect("port"),
        };
        let b = WalkwayEndpoint {
            place: PlaceId::new("b").expect("place"),
            port: PortId::new("entry").expect("port"),
        };
        WalkwayScopeKey::from_parts(&site, &a, &b).expect("from_parts")
    }

    #[test]
    fn build_walkway_array_fills_unblocked_cells() {
        let path = vec![(0, 0, 0), (1, 0, 0), (1, 0, 1)];
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        let layout = build_walkway_array(
            &path,
            BlockState::bare("minecraft:gravel"),
            &blocked,
            &sample_key(),
        );
        assert_eq!(layout.blocked_count, 0);
        assert_eq!(layout.origin, (0, 0, 0));
        assert_eq!(layout.array.dims, Dims { x: 2, y: 1, z: 2 });
        // Three of the four cells should hold gravel; (0,0,1) was never
        // in the path, so it stays air.
        let palette_id_at = |x: u32, z: u32| -> &str {
            let i = layout.array.dims.index(x, 0, z).expect("in-range");
            let pi = layout.array.voxels[i];
            layout.array.palette.entries[usize::from(pi.0)].id.as_str()
        };
        assert_eq!(palette_id_at(0, 0), "minecraft:gravel");
        assert_eq!(palette_id_at(1, 0), "minecraft:gravel");
        assert_eq!(palette_id_at(1, 1), "minecraft:gravel");
        assert_eq!(palette_id_at(0, 1), "minecraft:air");
    }

    #[test]
    fn build_walkway_array_skips_blocked_cells() {
        let path = vec![(0, 0, 0), (1, 0, 0), (2, 0, 0)];
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((1, 0, 0));
        let layout = build_walkway_array(
            &path,
            BlockState::bare("minecraft:gravel"),
            &blocked,
            &sample_key(),
        );
        assert_eq!(layout.blocked_count, 1);
        // Middle cell stays air despite being on the path.
        let mid = layout.array.dims.index(1, 0, 0).unwrap();
        assert_eq!(layout.array.voxels[mid], PaletteIndex::AIR);
    }
}
