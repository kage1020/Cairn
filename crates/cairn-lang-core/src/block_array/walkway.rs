//! Port resolution and walkway voxelisation for site `connect` rows.
//!
//! Each `connect from.port to to.port path=@MAT` row lays a 1-block-wide
//! gravel-like strip between the two named ports. The work splits into
//! three pieces, each unit-tested in isolation so a coordinate sign error
//! or off-by-one fails loudly here rather than only at the full-village
//! integration boundary:
//!
//! 1. [`port_world_position`] turns a `(place_origin, def, port_id)`
//!    triple into the world-voxel coordinate one block outside the
//!    member's `side=` face. Ports are exposed on `door` and `window`
//!    members; doors anchor at one of the wall-local
//!    `at=center | left | right` positions, windows at the rectangle's
//!    geometric centre (`offset + size.w / 2`). Other roles (stair,
//!    roof, …) lower silently to `None` so the caller can decide
//!    whether to fail with `W_DEFERRED_MEMBER`.
//! 2. [`l_path`] walks a Manhattan L (x-axis first, then z-axis) between
//!    two world voxels at a constant Y, deduplicating the corner cell so
//!    every coordinate appears once. When the L collides with an
//!    existing structure, [`route_path`] searches the ground plane for a
//!    deterministic shortest detour around the obstacle instead.
//! 3. [`build_walkway_array`] turns the path into a [`BlockArray`] whose
//!    voxel grid bounds the strip's bounding box, returning the world-
//!    space origin so the lockfile can pin where the array lives. Cells
//!    that overlap an existing structure ([`blocked`] in the signature)
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

use crate::ast::ValueKind;
use crate::ids::{PortId, WalkwayScopeKey};
use crate::intent::{DefIr, Member, MemberRole};

use super::openings::{WallSide, wall_length, wall_local_to_grid};
use super::wall_column::WallColumn;
use super::{BlockArray, BlockState, Dims, Palette, PaletteIndex};

/// Wall-local `v` coordinate where a port anchors. Walkways are flat
/// 1-voxel-thick strips at the placement's ground row, so every port —
/// door or window — pins to `v = 0` regardless of the member's
/// authored `y=`. Named so a future port surface (multi-level walkway,
/// raised window catwalk) shows up as an intentional re-bind here
/// rather than a stray `0` literal at the call site.
const PORT_GROUND_V: u32 = 0;

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

/// World-space `(x, y, z)` coordinate one block outside the named
/// port's wall, at the placement's ground row (`place_origin.1`).
///
/// `place_dims` carries the full inflated placement extents (interior
/// plus roof overhang on each side) so the helper can shift the
/// member's wall-local coordinate into the right world cell — the
/// building walls sit at `origin + overhang`, not at `origin`, when a
/// roof `overhang=` inflates the bounding box. The
/// `(dims.x - interior_w) / 2` derivation is the inverse of the
/// inflation [`super::lower`] does up front.
///
/// Ports anchor on two member roles:
///
/// * [`MemberRole::Door`] — wall-local `u` taken from `at=`. Three
///   named anchors are accepted: `center` (`wall_length / 2` — odd
///   widths have a unique geometric centre; even widths land at the
///   column one cell `+u` of the midpoint, the convention spec
///   `syntax.md` §5.4 names "round-half-up" and that
///   `super::lower::carve_door` uses when cutting the opening),
///   `left` (`0`, the wall-local axis origin), and `right`
///   (`wall_length - 1`, the far corner). Numeric offsets are
///   reserved for a future extension.
/// * [`MemberRole::Window`] — wall-local `u` is the rectangle's
///   geometric centre (`offset + size.w / 2`). Even-width windows
///   take the column one cell `+u` of the midpoint by the same
///   integer-division convention doors use. `sym=true` does not
///   move the port: it is taken from the primary `offset` side, which
///   is the only one whose `id=` is referenced from a `connect` row.
///   `y=` does not lift the port off the ground row either, because
///   the walkway is a 1-voxel-thick flat strip whose Y must match the
///   other endpoint (3D path search is out of scope, see module
///   doc-comment).
///
/// Returns `None` when:
///
/// * the port id does not name a member of the def,
/// * the member's role is not [`MemberRole::Door`] or
///   [`MemberRole::Window`] (stair / roof / other roles short-circuit
///   silently — port support is reserved for a future extension),
/// * the member is missing a `side=` argument or its value is not one
///   of `front` / `back` / `left` / `right`,
/// * the door is missing `at=` or carries a value other than
///   `center` / `left` / `right`,
/// * the window is missing `offset=` / `size=WxH`, or its
///   `offset + size.w` exceeds the wall length, or any row of
///   `y ..= y + size.h - 1` falls outside the rows the def's `walls`
///   members paint (so a window that would not even be carved cannot
///   anchor a walkway either),
/// * the def has no `size=` to bound the wall against,
/// * an internal arithmetic step (`checked_add` /
///   `wall_local_to_grid` bounds / `i32::try_from`) over- or
///   under-flows.
///
/// The caller is expected to map all of these into one
/// `W_DEFERRED_MEMBER` warning per `connect` row — surfacing them as
/// resolver errors would lose the resolver's nearest-match suggestion
/// machinery, and the diagnostic anchor lives on the `connect` row
/// where the user can act on it.
#[must_use]
pub fn port_world_position(
    place_origin: (i32, i32, i32),
    place_dims: Dims,
    def: &DefIr,
    port_id: &PortId,
) -> Option<(i32, i32, i32)> {
    let member = def
        .members
        .iter()
        .find(|m| m.id.as_deref() == Some(port_id.as_str()))?;
    let side = ident_value(member, "side").and_then(WallSide::from_ident)?;
    let def_size = def.size.as_ref()?;
    let interior_w = def_size.w.get();
    let interior_h = def_size.h.get();
    // Overhang inflates symmetrically on each horizontal axis, so x and
    // z agree; `.max()` is the conservative pick if a future divergence
    // sneaks in — it keeps the port outside the larger eave rather than
    // averaging into a half-inside coordinate.
    let overhang_x = place_dims.x.saturating_sub(interior_w) / 2;
    let overhang_z = place_dims.z.saturating_sub(interior_h) / 2;
    let overhang = overhang_x.max(overhang_z);
    let len = wall_length(side, interior_w, interior_h);
    let (wall_x, wall_z) = match member.role {
        MemberRole::Door => {
            let u = door_anchor_offset(member, len)?;
            door_world_xz(side, u, overhang, interior_w, interior_h, place_origin)?
        }
        MemberRole::Window => {
            // Window port also has to fit *vertically* within the
            // def's walls — otherwise the window cut itself is
            // deferred and a walkway anchored to a non-existent
            // window would land the user with a strip leading into
            // a solid wall. `wall_column_of` is empty when no walls
            // member declares a positive `height=`, which is the
            // same condition that prevents the window from being
            // voxelised.
            let wall_column = wall_column_of(def);
            let u = window_center_offset(member, len, &wall_column)?;
            window_world_xz(
                side,
                u,
                overhang,
                interior_w,
                interior_h,
                place_dims,
                place_origin,
            )?
        }
        // Stair / roof ports are reserved for a future extension.
        // Exhaustive match (no `_ =>`) so adding a new `MemberRole`
        // variant trips the non-exhaustive-patterns check instead of
        // silently being treated as "not a port".
        MemberRole::Floor
        | MemberRole::Walls
        | MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Level
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => return None,
    };
    let (nx, nz) = normal_step(side);
    Some((wall_x + nx, place_origin.1, wall_z + nz))
}

/// Walk a Manhattan L between two world voxels at a fixed Y, x-axis
/// first then z-axis. Deduplicates the corner cell so every coordinate
/// appears in the returned `Vec` exactly once.
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
    let mut voxels: Vec<(i32, i32, i32)> = Vec::new();
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
    // x-leg, so the loop steps z BEFORE pushing — every cell here
    // is fresh and the `contains` guard is a structural safety net
    // for callers that pass overlapping legs (e.g. a single-axis
    // path constructed by hand) rather than a load-bearing dedup.
    let mut z = z0;
    let step_z: i32 = match z1.cmp(&z0) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
    };
    while z != z1 {
        z += step_z;
        let cell = (x1, y, z);
        if !voxels.contains(&cell) {
            voxels.push(cell);
        }
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
/// [`ROUTE_AREA_CAP`]'s doc has always described this case ("two ports
/// megametres apart"), but only `route_path` consulted it, and `route_path`
/// runs second and only when something is in the way. An unobstructed pair
/// walked straight past: two `place` rows chained with `east_of=` and
/// `north_of=` at `gap=30000` spent 32 seconds on roughly 1.8 GB.
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
    fn delta(self) -> (i32, i32) {
        match self {
            Self::PosX => (1, 0),
            Self::NegX => (-1, 0),
            Self::PosZ => (0, 1),
            Self::NegZ => (0, -1),
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
    /// The search rectangle exceeds [`ROUTE_AREA_CAP`]; the site is too
    /// spread out to route. Carries the offending area so the caller
    /// can surface both numbers.
    AreaCapExceeded {
        /// Cells the rectangle would cover (`u64::MAX` when the
        /// span product itself overflowed).
        area: u64,
        /// The cap it exceeded, i.e. [`ROUTE_AREA_CAP`].
        cap: u64,
    },
    /// Inflating the search rectangle stepped past the `i32` coordinate
    /// space (an endpoint or obstacle at `i32::MIN` / `i32::MAX`).
    CoordinateOverflow,
}

/// Pre-indexed view of the blocked-cell set, built **once** per
/// lowering with [`BlockedIndex::new`] and shared by every `connect`
/// row. [`route_path`] needs the bounding rectangle of the blocked
/// cells on its walk plane; deriving that inside the router would
/// re-scan the whole set per row, letting a large site with many
/// colliding rows multiply one linear scan into billions of iterations
/// — the pre-computed per-plane bounds keep the router's per-row cost
/// bounded by [`ROUTE_AREA_CAP`] alone.
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
/// changes. Ties beyond that are broken by the fixed [`STEP_DIRS`]
/// expansion order and a monotonic queue sequence number, never by hash
/// iteration order, so the result is fully deterministic (a lockfile
/// requirement).
///
/// The searchable area is the [`search_rect`] rectangle: blocked cells
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
/// path length exceeding the [`ROUTE_AREA_CAP`]-derived bound, or a
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
        let (dx, dz) = dir.delta();
        let cell = (start.0 + dx, start.1 + dz);
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
            let (dx, dz) = next_dir.delta();
            let next = (cell.0 + dx, cell.1 + dz);
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
    let array = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: scope_key.as_str().to_owned(),
    };
    WalkwayLayout {
        array,
        origin,
        blocked_count,
    }
}

fn ident_value<'a>(member: &'a Member, key: &str) -> Option<&'a str> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Wall-local `u` anchor for a door port. Accepts the three named
/// anchors the spec defines for `at=`:
///
/// * `center` — `len / 2` (integer division, so even widths land at
///   the column one cell `+u` of the midpoint, matching the
///   convention `super::lower::carve_door` uses when cutting the
///   opening; spec `syntax.md` §5.4 calls this "round-half-up").
/// * `left`   — `0`, the wall-local axis origin.
/// * `right`  — `len - 1`, the far corner. The `len.saturating_sub(1)`
///   guard returns `0` for a hypothetical `len == 0` rather than
///   underflowing `u32`, but `len == 0` is unreachable in practice:
///   `DefIr.size.w` / `.h` are `NonZeroU32`, and `wall_length` is one
///   of them — so every shipping caller has `len ≥ 1` and the
///   `right` anchor lands on a valid column.
///
/// Numeric offsets (`at=N`) are reserved for a future extension and
/// fall through to `None` so the caller cascades a
/// `W_DEFERRED_MEMBER` warning rather than silently rounding to
/// centre. Returns `None` when the member is missing `at=` or carries
/// any other value.
pub(super) fn door_anchor_offset(member: &Member, len: u32) -> Option<u32> {
    let raw = member.intent_state.get("at")?;
    match &raw.value.kind {
        ValueKind::Ident(s) => match s.as_str() {
            "center" => Some(len / 2),
            "left" => Some(0),
            "right" => Some(len.saturating_sub(1)),
            _ => None,
        },
        _ => None,
    }
}

fn door_world_xz(
    side: WallSide,
    u: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    origin: (i32, i32, i32),
) -> Option<(i32, i32)> {
    let u_i = i32::try_from(u).ok()?;
    let w_i = i32::try_from(interior_w).ok()?;
    let h_i = i32::try_from(interior_h).ok()?;
    let o = i32::try_from(overhang).ok()?;
    // Composed with `checked_*`, matching `window_world_xz` and the `None`
    // contract `port_world_position` documents for both. Guarding only the
    // individual conversions left the sum unguarded, so a `place` far enough
    // out — `gap=2147483647` reaches it — panicked in a debug build and
    // wrapped in a release one, sending the router billions of cells the
    // other way.
    let (x, z) = match side {
        WallSide::Front => (
            origin.0.checked_add(o)?.checked_add(u_i)?,
            origin.2.checked_add(o)?.checked_add(h_i)?.checked_sub(1)?,
        ),
        WallSide::Back => (
            origin
                .0
                .checked_add(o)?
                .checked_add(w_i.checked_sub(1)?.checked_sub(u_i)?)?,
            origin.2.checked_add(o)?,
        ),
        WallSide::Left => (
            origin.0.checked_add(o)?,
            origin.2.checked_add(o)?.checked_add(u_i)?,
        ),
        WallSide::Right => (
            origin.0.checked_add(o)?.checked_add(w_i)?.checked_sub(1)?,
            origin
                .2
                .checked_add(o)?
                .checked_add(h_i.checked_sub(1)?.checked_sub(u_i)?)?,
        ),
    };
    Some((x, z))
}

/// Window port wall-local centre offset: `offset + size.w / 2`, with
/// two bounds checks so a window that does not fit the wall returns
/// `None` and cascades to `W_DEFERRED_MEMBER` rather than producing an
/// out-of-range world coordinate.
///
/// Horizontal bound: `offset + size.w ≤ wall_length`. The equality
/// case (`==`) is intentionally accepted — a window whose right edge
/// touches the wall's right corner still fits.
///
/// Vertical bound: every row of the rectangle, `y ..= y + size.h - 1`,
/// lies inside one course of the def's [`WallColumn`]. A `walls
/// height=H` fills the world rows `1 ..= H` — the floor slab owns row
/// `0` — so a window flush with the top course (`y + size.h == H + 1`)
/// is inside the wall and one starting on the ground plane (`y == 0`)
/// is not.
///
/// This is the predicate [`super::lower`] cuts the window with, called
/// on the column that pass builds, so a rectangle that anchors a
/// walkway and a rectangle the openings pass carves are the same set by
/// construction rather than by two limits agreeing.
fn window_center_offset(member: &Member, len: u32, wall_column: &WallColumn) -> Option<u32> {
    let offset = nonneg_int_value(member, "offset")?;
    let (sw, sh) = size_member(member, "size")?;
    let y = nonneg_int_value(member, "y")?;
    let horizontal_end = offset.checked_add(sw)?;
    if horizontal_end > len {
        return None;
    }
    if !wall_column.contains_rows(y, sh) {
        return None;
    }
    Some(offset + sw / 2)
}

/// Window-side variant of [`door_world_xz`]. Delegates to
/// [`wall_local_to_grid`] so the wall-local → grid mapping is shared
/// with the openings carved into the wall itself (`block_array::lower`
/// uses the same helper for the window cut). `v = PORT_GROUND_V` pins
/// the port to the ground row regardless of the window's authored
/// `y=`.
fn window_world_xz(
    side: WallSide,
    u: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    place_dims: Dims,
    origin: (i32, i32, i32),
) -> Option<(i32, i32)> {
    let (grid_x, _, grid_z) = wall_local_to_grid(
        side,
        u,
        PORT_GROUND_V,
        overhang,
        interior_w,
        interior_h,
        place_dims,
    )?;
    let grid_x = i32::try_from(grid_x).ok()?;
    let grid_z = i32::try_from(grid_z).ok()?;
    Some((origin.0.checked_add(grid_x)?, origin.2.checked_add(grid_z)?))
}

/// The rows the def's `walls` members occupy, mirroring
/// `super::lower::wall_column` — the port needs to know where the wall
/// is so a window port can be checked against the same masonry the
/// openings pass cuts.
///
/// Empty when no `walls` member declares a positive `height=`, the same
/// condition that prevents the openings pass from carving any door or
/// window. Only top-level `walls` members are considered: `level y=N`
/// flattening lives in `lower.rs` and is not integrated with walkway
/// port resolution yet (walkways currently only match door / window
/// ports declared directly under the def body). When a port on a
/// level-scoped door / window lands, this helper will need to walk
/// `member.children` too — and every such member carries the level's
/// `y=N` as its span offset, which is why the column is built from
/// `(offset, height)` pairs rather than from heights alone.
fn wall_column_of(def: &DefIr) -> WallColumn {
    WallColumn::from_walls(
        def.members
            .iter()
            .filter(|m| matches!(m.role, MemberRole::Walls))
            .filter_map(|m| nonneg_int_value(m, "height").map(|h| (0, h))),
    )
}

/// Strict non-negative integer reader — unlike `super::lower::nonneg_int`
/// this propagates `i64 → u32` overflow as `None` rather than clamping
/// to `u32::MAX`. The clamp is harmless for floor / wall sizing where
/// the lowered ceiling is then capped by the volume's own dims, but it
/// would be a silent-wrong-answer here: a window written with
/// `offset = 2^33` would resolve to `offset = u32::MAX` and land the
/// port at an arithmetic-overflow-or-wraparound world cell.
fn nonneg_int_value(member: &Member, key: &str) -> Option<u32> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v >= 0 => u32::try_from(*v).ok(),
        _ => None,
    }
}

fn size_member(member: &Member, key: &str) -> Option<(u32, u32)> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Size { w, h } => Some((w.get(), h.get())),
        _ => None,
    }
}

fn normal_step(side: WallSide) -> (i32, i32) {
    match side {
        WallSide::Front => (0, 1),
        WallSide::Back => (0, -1),
        WallSide::Left => (-1, 0),
        WallSide::Right => (1, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(name: &str) -> PortId {
        PortId::new(name).expect("valid port id")
    }

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
            Err(RoutePathError::AreaCapExceeded { area: _, cap }) if cap == ROUTE_AREA_CAP,
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

    #[test]
    fn port_world_position_offsets_one_block_outside_front_door() {
        // size=3x3 interior, no overhang inflation (place dims match
        // interior). center_u = wall_length / 2 = 3 / 2 = 1; door wall
        // world at (10 + 1, 0, 20 + 3 - 1) = (11, 0, 22); +1 in +z
        // direction → (11, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (11, 0, 23));
    }

    #[test]
    fn port_world_position_shifts_outward_past_roof_overhang() {
        // size=3x3 with a `+1` overhang on every horizontal side → place
        // dims (5, _, 5). Front wall world: (origin.x + overhang + u,
        // origin.z + overhang + interior_h - 1) = (10 + 1 + 1,
        // 20 + 1 + 3 - 1) = (12, 23); +1 in the +z direction puts the
        // port one block beyond the eave → (12, 0, 24).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (12, 0, 24));
    }

    #[test]
    fn port_world_position_back_side_steps_into_negative_z() {
        // size=3x3, overhang=0, center u=1. Back wall world:
        // x = origin.x + (w-1-u) = 10 + (3-1-1) = 11, z = origin.z = 20.
        // Normal step is (0, -1) → port (11, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=back at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (11, 0, 19));
    }

    #[test]
    fn port_world_position_left_side_steps_into_negative_x() {
        // size=3x3, overhang=0, center u=1. Left wall world:
        // x = origin.x = 10, z = origin.z + u = 20 + 1 = 21.
        // Normal step is (-1, 0) → port (9, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=left at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (9, 0, 21));
    }

    #[test]
    fn port_world_position_right_side_steps_into_positive_x() {
        // size=3x3, overhang=0, center u=1. Right wall world:
        // x = origin.x + (w-1) = 12, z = origin.z + (h-1-u) = 20 + 1 = 21.
        // Normal step is (+1, 0) → port (13, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=right at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (13, 0, 21));
    }

    #[test]
    fn port_world_position_window_front_resolves_to_offset_center() {
        // size=3x3, no overhang, window offset=0 size=1x1 on front wall.
        // wall_length(Front, 3, 3) = 3; u = 0 + 1/2 = 0. Wall world via
        // wall_local_to_grid: (origin.x + 0, origin.z + 3 - 1) = (10, 22);
        // +1 in the +z normal step → port (10, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_back_resolves_to_mirrored_center() {
        // size=3x3, window offset=1 size=1x1 on back wall. wall_length = 3;
        // u = 1 + 0 = 1. Back wall world: mirrored = 3 - 1 - 1 = 1,
        // (origin.x + 1, origin.z + 0) = (11, 20); -1 in the -z normal
        // step → port (11, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=back y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (11, 0, 19));
    }

    #[test]
    fn port_world_position_window_left_resolves_to_offset_center() {
        // size=3x3, window offset=1 size=1x1 on left wall. wall_length
        // (Left, 3, 3) = 3; u = 1. Left wall world: (origin.x + 0,
        // origin.z + 1) = (10, 21); -1 in the -x normal step → port
        // (9, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=left y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (9, 0, 21));
    }

    #[test]
    fn port_world_position_window_right_resolves_to_mirrored_center() {
        // size=3x3, window offset=1 size=1x1 on right wall. wall_length
        // (Right, 3, 3) = 3; u = 1. Right wall world: mirrored = 1,
        // x = origin.x + 3 - 1 = 12, z = origin.z + 1 = 21; +1 in the +x
        // normal step → port (13, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=right y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (13, 0, 21));
    }

    #[test]
    fn port_world_position_window_shifts_outward_past_roof_overhang() {
        // size=3x3 interior, place_dims=(5,_,5) for overhang=1. Window
        // offset=0 size=1x1 on front. u = 0. Wall world via
        // wall_local_to_grid with overhang=1: (origin.x + 1, origin.z +
        // 1 + 3 - 1) = (11, 23); +1 in the +z normal step → port
        // (11, 0, 24).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (11, 0, 24));
    }

    #[test]
    fn port_world_position_window_centres_on_2x2_offset() {
        // village.crn shape: size=9x7, place_dims (11,_,9) for
        // overhang=1, window id=front side=front y=2 offset=2 size=2x2.
        // wall_length(Front, 9, 7) = 9; u = 2 + 2/2 = 3. Wall world:
        // (origin.x + 1 + 3, origin.z + 1 + 7 - 1) = (origin.x + 4,
        // origin.z + 7); +1 in the +z normal step → port shifts to z+8.
        // With origin (0,0,0): port (4, 0, 8).
        let src = concat!(
            "def cottage size=9x7:\n",
            "  walls mat_slot=w height=4\n",
            "  window id=front side=front y=2 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 11, y: 1, z: 9 };
        let pos = port_world_position((0, 0, 0), dims, def, &pid("front")).expect("port resolves");
        assert_eq!(pos, (4, 0, 8));
    }

    #[test]
    fn port_world_position_window_returns_none_when_offset_size_overflows_wall() {
        // size=3x3 → wall_length(Front) = 3. offset=2 + size.w=2 = 4 > 3.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=5\n",
            "  window id=light side=front y=1 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("light")).is_none());
    }

    #[test]
    fn port_world_position_window_sym_true_uses_primary_offset() {
        // `sym=true` mirrors the cut at lowering time but the port is
        // taken from the primary `offset` side only (the rule the spec
        // calls out so a single `id=` always maps to one coordinate).
        // Same geometry as the front-resolves test, just with `sym=true`
        // tacked on; the world position must not move.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 sym=true mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_pins_y_to_ground_row_regardless_of_authored_y() {
        // `y=4` on the window must not lift the port off the ground row
        // — walkways are flat 1-voxel strips and the port Y must agree
        // with the other endpoint (door y=0). The port stays at
        // `place_origin.1`, here = 7. Walls `height=10` so the window
        // still fits vertically (`y + size.h = 5 ≤ 10`) and the
        // resolve / pin separation is the only thing under test.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=10\n",
            "  window id=light side=front y=4 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        // Geometry is identical to the front-resolves test apart from
        // the `place_origin.1` lift, so the full `(x, y, z)` triple is
        // pinned: a regression that honours `window.y` would land the
        // port at `(10, 11, 23)` instead of `(10, 7, 23)`.
        let pos =
            port_world_position((10, 7, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (10, 7, 23));
    }

    #[test]
    fn port_world_position_returns_none_for_roof_role() {
        // Roof ports are reserved; the role guard must short-circuit
        // even when `id=` matches.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  roof id=top kind=gable mat_slot=r\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("top")).is_none());
    }

    #[test]
    fn port_world_position_returns_none_for_stair_role() {
        // Stair ports are reserved; same short-circuit as roof.
        let src = concat!("def cottage size=3x3:\n", "  stair id=up at=corner\n");
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("up")).is_none());
    }

    #[test]
    fn port_world_position_window_accepts_boundary_offset_plus_size_equal_wall_length() {
        // Pin the *acceptance* edge of the horizontal bound — a window
        // whose right edge touches the wall's right corner
        // (`offset + size.w == wall_length`) must resolve. A regression
        // that tightens the check from `>` to `>=` would only fail
        // this test, not the existing overflow case.
        // size=3x3 → wall_length(Front) = 3. offset=1 + size.w=2 = 3
        // (== wall_length, so accepted). u = 1 + 2/2 = 2. Wall world:
        // (origin.x + 2, origin.z + 3 - 1) = (12, 22); +1 +z → port
        // (12, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=5\n",
            "  window id=light side=front y=1 offset=1 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (12, 0, 23));
    }

    #[test]
    fn port_world_position_window_accepts_a_rectangle_inside_the_wall() {
        // Pin the acceptance side of the *vertical* bound with a rectangle
        // strictly inside the wall (rows 2..=2 of 1..=3), so the
        // flush-with-the-top case is pinned by a test of its own and the
        // two edges cannot be re-pinned together by accident.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=2 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_returns_none_when_the_top_edge_pierces_the_wall() {
        // The window cut itself would be deferred when its top row is
        // above the wall. Anchoring a walkway to a non-existent cut would
        // leave the user with a strip running into a solid wall, so the
        // port must defer too.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=2\n",
            "  window id=light side=front y=2 offset=0 size=1x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        // `walls height=2` paints rows 1..=2; the rectangle wants 2..=3.
        assert!(port_world_position((0, 0, 0), dims, def, &pid("light")).is_none());
    }

    #[test]
    fn port_world_position_window_returns_none_when_def_has_no_walls() {
        // A `def` without a `walls` member cannot voxelise any window
        // (the openings pass has nothing to carve into). The port must
        // defer for the same reason: anchoring a walkway to a
        // never-voxelised cut would leave the strip running into
        // emptiness.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  window id=light side=front y=0 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("light")).is_none());
    }

    #[test]
    fn port_world_position_window_back_shifts_outward_past_roof_overhang() {
        // Back wall's `wall_local_to_grid` differs from Front's (it
        // mirrors `u` along `x` and pins `z = overhang`), so an overhang
        // regression on the back side would slip past the Front-only
        // overhang test. size=3x3 interior, place_dims=(5,_,5) for
        // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
        // 1 - 0 = 2; wall world (origin.x + 1 + 2, origin.z + 1) =
        // (13, 21); -1 -z normal → port (13, 0, 20).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=back y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (13, 0, 20));
    }

    #[test]
    fn port_world_position_window_right_shifts_outward_past_roof_overhang() {
        // Right wall mirrors `u` along `z` and pins `x = overhang +
        // interior_w - 1`. size=3x3 interior, place_dims=(5,_,5) for
        // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
        // 1 - 0 = 2; wall world (origin.x + 1 + 3 - 1, origin.z + 1 +
        // 2) = (13, 23); +1 +x normal → port (14, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=right y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("light")).expect("port resolves");
        assert_eq!(pos, (14, 0, 23));
    }

    #[test]
    fn port_world_position_returns_none_for_unknown_port_id() {
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("nope")).is_none());
    }

    #[test]
    fn port_world_position_door_at_left_resolves_to_origin_corner_on_front() {
        // size=3x3, no overhang. `at=left` pins u = 0 (the wall-local axis
        // origin). Front wall world: (origin.x + 0, _, origin.z + 3 - 1)
        // = (10, _, 22); +1 in the +z normal step → port (10, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_door_at_right_resolves_to_far_corner_on_front() {
        // size=3x3, no overhang. `at=right` pins u = wall_length - 1 = 2.
        // Front wall world: (origin.x + 2, _, origin.z + 3 - 1) = (12, _,
        // 22); +1 in the +z normal step → port (12, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=right\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (12, 0, 23));
    }

    #[test]
    fn port_world_position_door_back_at_left_uses_mirrored_axis() {
        // Back wall mirrors u along x (`x = w - 1 - u`), so `at=left`
        // (u = 0) lands at the far x corner: (origin.x + (3 - 1 - 0),
        // origin.z + 0) = (12, _, 20); -1 in the -z normal step → port
        // (12, 0, 19). A regression that forgets the mirror would land at
        // (10, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=back at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (12, 0, 19));
    }

    #[test]
    fn port_world_position_door_left_at_right_uses_far_z_corner() {
        // Left wall maps u to z without mirroring, so `at=right`
        // (u = interior_h - 1 = 2) lands at z = origin.z + 2 = 22.
        // x = origin.x; -1 in the -x normal step → port (9, 0, 22).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=left at=right\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (9, 0, 22));
    }

    #[test]
    fn port_world_position_door_right_at_left_mirrors_along_z() {
        // Right wall mirrors u along z (`z = h - 1 - u`), so `at=left`
        // (u = 0) lands at the far z corner: z = origin.z + 2 = 22.
        // x = origin.x + interior_w - 1 = 12; +1 in the +x normal step →
        // port (13, 0, 22). A regression that forgets the mirror would
        // land at z = 20 instead.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=right at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (13, 0, 22));
    }

    #[test]
    fn port_world_position_door_at_left_shifts_outward_past_roof_overhang() {
        // size=3x3 interior, place_dims=(5,_,5) for overhang=1.
        // `at=left` pins u = 0. Front wall world: (origin.x + overhang +
        // 0, _, origin.z + overhang + 3 - 1) = (11, _, 23); +1 in the +z
        // normal step → port (11, 0, 24). A regression that drops the
        // overhang shift would land inside the eave at z = 23.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos =
            port_world_position((10, 0, 20), dims, def, &pid("entry")).expect("port resolves");
        assert_eq!(pos, (11, 0, 24));
    }

    #[test]
    fn port_world_position_door_returns_none_for_unknown_at_value() {
        // `at=middle` is not one of `center | left | right` and must
        // cascade to `W_DEFERRED_MEMBER` via `None` rather than being
        // silently rounded to a centre value.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=middle\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, &pid("entry")).is_none());
    }
}
