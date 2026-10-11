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
//!
//! The arguments that place an opening on its wall — a door's `at=`, a
//! window's `offset=` / `y=` / `size=` — are read here too, once, for the
//! cut and for a `connect` port anchored on the opening alike, so the two
//! accept the same values and refuse the same first one in the same words.

use crate::ast::ValueKind;
use crate::intent::Member;

use super::Dims;
use super::args::size_value;

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
/// - `ports::window_world_xz` propagates the `None` with `?`. Its `u`
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

/// How a member argument a port reads was written, when it was not written
/// in a shape the port can use.
///
/// Kept apart from the value itself so the note can say *which* of the
/// three it was — an author who left `side=` off and one who typed
/// `side=frnt` are fixing different things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Written {
    /// The key is not on the line.
    Absent,
    /// The key carries an identifier the port does not accept.
    Ident(String),
    /// The key carries a value of some other shape (a number, a string, …).
    OtherShape,
}

impl Written {
    pub(super) fn of_ident(member: &Member, key: &str) -> Self {
        match member.intent_state.get(key).map(|v| &v.value.kind) {
            None => Self::Absent,
            Some(ValueKind::Ident(s)) => Self::Ident(s.clone()),
            Some(_) => Self::OtherShape,
        }
    }
}

/// What is wrong with a door's `at=`, as the rest of a sentence whose
/// subject is the door.
///
/// One wording for both places that say it — the door's own
/// `W_DEFERRED_MEMBER` ([`door_at_deferral`]) and the note a `connect` row
/// naming the door carries (`ports::PortRejection::note`) — because both read
/// `at=` through [`door_anchor_offset`], so they cannot classify it
/// differently, and with one sentence they cannot describe it differently
/// either.
pub(super) fn door_at_clause(written: &Written) -> String {
    match written {
        Written::Absent => "has no `at=`".to_owned(),
        Written::Ident(s) => format!("has `at={s}`, which is not one of center, left, right"),
        Written::OtherShape => "has an `at=` that is not one of center, left, right (numeric \
                                offsets are reserved)"
            .to_owned(),
    }
}

/// The reason `super::lower::door::carve_door` defers a door whose `at=`
/// [`door_anchor_offset`] refused.
pub(super) fn door_at_deferral(written: &Written) -> String {
    format!(
        "door {} — use `at=center | left | right`",
        door_at_clause(written)
    )
}

/// The one window argument [`read_window_args`] could not use, and how it
/// was written. One variant per case rather than a key and a flag, so an
/// `offset=` that is absent — which is not a fault, it reads as `0` — has
/// no variant to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowArgFault {
    /// `offset=` is present and not a non-negative integer.
    OffsetMalformed,
    /// No `y=`.
    YAbsent,
    /// `y=` is present and not a non-negative integer.
    YMalformed,
    /// No `size=`.
    SizeAbsent,
    /// `size=` is present and not a `WxH`.
    SizeMalformed,
}

impl WindowArgFault {
    /// What is wrong, as the rest of a sentence whose subject is the
    /// window — the same single wording source [`door_at_clause`] is.
    pub(super) fn clause(self) -> &'static str {
        match self {
            Self::OffsetMalformed => {
                "has an `offset=` that is not a non-negative integer that fits in u32"
            }
            Self::YAbsent => "has no `y=`",
            Self::YMalformed => "has a `y=` that is not a non-negative integer that fits in u32",
            Self::SizeAbsent => "has no `size=WxH`",
            Self::SizeMalformed => "has a `size=` that is not a `WxH` of two positive integers",
        }
    }

    /// The reason `super::lower::window::cut_window` defers the window.
    pub(super) fn deferral(self) -> String {
        format!("window {}", self.clause())
    }
}

/// The window arguments a port and a cut both need, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowArgs {
    /// `offset=`, `0` when absent.
    pub(super) offset: u32,
    /// `y=`, relative to the member's own level.
    pub(super) y: u32,
    /// `size=`'s `W`.
    pub(super) width: u32,
    /// `size=`'s `H`.
    pub(super) height: u32,
}

/// Read a window's `offset=`, `y=` and `size=`, in that order, refusing
/// at the first one that cannot be used.
///
/// `super::lower::window::cut_window` and `ports::window_center_offset`, which
/// `ports::port_world_position` asks for a window port, both call this, so the
/// cut and the port accept one set and name one first fault.
/// `repeat=`, `step=` and `sym=` are not read here: only the cut uses them,
/// and a window they defer is refused as a port by
/// `ports::PortRejection::NotCut` rather than by a reason of its own.
///
/// # Errors
///
/// The [`WindowArgFault`] for the first argument that cannot be used.
pub(super) fn read_window_args(member: &Member) -> Result<WindowArgs, WindowArgFault> {
    let offset = if member.intent_state.contains_key("offset") {
        member
            .nonneg_u32("offset")
            .ok_or(WindowArgFault::OffsetMalformed)?
    } else {
        0
    };
    let y = match (
        member.intent_state.contains_key("y"),
        member.nonneg_u32("y"),
    ) {
        (_, Some(y)) => y,
        (false, None) => return Err(WindowArgFault::YAbsent),
        (true, None) => return Err(WindowArgFault::YMalformed),
    };
    let (width, height) = match (
        member.intent_state.contains_key("size"),
        size_value(member, "size"),
    ) {
        (_, Some(size)) => size,
        (false, None) => return Err(WindowArgFault::SizeAbsent),
        (true, None) => return Err(WindowArgFault::SizeMalformed),
    };
    Ok(WindowArgs {
        offset,
        y,
        width,
        height,
    })
}

/// Wall-local `u` anchor for a door port. Accepts the three named
/// anchors the spec defines for `at=`:
///
/// * `center` — `len / 2` (integer division, so even widths land at
///   the column one cell `+u` of the midpoint, matching the
///   convention `super::lower::door::carve_door` uses when cutting the
///   opening; `spec/syntax` "Selectors" calls this "round-half-up").
/// * `left`   — `0`, the wall-local axis origin.
/// * `right`  — `len - 1`, the far corner. The `len.saturating_sub(1)`
///   guard returns `0` for a hypothetical `len == 0` rather than
///   underflowing `u32`, but `len == 0` is unreachable in practice:
///   `DefIr.size.w` / `.h` are `NonZeroU32`, and `wall_length` is one
///   of them — so every shipping caller has `len ≥ 1` and the
///   `right` anchor lands on a valid column.
///
/// Numeric offsets (`at=N`) are reserved for a future extension and
/// are refused with how `at=` was [`Written`] rather than silently
/// rounded to centre, as is a missing `at=` or any other value.
///
/// `super::lower::door::carve_door` cuts the doorway at this column and defers
/// with [`door_at_deferral`] of this refusal, so the cut, the port and
/// both of their messages read `at=` once.
pub(super) fn door_anchor_offset(member: &Member, len: u32) -> Result<u32, Written> {
    match member.ident_value("at") {
        Some("center") => Ok(len / 2),
        Some("left") => Ok(0),
        Some("right") => Ok(len.saturating_sub(1)),
        _ => Err(Written::of_ident(member, "at")),
    }
}

#[cfg(test)]
mod tests;
