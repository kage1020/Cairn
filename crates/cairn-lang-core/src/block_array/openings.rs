//! Wall-local coordinate mapping for openings (doors and windows).
//!
//! Door/window members are written in the source as `side=front offset=N y=N
//! size=WxH`. That coordinate system is anchored to the wall the opening is
//! cut into, not to the struct's voxel grid: `offset` runs along the wall,
//! `y` rises from the floor, and the wall index itself (`x` or `z`) is fixed
//! by the side. This module converts a `(side, u, v)` wall-local
//! coordinate into a `(x, y, z)` struct-grid coordinate, accounting for the
//! `overhang` shift that pushes floor/walls inward by a constant on every
//! axis (see `block_array/mod.rs` for the overhang convention).
//!
//! Kept in its own module so doors and windows share one mapping instead of
//! re-deriving it twice with subtle sign errors.

use super::Dims;

/// Which wall of a struct an opening cuts through.
///
/// Sides follow `spec/syntax` "Selectors": `front` = `+z`, `back` = `-z`,
/// `left` = `-x`, `right` = `+x`. The names are world-facing (a viewer
/// outside the structure sees the front wall as the +z face).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallSide {
    /// `+z` face of the struct.
    Front,
    /// `-z` face of the struct.
    Back,
    /// `-x` face of the struct.
    Left,
    /// `+x` face of the struct.
    Right,
}

impl WallSide {
    /// Parse a `side=` identifier value into a [`WallSide`], if it names one
    /// of the four cardinal walls. Returns `None` for any other identifier;
    /// the caller turns that into a `W_DEFERRED_MEMBER` warning.
    #[must_use]
    pub fn from_ident(name: &str) -> Option<Self> {
        match name {
            "front" => Some(Self::Front),
            "back" => Some(Self::Back),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// Unit step `(dx, dz)` from the wall toward the outside of the struct.
    #[must_use]
    pub fn outward_normal(self) -> (i32, i32) {
        match self {
            Self::Front => (0, 1),
            Self::Back => (0, -1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }
}

/// Length of the wall along its `offset` axis, in voxels.
///
/// `interior_w` is the struct's authored `size.w` and `interior_h` the
/// authored `size.h` (i.e. the footprint *before* overhang inflation). The
/// front and back walls run along `x`, so their length is `interior_w`; the
/// left and right walls run along `z`, so their length is `interior_h`.
#[must_use]
pub fn wall_length(side: WallSide, interior_w: u32, interior_h: u32) -> u32 {
    match side {
        WallSide::Front | WallSide::Back => interior_w,
        WallSide::Left | WallSide::Right => interior_h,
    }
}

/// Convert a wall-local `(u, v)` coordinate into a struct-grid `(x, y, z)`.
///
/// - `u` is the offset along the wall, starting at 0 at the wall's *left
///   end* in the view of someone standing outside that wall (front: low x,
///   back: low x mirrored to high x, left: low z, right: low z mirrored to
///   high z; the per-side mirroring keeps `sym=true` symmetric about the
///   wall's midpoint regardless of which side is named).
/// - `v` is the height from the floor (`y` directly).
/// - `overhang` shifts the wall inward on its own axis.
/// - `interior` carries the authored `size` extents (before overhang
///   inflation). Pass the struct's `size.w` / `size.h` so the helper can
///   compute the back-wall and right-wall mirroring without re-deriving the
///   full inflated dimensions.
///
/// Returns `None` if `(u, v)` falls outside the wall's range.
///
/// No caller can reach that `None` today; each asks only after checking
/// the same bounds itself. There are five callers and two dispositions:
///
/// - The four lowering callers in `block_array::lower` — the single-cell
///   `plate_voxel_position` and the walks over a rectangle or a band,
///   `carve_door`, `fill_stair` and `paint_window_rect` — validate `u` and
///   `v` against the wall first and report any author error there, once per
///   member. A `None` reaching them means one of those checks stopped
///   agreeing with this function, which is a compiler bug rather than
///   something the author can fix, so each one `debug_assert!`s with a
///   message naming the check it relied on, then drops the cell (or the
///   plate) in release builds rather than panicking over one voxel. The
///   sites are tagged `INVARIANT(wall-grid-validated)`.
/// - `walkway::window_world_xz` propagates the `None` with `?`. Its `u`
///   comes from `window_center_offset`, already bounded by the wall, and its
///   `v` is the ground row `0`, so that `None` is unreachable too.
#[must_use]
pub fn wall_local_to_grid(
    side: WallSide,
    u: u32,
    v: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    dims: Dims,
) -> Option<(u32, u32, u32)> {
    let length = wall_length(side, interior_w, interior_h);
    if u >= length || v >= dims.y {
        return None;
    }
    match side {
        WallSide::Front => {
            let x = overhang.saturating_add(u);
            let z = overhang.saturating_add(interior_h).saturating_sub(1);
            Some((x, v, z))
        }
        WallSide::Back => {
            // Back wall mirrors `u` so symmetry around the wall midpoint is
            // preserved between sides — a `sym=true` window on the back
            // looks like the front-wall pair viewed from the other side.
            let mirrored = interior_w.saturating_sub(1).saturating_sub(u);
            let x = overhang.saturating_add(mirrored);
            let z = overhang;
            Some((x, v, z))
        }
        WallSide::Left => {
            let x = overhang;
            let z = overhang.saturating_add(u);
            Some((x, v, z))
        }
        WallSide::Right => {
            // Right wall mirrors `u` along z for the same reason the back
            // wall mirrors along x.
            let mirrored = interior_h.saturating_sub(1).saturating_sub(u);
            let x = overhang.saturating_add(interior_w).saturating_sub(1);
            let z = overhang.saturating_add(mirrored);
            Some((x, v, z))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims_for(interior_w: u32, interior_h: u32, overhang: u32, y: u32) -> Dims {
        Dims {
            x: interior_w + 2 * overhang,
            y,
            z: interior_h + 2 * overhang,
        }
    }

    #[test]
    fn from_ident_parses_four_cardinals() {
        assert_eq!(WallSide::from_ident("front"), Some(WallSide::Front));
        assert_eq!(WallSide::from_ident("back"), Some(WallSide::Back));
        assert_eq!(WallSide::from_ident("left"), Some(WallSide::Left));
        assert_eq!(WallSide::from_ident("right"), Some(WallSide::Right));
        assert_eq!(WallSide::from_ident("up"), None);
    }

    #[test]
    fn front_wall_maps_u_to_x_and_z_to_far_edge() {
        // size=9x7, overhang=1 → interior x ∈ [1, 9], z ∈ [1, 7].
        // Front wall is the +z face: z = overhang + size.h - 1 = 7.
        let dims = dims_for(9, 7, 1, 9);
        assert_eq!(
            wall_local_to_grid(WallSide::Front, 0, 1, 1, 9, 7, dims),
            Some((1, 1, 7)),
        );
        assert_eq!(
            wall_local_to_grid(WallSide::Front, 8, 4, 1, 9, 7, dims),
            Some((9, 4, 7)),
        );
    }

    #[test]
    fn back_wall_mirrors_u_along_x() {
        // size=9x7, overhang=1. Back wall at z=overhang=1, and u=0 should
        // land at the *opposite* x corner from front u=0 so a `sym=true`
        // opening looks the same from both sides of the building.
        let dims = dims_for(9, 7, 1, 9);
        assert_eq!(
            wall_local_to_grid(WallSide::Back, 0, 1, 1, 9, 7, dims),
            Some((9, 1, 1)),
        );
        assert_eq!(
            wall_local_to_grid(WallSide::Back, 8, 1, 1, 9, 7, dims),
            Some((1, 1, 1)),
        );
    }

    #[test]
    fn left_and_right_walls_run_along_z() {
        let dims = dims_for(9, 7, 1, 9);
        // Left wall: x=overhang, u runs along z starting at z=overhang.
        assert_eq!(
            wall_local_to_grid(WallSide::Left, 0, 2, 1, 9, 7, dims),
            Some((1, 2, 1)),
        );
        assert_eq!(
            wall_local_to_grid(WallSide::Left, 6, 2, 1, 9, 7, dims),
            Some((1, 2, 7)),
        );
        // Right wall: x=overhang+size.w-1=9, u runs along z mirrored.
        assert_eq!(
            wall_local_to_grid(WallSide::Right, 0, 2, 1, 9, 7, dims),
            Some((9, 2, 7)),
        );
        assert_eq!(
            wall_local_to_grid(WallSide::Right, 6, 2, 1, 9, 7, dims),
            Some((9, 2, 1)),
        );
    }

    #[test]
    fn out_of_range_returns_none() {
        let dims = dims_for(9, 7, 1, 9);
        // u past the wall length.
        assert_eq!(
            wall_local_to_grid(WallSide::Front, 9, 1, 1, 9, 7, dims),
            None,
        );
        // v past dims.y.
        assert_eq!(
            wall_local_to_grid(WallSide::Front, 0, 9, 1, 9, 7, dims),
            None,
        );
    }

    #[test]
    fn the_gate_is_exactly_the_wall_on_every_side() {
        // The lowering callers assert rather than report a `None`, so the
        // assertions can only catch a gate that *tightens*, and only where
        // a test paints the cell it no longer admits. A gate that loosens
        // never returns `None` at all, and on the back and right walls
        // the `saturating_sub` mirror would answer a valid-looking corner
        // cell instead. Pin both edges on all four sides: the last cell of
        // the wall (`u = len - 1`, `v = dims.y - 1`) maps, and one past it
        // on either axis does not.
        let dims = dims_for(9, 7, 1, 9);
        for (side, len, last_cell) in [
            (WallSide::Front, 9, (9, 8, 7)),
            (WallSide::Back, 9, (1, 8, 1)),
            (WallSide::Left, 7, (1, 8, 7)),
            (WallSide::Right, 7, (9, 8, 1)),
        ] {
            assert_eq!(wall_length(side, 9, 7), len, "{side:?}");
            assert_eq!(
                wall_local_to_grid(side, len - 1, dims.y - 1, 1, 9, 7, dims),
                Some(last_cell),
                "{side:?}: the last cell of the wall maps",
            );
            assert_eq!(
                wall_local_to_grid(side, len, 0, 1, 9, 7, dims),
                None,
                "{side:?}: `u = len` is past the wall",
            );
            assert_eq!(
                wall_local_to_grid(side, 0, dims.y, 1, 9, 7, dims),
                None,
                "{side:?}: `v = dims.y` is past the wall",
            );
        }
    }

    #[test]
    fn wall_length_picks_axis_by_side() {
        assert_eq!(wall_length(WallSide::Front, 9, 7), 9);
        assert_eq!(wall_length(WallSide::Back, 9, 7), 9);
        assert_eq!(wall_length(WallSide::Left, 9, 7), 7);
        assert_eq!(wall_length(WallSide::Right, 9, 7), 7);
    }
}
