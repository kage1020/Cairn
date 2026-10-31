//! Wall-local coordinates for doors and windows, and the arguments that place them on a wall.

use crate::ast::ValueKind;
use crate::intent::Member;

use super::Dims;
use super::args::size_value;

/// A struct wall, named as seen from outside (`spec/syntax` "Selectors").
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

/// Length of the wall along its `offset` axis, given the authored `size=WxH`.
#[must_use]
pub fn wall_length(side: WallSide, interior_w: u32, interior_h: u32) -> u32 {
    match side {
        WallSide::Front | WallSide::Back => interior_w,
        WallSide::Left | WallSide::Right => interior_h,
    }
}

/// Convert a wall-local `(u, v)` into a struct-grid `(x, y, z)`, or `None` outside the wall.
///
/// `u` runs from the wall's left end as seen from outside, so `sym=true` mirrors about its middle.
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

/// How an argument a port cannot use was written.
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

/// What is wrong with a door's `at=`, as the rest of a sentence whose subject is the door.
pub(super) fn door_at_clause(written: &Written) -> String {
    match written {
        Written::Absent => "has no `at=`".to_owned(),
        Written::Ident(s) => format!("has `at={s}`, which is not one of center, left, right"),
        Written::OtherShape => "has an `at=` that is not one of center, left, right (numeric \
                                offsets are reserved)"
            .to_owned(),
    }
}

/// The primary of the `E_INVALID_MEMBER` for a door whose `at=` [`door_anchor_offset`] refused.
pub(super) fn door_at_refusal(written: &Written) -> String {
    format!(
        "door {} — use `at=center | left | right`",
        door_at_clause(written)
    )
}

/// The first window argument [`read_window_args`] could not use.
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
    /// What is wrong, as the rest of a sentence whose subject is the window.
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

    /// The primary of the `E_INVALID_MEMBER` that refuses the window.
    pub(super) fn refusal(self) -> String {
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

/// Read a window's `offset=`, `y=` and `size=`, in that order.
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

/// Wall-local `u` of a door's `at=` on a wall `len` long, or how an unusable `at=` was written.
///
/// `center` is `len / 2`, the column right of the midpoint on an even wall (`spec/syntax`
/// "Selectors").
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
