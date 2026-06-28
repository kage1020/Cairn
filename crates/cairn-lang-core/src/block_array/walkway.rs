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
//!    members; doors anchor at the wall-local `at=center`, windows at
//!    the rectangle's geometric centre (`offset + size.w / 2`). Other
//!    roles (stair, roof, …) lower silently to `None` so the caller
//!    can decide whether to fail with `W_DEFERRED_MEMBER`.
//! 2. [`l_path`] walks a Manhattan L (x-axis first, then z-axis) between
//!    two world voxels at a constant Y, deduplicating the corner cell so
//!    every coordinate appears once.
//! 3. [`build_walkway_array`] turns the path into a [`BlockArray`] whose
//!    voxel grid bounds the strip's bounding box, returning the world-
//!    space origin so the lockfile can pin where the array lives. Cells
//!    that overlap an existing structure ([`blocked`] in the signature)
//!    are skipped and counted so the caller can emit one
//!    `W_WALKWAY_BLOCKED` warning per row.
//!
//! The walkway always sits at the two ports' shared Y. 3D path search
//! (staircases, multi-level walkways) is intentionally out of scope so
//! the port surface lands in one piece; every shipping example lays its
//! walkways flat against `y = 0`.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::ast::ValueKind;
use crate::ids::WalkwayScopeKey;
use crate::intent::{DefIr, Member, MemberRole};

use super::openings::{WallSide, wall_length, wall_local_to_grid};
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

/// World-space `(x, y, z)` coordinate one block outside the named
/// port's wall, at the ground row.
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
/// * [`MemberRole::Door`] — wall-local `u` taken from `at=center`
///   (`wall_length / 2`).
/// * [`MemberRole::Window`] — wall-local `u` is the rectangle's
///   geometric centre (`offset + size.w / 2`). `sym=true` does not
///   move the port: it is taken from the primary `offset` side, which
///   is the only one whose `id=` is referenced from a `connect` row.
///   `y=` does not lift the port off the ground row either, because
///   the walkway is a 1-voxel-thick flat strip whose Y must match the
///   other endpoint (3D path search is out of scope, see module
///   doc-comment).
///
/// Returns `None` when the port id does not name a door / window
/// member of the def, when the member is missing a `side=` argument,
/// when the window's `offset + size.w` exceeds the wall length, or
/// when the def has no `size=` to bound the wall against. The caller
/// is expected to map those into a `W_DEFERRED_MEMBER` warning —
/// surfacing them as resolver errors would lose the resolver's
/// nearest-match suggestion machinery.
#[must_use]
pub fn port_world_position(
    place_origin: (i32, i32, i32),
    place_dims: Dims,
    def: &DefIr,
    port_id: &str,
) -> Option<(i32, i32, i32)> {
    let member = def
        .members
        .iter()
        .find(|m| m.id.as_deref() == Some(port_id))?;
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
            let u = door_at_center_offset(member, len)?;
            door_world_xz(side, u, overhang, interior_w, interior_h, place_origin)?
        }
        MemberRole::Window => {
            let u = window_center_offset(member, len)?;
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
        // Returning `None` here keeps the door/window contract enforced
        // without growing the diagnostic surface.
        _ => return None,
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

fn door_at_center_offset(member: &Member, len: u32) -> Option<u32> {
    let raw = member.intent_state.get("at")?;
    match &raw.value.kind {
        ValueKind::Ident(s) if s == "center" => Some(len / 2),
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

/// Window port wall-local centre offset: `offset + size.w / 2`, with an
/// `offset + size.w ≤ wall_length` bounds check so a window written past
/// the wall returns `None` and cascades to `W_DEFERRED_MEMBER` rather
/// than producing an out-of-range world coordinate.
fn window_center_offset(member: &Member, len: u32) -> Option<u32> {
    let offset = nonneg_int_value(member, "offset")?;
    let (sw, _) = size_member(member, "size")?;
    let end = offset.checked_add(sw)?;
    if end > len {
        return None;
    }
    Some(offset + sw / 2)
}

/// Window-side variant of [`door_world_xz`]. Delegates to
/// [`wall_local_to_grid`] so the wall-local → grid mapping is shared
/// with the openings carved into the wall itself (`block_array::lower`
/// uses the same helper for the window cut). `v = 0` pins the port to
/// the ground row regardless of the window's authored `y=`.
fn window_world_xz(
    side: WallSide,
    u: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    place_dims: Dims,
    origin: (i32, i32, i32),
) -> Option<(i32, i32)> {
    let (grid_x, _, grid_z) =
        wall_local_to_grid(side, u, 0, overhang, interior_w, interior_h, place_dims)?;
    let grid_x = i32::try_from(grid_x).ok()?;
    let grid_z = i32::try_from(grid_z).ok()?;
    Some((origin.0.checked_add(grid_x)?, origin.2.checked_add(grid_z)?))
}

fn nonneg_int_value(member: &Member, key: &str) -> Option<u32> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v >= 0 => Some(u32::try_from(*v).unwrap_or(u32::MAX)),
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
        let pos = port_world_position((10, 0, 20), dims, def, "entry").expect("port resolves");
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
        let pos = port_world_position((10, 0, 20), dims, def, "entry").expect("port resolves");
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
        let pos = port_world_position((10, 0, 20), dims, def, "entry").expect("port resolves");
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
        let pos = port_world_position((10, 0, 20), dims, def, "entry").expect("port resolves");
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
        let pos = port_world_position((10, 0, 20), dims, def, "entry").expect("port resolves");
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
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
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
            "  window id=light side=back y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
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
            "  window id=light side=left y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
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
            "  window id=light side=right y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
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
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
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
            "  window id=front side=front y=2 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 11, y: 1, z: 9 };
        let pos = port_world_position((0, 0, 0), dims, def, "front").expect("port resolves");
        assert_eq!(pos, (4, 0, 8));
    }

    #[test]
    fn port_world_position_window_returns_none_when_offset_size_overflows_wall() {
        // size=3x3 → wall_length(Front) = 3. offset=2 + size.w=2 = 4 > 3.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  window id=light side=front y=1 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, "light").is_none());
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
            "  window id=light side=front y=1 offset=0 size=1x1 sym=true mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 0, 20), dims, def, "light").expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_pins_y_to_ground_row_regardless_of_authored_y() {
        // `y=4` on the window must not lift the port off the ground row
        // — walkways are flat 1-voxel strips and the port Y must agree
        // with the other endpoint (door y=0). The port stays at
        // `place_origin.1`.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  window id=light side=front y=4 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_world_position((10, 7, 20), dims, def, "light").expect("port resolves");
        assert_eq!(pos.1, 7, "port Y should equal place_origin.1, not window.y");
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
        assert!(port_world_position((0, 0, 0), dims, def, "top").is_none());
    }

    #[test]
    fn port_world_position_returns_none_for_stair_role() {
        // Stair ports are reserved; same short-circuit as roof.
        let src = concat!("def cottage size=3x3:\n", "  stair id=up at=corner\n");
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(port_world_position((0, 0, 0), dims, def, "up").is_none());
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
        assert!(port_world_position((0, 0, 0), dims, def, "nope").is_none());
    }
}
