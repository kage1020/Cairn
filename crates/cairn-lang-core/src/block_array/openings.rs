//! Wall-local coordinates for openings (doors and windows).
//!
//! A door or window is written as `side=front offset=N y=N size=WxH`, in the
//! coordinates of the wall it is cut into: `offset` runs along the wall, `y`
//! rises from the floor, and the side fixes the wall's own `x` or `z`. This
//! module maps a `(side, u, v)` onto the struct grid, including the inward
//! `overhang` shift (the overhang convention in the `block_array` module
//! docs), in one place so doors and windows cannot disagree on a sign.
//!
//! It also reads the arguments that place an opening on its wall — a door's
//! `at=`, a window's `offset=`, `y=` and `size=` — once for the cut and a
//! `connect` port on the opening alike, so the two accept the same values
//! and refuse the same first one in the same words.

use crate::ast::ValueKind;
use crate::intent::Member;

use super::Dims;
use super::args::size_value;

/// Which wall of a struct an opening cuts through, named as seen from
/// outside (`spec/syntax` "Selectors").
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
    /// The [`WallSide`] a `side=` identifier names, or `None`.
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

/// Length of the wall along its `offset` axis, in voxels. `interior_w` and
/// `interior_h` are the authored `size=WxH`, before overhang inflation.
#[must_use]
pub fn wall_length(side: WallSide, interior_w: u32, interior_h: u32) -> u32 {
    match side {
        WallSide::Front | WallSide::Back => interior_w,
        WallSide::Left | WallSide::Right => interior_h,
    }
}

/// Convert a wall-local `(u, v)` coordinate into a struct-grid `(x, y, z)`,
/// or `None` when it falls outside the wall.
///
/// - `u` runs along the wall from its left end as seen from outside, so it
///   starts at high `x` on the back wall and high `z` on the right one;
///   that keeps `sym=true` symmetric about the midpoint of every wall.
/// - `v` is the height from the floor.
/// - `overhang` shifts the wall inward.
/// - `interior_w` and `interior_h` are the authored `size=WxH`, before
///   overhang inflation.
///
/// No caller reaches the `None`: each checks the same bounds first. The
/// lowering callers (`carve_door`, `fill_stair`, `paint_window_rect`,
/// `plate_voxel_position`) report author errors in that check, so a `None`
/// here is a compiler bug; at the sites tagged
/// `INVARIANT(wall-grid-validated)` they `debug_assert!` and drop the cell
/// in a release build. `ports::wall_world_xz` propagates it with `?`.
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
            let mirrored = interior_h.saturating_sub(1).saturating_sub(u);
            let x = overhang.saturating_add(interior_w).saturating_sub(1);
            let z = overhang.saturating_add(mirrored);
            Some((x, v, z))
        }
    }
}

/// How an argument a port cannot use was written, so the note can tell a
/// missing `side=` from a misspelt `side=frnt`.
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
/// Shared by the door's own `W_DEFERRED_MEMBER` ([`door_at_deferral`]) and
/// the note on a `connect` row naming the door (`ports::PortRejection::note`),
/// so the two describe one refusal in one wording.
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

/// The first window argument [`read_window_args`] could not use, and how it
/// was written. One variant per case rather than a key and a flag, so an
/// absent `offset=`, which reads as `0`, has none.
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
    /// window, shared like [`door_at_clause`].
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
/// The cut (`super::lower::window::cut_window`) and the port
/// (`ports::window_center_offset`) both read through this, so they accept
/// one set and name one first fault. `repeat=`, `step=` and `sym=` are the
/// cut's alone; a window they defer is refused as a port by
/// `ports::PortRejection::NotCut`.
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

/// Wall-local `u` of a door's `at=` on a wall `len` long, for the cut
/// (`super::lower::door::carve_door`) and a port alike:
///
/// * `center`: `len / 2`, so an even wall rounds half up (`spec/syntax`
///   "Selectors").
/// * `left`: `0`.
/// * `right`: `len - 1`. `len` is a `size=` extent, never `0`.
///
/// Anything else, a numeric `at=N` (reserved) or a missing `at=`
/// included, is refused with how it was [`Written`].
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
