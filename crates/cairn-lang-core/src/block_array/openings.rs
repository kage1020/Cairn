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
    /// The key carries an integer, a numeric offset on a door's `at=`.
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

/// What is wrong with a door's `at=`, as the rest of a sentence whose subject is the door.
pub(super) fn door_at_clause(written: &Written) -> String {
    match written {
        Written::Absent => "has no `at=`".to_owned(),
        Written::Ident(s) => format!("has `at={s}`, which is not one of center, left, right"),
        Written::Int(n) => format!(
            "has `at={n}`, which is not one of center, left, right (numeric offsets are reserved)"
        ),
        Written::OtherShape(described) => {
            format!("has an `at=` that is {described}, not one of center, left, right")
        }
    }
}

/// The primary of the `E_INVALID_MEMBER` for a door whose `at=` [`door_anchor_offset`] refused.
pub(super) fn door_at_refusal(written: &Written) -> String {
    format!(
        "door {} — use `at=center | left | right`",
        door_at_clause(written)
    )
}

/// One of a window's three columns or rows an `anchor=` can name: its first, its middle or its last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Third {
    Start,
    Middle,
    End,
}

impl Third {
    /// How far this third lies from the first of `span` cells; the middle of an even span is the
    /// later of its two middle cells, as a door's `at=center` is.
    const fn shift(self, span: u32) -> u32 {
        match self {
            Self::Start => 0,
            Self::Middle => span / 2,
            Self::End => span.saturating_sub(1),
        }
    }
}

/// Every `anchor=` a window takes: its name, the column it names and the row.
const WINDOW_ANCHORS: [(&str, Third, Third); 9] = [
    ("bottom_left", Third::Start, Third::Start),
    ("bottom_center", Third::Middle, Third::Start),
    ("bottom_right", Third::End, Third::Start),
    ("middle_left", Third::Start, Third::Middle),
    ("center", Third::Middle, Third::Middle),
    ("middle_right", Third::End, Third::Middle),
    ("top_left", Third::Start, Third::End),
    ("top_center", Third::Middle, Third::End),
    ("top_right", Third::End, Third::End),
];

/// The names [`WINDOW_ANCHORS`] lists, for a message.
fn window_anchor_names() -> String {
    WINDOW_ANCHORS.map(|(name, _, _)| name).join(", ")
}

/// The cell of a window its `offset=` and `y=` place, seen from outside (`spec/entities` "Anchor
/// conventions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowAnchor {
    name: &'static str,
    along: Third,
    up: Third,
}

impl WindowAnchor {
    /// The anchor a window with no `anchor=` takes.
    const BOTTOM_LEFT: Self = Self {
        name: "bottom_left",
        along: Third::Start,
        up: Third::Start,
    };

    /// The anchor `name` names, or `None` for a name [`WINDOW_ANCHORS`] does not list.
    pub(super) fn from_ident(name: &str) -> Option<Self> {
        WINDOW_ANCHORS
            .into_iter()
            .find(|(n, _, _)| *n == name)
            .map(|(name, along, up)| Self { name, along, up })
    }
}

/// The first window argument [`read_window_args`] could not use.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// `anchor=` is present and names no anchor, so never [`Written::Absent`].
    AnchorUnknown(Written),
    /// The anchor lies `shift` columns right of the window's first column, and `offset=` is less.
    BeforeWallStart {
        anchor: &'static str,
        offset: u32,
        shift: u32,
    },
    /// The anchor lies `shift` rows above the window's lowest row, and `y=` is less.
    BelowBaseRow {
        anchor: &'static str,
        y: u32,
        shift: u32,
    },
}

impl WindowArgFault {
    /// What is wrong, as the rest of a sentence whose subject is the window.
    pub(super) fn clause(&self) -> String {
        match self {
            Self::OffsetMalformed => {
                "has an `offset=` that is not a non-negative integer that fits in u32".to_owned()
            }
            Self::YAbsent => "has no `y=`".to_owned(),
            Self::YMalformed => {
                "has a `y=` that is not a non-negative integer that fits in u32".to_owned()
            }
            Self::SizeAbsent => "has no `size=WxH`".to_owned(),
            Self::SizeMalformed => {
                "has a `size=` that is not a `WxH` of two positive integers".to_owned()
            }
            Self::AnchorUnknown(written) => {
                let names = window_anchor_names();
                match written {
                    Written::Ident(s) => format!("has `anchor={s}`, which is not one of {names}"),
                    Written::Int(n) => format!("has `anchor={n}`, which is not one of {names}"),
                    Written::OtherShape(described) => {
                        format!("has an `anchor=` that is {described}, not one of {names}")
                    }
                    Written::Absent => format!("has no `anchor=` (expected one of {names})"),
                }
            }
            Self::BeforeWallStart {
                anchor,
                offset,
                shift,
            } => format!(
                "has `offset={offset}` at `anchor={anchor}`, which puts its first column at \
                 `offset - {shift}` = -{}, before the left end of its wall",
                shift - offset,
            ),
            Self::BelowBaseRow { anchor, y, shift } => format!(
                "has `y={y}` at `anchor={anchor}`, which puts its lowest row at `y - {shift}` = \
                 -{}, below the base row",
                shift - y,
            ),
        }
    }

    /// The primary of the `E_INVALID_MEMBER` that refuses the window.
    pub(super) fn refusal(&self) -> String {
        format!("window {}", self.clause())
    }

    /// The note on that refusal, saying what the argument takes.
    pub(super) fn fix(&self) -> String {
        match self {
            Self::OffsetMalformed => "write `offset=N`, where `N` is a non-negative integer: the \
                                      window's first column along the wall, counted from 0 at \
                                      its left end seen from outside, or the column of the cell \
                                      its `anchor=` names"
                .to_owned(),
            Self::YAbsent | Self::YMalformed => "write `y=N`, where `N` is a non-negative \
                                                 integer: the window's lowest row, counted up \
                                                 from 0, the base row of the struct or of the \
                                                 window's `level`, or the row of the cell its \
                                                 `anchor=` names"
                .to_owned(),
            Self::SizeAbsent | Self::SizeMalformed => "write `size=WxH`, where `W` and `H` are \
                                                       positive integers: the window's width \
                                                       along the wall and its height in rows"
                .to_owned(),
            Self::AnchorUnknown(_) => format!(
                "write `anchor=` as one of {}: the cell of the window, seen from outside, that \
                 `offset=` and `y=` place; without `anchor=` they place its bottom-left cell",
                window_anchor_names(),
            ),
            Self::BeforeWallStart { anchor, shift, .. } => format!(
                "`anchor={anchor}` names the cell {shift} along from the window's first column, \
                 so `offset=` must be at least {shift}; raise `offset=`, or choose an `anchor=` \
                 further left",
            ),
            Self::BelowBaseRow { anchor, shift, .. } => format!(
                "`anchor={anchor}` names the cell {shift} up from the window's lowest row, so \
                 `y=` must be at least {shift}; raise `y=`, or choose an `anchor=` further down",
            ),
        }
    }
}

/// The window arguments a port and a cut both need, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowArgs {
    /// `offset=`, `0` when absent: the column of the window's anchor.
    pub(super) offset: u32,
    /// `y=`, relative to the member's own level: the row of the window's anchor.
    pub(super) y: u32,
    /// `size=`'s `W`.
    pub(super) width: u32,
    /// `size=`'s `H`.
    pub(super) height: u32,
    /// `anchor=`, or `None` when absent, which places the bottom-left cell.
    pub(super) anchor: Option<WindowAnchor>,
}

impl WindowArgs {
    fn anchor_or_default(self) -> WindowAnchor {
        self.anchor.unwrap_or(WindowAnchor::BOTTOM_LEFT)
    }

    /// Columns from the window's first column to its anchor's.
    fn column_shift(self) -> u32 {
        self.anchor_or_default().along.shift(self.width)
    }

    /// The window's first column along the wall.
    pub(super) fn column(self) -> u32 {
        self.offset - self.column_shift()
    }

    /// The window's lowest row, relative to the member's own level.
    pub(super) fn row(self) -> u32 {
        self.y - self.anchor_or_default().up.shift(self.height)
    }

    /// The window's first column as a formula in `offset`, such as `offset - 1`.
    pub(super) fn column_term(self) -> String {
        match self.column_shift() {
            0 => "offset".to_owned(),
            shift => format!("offset - {shift}"),
        }
    }

    /// ` anchor=NAME` when the line writes one, to follow its `offset=` in a message.
    pub(super) fn anchor_written(self) -> String {
        self.anchor
            .map(|anchor| format!(" anchor={}", anchor.name))
            .unwrap_or_default()
    }
}

/// Read a window's `offset=`, `y=`, `size=` and `anchor=`, in that order.
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
    let anchor = match Written::of_ident(member, "anchor") {
        Written::Absent => None,
        Written::Ident(name) => match WindowAnchor::from_ident(&name) {
            Some(anchor) => Some(anchor),
            None => return Err(WindowArgFault::AnchorUnknown(Written::Ident(name))),
        },
        other => return Err(WindowArgFault::AnchorUnknown(other)),
    };
    let args = WindowArgs {
        offset,
        y,
        width,
        height,
        anchor,
    };
    let at = args.anchor_or_default();
    let shift = at.along.shift(width);
    if offset < shift {
        return Err(WindowArgFault::BeforeWallStart {
            anchor: at.name,
            offset,
            shift,
        });
    }
    let shift = at.up.shift(height);
    if y < shift {
        return Err(WindowArgFault::BelowBaseRow {
            anchor: at.name,
            y,
            shift,
        });
    }
    Ok(args)
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
