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
///   `offset + size.w` exceeds the wall length, or its
///   `y + size.h` exceeds the def's walls `height=` (so a window
///   that would not even be carved cannot anchor a walkway either),
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
            // a solid wall. `wall_height_of` returns `None` when no
            // walls member declares a positive `height=`, which is
            // the same condition that prevents the window from
            // being voxelised.
            let wall_height = wall_height_of(def)?;
            let u = window_center_offset(member, len, wall_height)?;
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
const ROUTE_AREA_CAP: u64 = 4_000_000;

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
/// strip and the lockfile stays reproducible.
const STEP_DIRS: [StepDir; 4] = [StepDir::PosX, StepDir::NegX, StepDir::PosZ, StepDir::NegZ];

/// Deterministic shortest detour between two world voxels at a shared
/// Y, avoiding `blocked` cells. The fallback [`l_path`] cannot route
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
/// The searchable area is the bounding rectangle of the blocked cells
/// *on the walk plane* (`y == from.1`) plus both endpoints, inflated by
/// one cell so a route can always hug the outside of the outermost
/// obstacle. Blocked cells on other Y planes neither obstruct nor
/// inflate the search.
///
/// Returns the cell sequence from `from` to `to` inclusive, or `None`
/// when:
///
/// * either endpoint is itself a blocked cell (a port buried under
///   another placement's floor),
/// * no unobstructed route exists inside the search rectangle (the
///   target is fully enclosed),
/// * the search rectangle exceeds [`ROUTE_AREA_CAP`] cells.
///
/// The caller is expected to fall back to [`l_path`] with skipped
/// cells and a `W_WALKWAY_BLOCKED` warning on `None`.
#[must_use]
pub fn route_path<S: BuildHasher>(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    blocked: &HashSet<(i32, i32, i32), S>,
) -> Option<Vec<(i32, i32, i32)>> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};

    /// Ground-plane `(x, z)` coordinate — the Y is fixed for the whole
    /// search.
    type Cell = (i32, i32);

    let y = from.1;
    if blocked.contains(&from) || blocked.contains(&to) {
        return None;
    }
    if from == to {
        return Some(vec![from]);
    }

    // Search rectangle: bbox(blocked on this plane ∪ endpoints) + 1.
    let mut min_x = from.0.min(to.0);
    let mut max_x = from.0.max(to.0);
    let mut min_z = from.2.min(to.2);
    let mut max_z = from.2.max(to.2);
    for &(x, by, z) in blocked {
        if by != y {
            continue;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    let min_x = min_x.checked_sub(1)?;
    let max_x = max_x.checked_add(1)?;
    let min_z = min_z.checked_sub(1)?;
    let max_z = max_z.checked_add(1)?;
    let span_x = u64::try_from(i64::from(max_x) - i64::from(min_x) + 1).ok()?;
    let span_z = u64::try_from(i64::from(max_z) - i64::from(min_z) + 1).ok()?;
    if span_x.checked_mul(span_z)? > ROUTE_AREA_CAP {
        return None;
    }
    let in_bounds = |(x, z): (i32, i32)| x >= min_x && x <= max_x && z >= min_z && z <= max_z;

    // Dijkstra over (cell, dir). `best` keeps the smallest (len, turns)
    // seen per state; on an exact cost tie the first-queued candidate
    // wins (never relaxed on equality), which pins the tie-break to the
    // deterministic queue order below.
    let mut best: HashMap<(Cell, StepDir), (u32, u32)> = HashMap::new();
    let mut parent: HashMap<(Cell, StepDir), (Cell, StepDir)> = HashMap::new();
    // The heap orders by (len, turns, seq); `states[seq]` carries the
    // matching (cell, dir) payload so the heap entries stay `Copy` and
    // totally ordered without a custom `Ord` impl.
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut states: Vec<(Cell, StepDir)> = Vec::new();

    let start = (from.0, from.2);
    let goal = (to.0, to.2);
    for dir in STEP_DIRS {
        let (dx, dz) = dir.delta();
        let cell = (start.0 + dx, start.1 + dz);
        if !in_bounds(cell) || blocked.contains(&(cell.0, y, cell.1)) {
            continue;
        }
        // First step off the port costs no turn regardless of heading.
        let cost = (1, 0);
        best.insert((cell, dir), cost);
        let seq = u32::try_from(states.len()).ok()?;
        states.push((cell, dir));
        heap.push(Reverse((cost.0, cost.1, seq)));
    }

    let mut goal_state: Option<(Cell, StepDir)> = None;
    while let Some(Reverse((len, turns, seq))) = heap.pop() {
        let (cell, dir) = states[usize::try_from(seq).ok()?];
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
            if !in_bounds(next) || blocked.contains(&(next.0, y, next.1)) {
                continue;
            }
            let next_cost = (len.checked_add(1)?, turns + u32::from(next_dir != dir));
            let key = (next, next_dir);
            if best.get(&key).is_none_or(|&c| next_cost < c) {
                best.insert(key, next_cost);
                parent.insert(key, (cell, dir));
                let next_seq = u32::try_from(states.len()).ok()?;
                states.push(key);
                heap.push(Reverse((next_cost.0, next_cost.1, next_seq)));
            }
        }
    }

    let mut state = goal_state?;
    let mut cells = vec![(state.0.0, y, state.0.1)];
    while let Some(&prev) = parent.get(&state) {
        cells.push((prev.0.0, y, prev.0.1));
        state = prev;
    }
    cells.push(from);
    cells.reverse();
    Some(cells)
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
    Some(match side {
        WallSide::Front => (origin.0 + o + u_i, origin.2 + o + h_i - 1),
        WallSide::Back => (origin.0 + o + (w_i - 1 - u_i), origin.2 + o),
        WallSide::Left => (origin.0 + o, origin.2 + o + u_i),
        WallSide::Right => (origin.0 + o + w_i - 1, origin.2 + o + (h_i - 1 - u_i)),
    })
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
/// Vertical bound: `y + size.h ≤ wall_height`. The window must fit
/// inside the walls that carry it; otherwise the openings pass would
/// already defer the cut, and a walkway port for a non-existent
/// window cut would lead the strip into a solid wall. The equality
/// case is again accepted so a full-height window flush with the wall
/// top stays valid.
fn window_center_offset(member: &Member, len: u32, wall_height: u32) -> Option<u32> {
    let offset = nonneg_int_value(member, "offset")?;
    let (sw, sh) = size_member(member, "size")?;
    let y = nonneg_int_value(member, "y")?;
    let horizontal_end = offset.checked_add(sw)?;
    if horizontal_end > len {
        return None;
    }
    let vertical_end = y.checked_add(sh)?;
    if vertical_end > wall_height {
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

/// Largest `height=` declared on a `walls` member of the def, mirroring
/// the intent behind `super::lower::max_wall_top` — the port needs to
/// know how tall the wall is so a window port can fit vertically.
/// Returns `None` when no `walls` member declares a positive `height=`
/// (the same condition that prevents the openings pass from carving any
/// door or window). Only top-level `walls` members are considered:
/// `level y=N` flattening lives in `lower.rs` and is not integrated
/// with walkway port resolution yet (walkways currently only match
/// door / window ports declared directly under the def body). When a
/// port on a level-scoped door / window lands, this helper will need
/// to walk `member.children` too.
fn wall_height_of(def: &DefIr) -> Option<u32> {
    def.members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Walls))
        .filter_map(|m| nonneg_int_value(m, "height"))
        .filter(|h| *h > 0)
        .max()
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

    #[test]
    fn route_path_unobstructed_is_shortest() {
        // With nothing in the way the route must not detour: the cell
        // count is exactly the Manhattan distance plus the start cell.
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        let (from, to) = ((0, 0, 0), (3, 0, 2));
        let path = route_path(from, to, &blocked).expect("open plane routes");
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_detours_around_a_wall() {
        // A solid wall of blocked cells across the straight line forces
        // the route around one end. Wall at x=2, z∈[-2, 2]; endpoints on
        // either side at z=0. Shortest detour: up/down to z=±3 and back
        // → 4 + manhattan extra steps.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route_path(from, to, &blocked).expect("detour exists");
        assert_route_shape(&path, from, to, &blocked);
        // Manhattan is 4; rounding the wall costs 3 extra cells each way
        // (to z=3 or z=-3 and back) → 4 + 6 steps, 11 cells.
        assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
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
        let a = route_path((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        let b = route_path((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        assert_eq!(a, b);
    }

    #[test]
    fn route_path_returns_none_when_endpoint_is_blocked() {
        // A port buried under another placement's floor cannot anchor
        // a route; the caller falls back to the skip-and-warn lay.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((0, 0, 0));
        blocked.insert((9, 0, 9));
        assert!(route_path((0, 0, 0), (5, 0, 5), &blocked).is_none());
        assert!(route_path((5, 0, 5), (9, 0, 9), &blocked).is_none());
    }

    #[test]
    fn route_path_returns_none_when_target_is_enclosed() {
        // A full ring of blocked cells around `to` leaves no route at
        // all — the search must terminate with `None` rather than spin.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for d in -1..=1 {
            blocked.insert((5 + d, 0, 4));
            blocked.insert((5 + d, 0, 6));
            blocked.insert((4, 0, 5 + d));
            blocked.insert((6, 0, 5 + d));
        }
        assert!(route_path((0, 0, 0), (5, 0, 5), &blocked).is_none());
    }

    #[test]
    fn route_path_same_endpoints_yields_single_cell() {
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        assert_eq!(
            route_path((5, 0, 5), (5, 0, 5), &blocked),
            Some(vec![(5, 0, 5)]),
        );
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
        let path = route_path(from, to, &blocked).expect("open at y=0");
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_gives_up_past_the_area_cap() {
        // Endpoints so far apart that the bounding rectangle exceeds the
        // search cap must return `None` instead of allocating the world.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((1, 0, 0));
        assert!(route_path((0, 0, 0), (10_000_000, 0, 10_000_000), &blocked).is_none());
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
    fn port_world_position_window_accepts_boundary_y_plus_size_equal_wall_height() {
        // Pin the acceptance edge of the *vertical* bound.
        // `y + size.h == walls.height` (here 2 + 1 == 3) must resolve.
        // A regression that tightens to `>=` would land the port at
        // `None` and silently drop the walkway.
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
    fn port_world_position_window_returns_none_when_y_plus_size_overflows_wall_height() {
        // The window cut itself would be deferred when its top edge
        // pierces the walls (`y + size.h > walls.height`). Anchoring a
        // walkway to a non-existent cut would leave the user with a
        // strip running into a solid wall, so the port must defer too.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=2\n",
            "  window id=light side=front y=1 offset=0 size=1x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        // `y + size.h = 1 + 2 = 3 > walls.height = 2` → None.
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
