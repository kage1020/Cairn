//! Wall-local coordinates for doors and windows, and the arguments that place them on a wall.

use crate::ast::ValueKind;
use crate::intent::Member;

use super::Dims;
use super::args::size_value;

/// Rows a door block stands, from the row it opens at.
pub(super) const DOOR_HEIGHT: u32 = 2;

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

    /// Unit step `(dx, dz)` along the wall toward its right end as seen from outside, the way
    /// [`wall_local_to_grid`]'s `u` grows.
    #[must_use]
    pub fn along(self) -> (i32, i32) {
        match self {
            Self::Front => (1, 0),
            Self::Back => (-1, 0),
            Self::Left => (0, 1),
            Self::Right => (0, -1),
        }
    }
}

/// The name a `side=` gives `side`.
pub(super) fn side_name(side: WallSide) -> &'static str {
    match side {
        WallSide::Front => "front",
        WallSide::Back => "back",
        WallSide::Left => "left",
        WallSide::Right => "right",
    }
}

/// The wall meeting `side` at its `offset=0` end, or at its far end.
fn corner_wall(side: WallSide, at_offset_zero: bool) -> WallSide {
    match (side, at_offset_zero) {
        (WallSide::Front, true) | (WallSide::Back, false) => WallSide::Left,
        (WallSide::Front, false) | (WallSide::Back, true) => WallSide::Right,
        (WallSide::Left, true) | (WallSide::Right, false) => WallSide::Back,
        (WallSide::Left, false) | (WallSide::Right, true) => WallSide::Front,
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
    /// The key carries an integer.
    Int(i64),
    /// The key carries a value of some other shape, worded as [`crate::ast::Value::describe`] does.
    OtherShape(String),
}

impl Written {
    pub(super) fn of_ident(member: &Member, key: &str) -> Self {
        let Some(raw) = member.written(key) else {
            return Self::Absent;
        };
        match &raw.value.kind {
            ValueKind::Ident(s) => Self::Ident(s.clone()),
            ValueKind::Int(n) => Self::Int(*n),
            _ => Self::OtherShape(raw.value.describe()),
        }
    }
}

/// Why a door's line names no column of its wall that [`door_column`] can stand it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DoorColumnFault {
    /// No `offset=`, and an `at=` that names no anchor, as written: `Absent` when neither is there.
    At(Written),
    /// Both `at=` and `offset=`.
    Both,
    /// An `offset=` that is not a non-negative `u32`, worded as [`crate::ast::Value::describe`]
    /// does.
    OffsetMalformed(String),
    /// An `offset=` naming a corner column of the `side` wall.
    Corner { offset: u32, side: WallSide },
    /// An `offset=` at or past the far end of the `side` wall, `length` long.
    PastTheWall {
        offset: u32,
        side: WallSide,
        length: u32,
    },
}

impl DoorColumnFault {
    /// What is wrong, as the rest of a sentence whose subject is the door.
    pub(super) fn clause(&self) -> String {
        match self {
            Self::At(Written::Absent) => "has no `at=` or `offset=`".to_owned(),
            Self::At(Written::Ident(s)) => {
                format!("has `at={s}`, which is not one of center, left, right")
            }
            Self::At(Written::Int(n)) => {
                format!("has `at={n}`, which is not one of center, left, right")
            }
            Self::At(Written::OtherShape(described)) => {
                format!("has an `at=` that is {described}, not one of center, left, right")
            }
            Self::Both => "names its column twice, with `at=` and with `offset=`".to_owned(),
            Self::OffsetMalformed(described) => format!(
                "has an `offset=` that is {described}, not a non-negative integer that fits in u32"
            ),
            Self::Corner { offset, side } => format!(
                "has `offset={offset}`, the {} wall's corner with the {} wall",
                side_name(*side),
                side_name(corner_wall(*side, *offset == 0)),
            ),
            Self::PastTheWall {
                offset,
                side,
                length,
            } => format!(
                "has `offset={offset}`, past the end of the {} wall (length {length})",
                side_name(*side),
            ),
        }
    }

    /// The primary of the `E_INVALID_MEMBER` that refuses the door.
    pub(super) fn refusal(&self) -> String {
        format!("door {}", self.clause())
    }

    /// The note on that refusal, saying what names a column the door can stand in.
    pub(super) fn fix(&self, length: u32) -> String {
        const ONCE: &str = "name the door's column once: `at=center`, `at=left` or `at=right` for \
                            an anchor, or `offset=N` for a column between the wall's corners, \
                            counted from 0 at its left end seen from outside";
        match self {
            Self::At(Written::Int(n)) => format!(
                "a column by number is `offset={n}`, counted from 0 at the wall's left end seen \
                 from outside; `at=` takes `center`, `left` or `right`"
            ),
            Self::At(_) | Self::Both => ONCE.to_owned(),
            Self::OffsetMalformed(_) => format!(
                "write `offset=N` with `N` a whole number; {}",
                between_the_corners(length),
            ),
            Self::Corner { offset, .. } => format!(
                "{}; `at={}` stands the door in this corner",
                between_the_corners(length),
                if *offset == 0 { "left" } else { "right" },
            ),
            Self::PastTheWall { .. } => between_the_corners(length),
        }
    }
}

/// The columns an `offset=` takes on a wall `length` long, as a clause.
fn between_the_corners(length: u32) -> String {
    match length {
        0..=2 => format!("the wall is {length} long, so no column lies between its corners"),
        3 => "`offset=` takes the one column between the wall's corners, 1".to_owned(),
        _ => format!(
            "`offset=` takes a column between the wall's corners, from 1 to {}",
            length - 2
        ),
    }
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

    /// The note on that refusal, saying what the argument takes.
    pub(super) fn fix(self) -> &'static str {
        match self {
            Self::OffsetMalformed => {
                "write `offset=N`, where `N` is a non-negative integer: the window's first column \
                 along the wall, counted from 0 at its left end seen from outside"
            }
            Self::YAbsent | Self::YMalformed => {
                "write `y=N`, where `N` is a non-negative integer: the window's lowest row, counted \
                 up from 0, the base row of the struct or of the window's `level`"
            }
            Self::SizeAbsent | Self::SizeMalformed => {
                "write `size=WxH`, where `W` and `H` are positive integers: the window's width \
                 along the wall and its height in rows"
            }
        }
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
    let offset = if member.written("offset").is_some() {
        member
            .nonneg_u32("offset")
            .ok_or(WindowArgFault::OffsetMalformed)?
    } else {
        0
    };
    let y = match (member.written("y").is_some(), member.nonneg_u32("y")) {
        (_, Some(y)) => y,
        (false, None) => return Err(WindowArgFault::YAbsent),
        (true, None) => return Err(WindowArgFault::YMalformed),
    };
    let (width, height) = match (member.written("size").is_some(), size_value(member, "size")) {
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

/// Wall-local `u` of the column a door's `at=` anchor or `offset=` names on its `side` wall,
/// `length` long (`spec/syntax` "Selectors").
///
/// # Errors
///
/// The [`DoorColumnFault`] when the line names no column a door can stand in.
pub(super) fn door_column(
    member: &Member,
    side: WallSide,
    length: u32,
) -> Result<u32, DoorColumnFault> {
    let Some(written) = member.written("offset") else {
        return match member.ident_value("at") {
            Some("center") => Ok(length / 2),
            Some("left") => Ok(0),
            Some("right") => Ok(length.saturating_sub(1)),
            _ => Err(DoorColumnFault::At(Written::of_ident(member, "at"))),
        };
    };
    if member.written("at").is_some() {
        return Err(DoorColumnFault::Both);
    }
    let offset = member
        .nonneg_u32("offset")
        .ok_or_else(|| DoorColumnFault::OffsetMalformed(written.value.describe()))?;
    if offset >= length {
        Err(DoorColumnFault::PastTheWall {
            offset,
            side,
            length,
        })
    } else if offset == 0 || offset == length - 1 {
        Err(DoorColumnFault::Corner { offset, side })
    } else {
        Ok(offset)
    }
}

#[cfg(test)]
mod tests;
