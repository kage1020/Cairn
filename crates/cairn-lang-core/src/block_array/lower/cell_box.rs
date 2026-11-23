//! The cells a line names with coordinate triples: a box between `from=` and `to=`, one cell at
//! `at=`, or a column rising from it (`spec/syntax` "Literals and separators"), and where the
//! copies of a series go.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::Bbox;
use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member};
pub(super) use crate::block_array::geometry::Cell;

use super::members::{RowFit, StructCtx};
use super::repeat::Repeat;

/// A corner as its line writes it, measured from the footprint's low corner.
type Corner = (u64, u64, u64);

/// What a line's triples name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// A box between two opposite corners, `from=` and `to=`.
    Box,
    /// The box of an `arch`, between `from=` and `to=` as a [`Shape::Box`]'s.
    Arch,
    /// The one cell `at=` names.
    Cell,
    /// The cells of a `column`, `height` rows up from the one `at=` names.
    Column {
        /// How many rows it rises, at least 1.
        height: u32,
    },
}

impl Shape {
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Box | Self::Arch => &["from", "to"],
            Self::Cell | Self::Column { .. } => &["at"],
        }
    }

    /// How many rows the shape rises above its highest corner.
    fn rise(self) -> u32 {
        match self {
            Self::Box | Self::Arch | Self::Cell => 0,
            Self::Column { height } => height.saturating_sub(1),
        }
    }

    /// The note on a corner that is missing or not a triple, saying what one is.
    fn corners_note(self) -> &'static str {
        match self {
            Self::Box | Self::Arch => {
                "write `from=` and `to=` as two opposite corners of the box, each a coordinate \
                 triple `x,y,z`, as in `from=1,1,1 to=3,2,3`"
            }
            Self::Cell => {
                "write `at=` as the coordinate triple `x,y,z` of the cell the block goes in, as \
                 in `at=1,1,1`"
            }
            Self::Column { .. } => {
                "write `at=` as the coordinate triple `x,y,z` of the cell the column's lowest \
                 block stands in, as in `at=1,1,1`"
            }
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Self::Box | Self::Arch => "which cells its box holds",
            Self::Cell => "which cell its block goes in",
            Self::Column { .. } => "which cell it stands on",
        }
    }

    /// What a deferral note says happens once the member it waits on builds.
    fn built(self) -> &'static str {
        match self {
            Self::Box => "the box is filled",
            Self::Arch => "the arch is cut",
            Self::Cell => "the block is placed",
            Self::Column { .. } => "the column is stood",
        }
    }

    fn rows(self) -> &'static str {
        match self {
            Self::Box | Self::Arch | Self::Column { .. } => "these rows",
            Self::Cell => "this row",
        }
    }

    fn lowered(self) -> &'static str {
        match self {
            Self::Box | Self::Arch => "box",
            Self::Cell => "`at=`",
            Self::Column { .. } => "`at=` or `height=`",
        }
    }

    /// Whose components a bounds note names.
    fn bounded(self) -> &'static str {
        match self {
            Self::Box | Self::Arch => "a corner's",
            Self::Cell | Self::Column { .. } => "the cell's",
        }
    }
}

/// The cells of a box in the block array, both corners included.
#[derive(Debug, Clone, Copy)]
pub(super) struct CellBox {
    low: Cell,
    high: Cell,
}

impl CellBox {
    /// The box with corners `a` and `b`, in either order.
    pub(super) fn spanning(a: Cell, b: Cell) -> Self {
        Self {
            low: (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2)),
            high: (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2)),
        }
    }

    /// The box moved by `by`, which must keep it inside the block array.
    pub(super) fn shifted(self, by: Cell) -> Self {
        let add = |c: Cell| (c.0 + by.0, c.1 + by.1, c.2 + by.2);
        Self {
            low: add(self.low),
            high: add(self.high),
        }
    }

    /// The box around this one and each copy `repeat` writes of it.
    pub(super) fn repeated(self, repeat: Repeat) -> Self {
        // A step is never negative, so the last copy holds the far corner.
        let last = repeat.shifts().last().unwrap_or((0, 0, 0));
        Self {
            low: self.low,
            high: self.shifted(last).high,
        }
    }

    /// The box's corner with the lowest `x`, `y` and `z`, and the one with the highest.
    pub(super) fn corners(self) -> (Cell, Cell) {
        (self.low, self.high)
    }

    /// The box, as a member's geometry records it.
    pub(super) fn bbox(self) -> Bbox {
        Bbox::spanning(self.low, self.high)
    }

    /// Every cell of the box, `x` fastest and `y` slowest.
    pub(super) fn cells(self) -> impl Iterator<Item = Cell> {
        let Self { low, high } = self;
        (low.1..=high.1).flat_map(move |y| {
            (low.2..=high.2).flat_map(move |z| (low.0..=high.0).map(move |x| (x, y, z)))
        })
    }
}

/// The cells `member`'s triples name in the block array, or the finding that leaves it unbuilt.
pub(super) fn written_cells(
    member: &Member,
    shape: Shape,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<CellBox, Box<Diagnostic>> {
    repeated_cells(member, shape, y_offset, Repeat::ONCE, ctx)
}

/// The cells of the first of `repeat`'s copies of `member`, once every copy is known to fit, or
/// the finding that leaves it unbuilt.
pub(super) fn repeated_cells(
    member: &Member,
    shape: Shape,
    y_offset: u32,
    repeat: Repeat,
    ctx: &StructCtx<'_>,
) -> Result<CellBox, Box<Diagnostic>> {
    let mut corners = Vec::with_capacity(2);
    for &key in shape.keys() {
        corners.push((key, corner(member, shape, key)?));
    }
    // A step is never negative, so the last copy is the farthest on every axis.
    for &(key, at) in &corners {
        let far = last_copy(at, repeat);
        if far.0 >= u64::from(ctx.interior_w) || far.2 >= u64::from(ctx.interior_h) {
            return Err(Box::new(outside_the_footprint(
                member, shape, key, at, repeat, ctx,
            )));
        }
    }
    // `max_by_key` keeps the last of a tie, so a flat box names its `to=`.
    let (key, highest) = corners
        .iter()
        .copied()
        .max_by_key(|(_, at)| at.1)
        .expect("a shape names at least one corner");
    let top = Top::of(key, highest, repeat, y_offset, shape.rise());
    match ctx.row_fit(top.row, shape.rows(), shape.built()) {
        RowFit::Fits => {}
        RowFit::Above => return Err(Box::new(above_the_struct(member, shape, top, ctx))),
        // The repair is on another member, so the line only defers.
        RowFit::Waits(note) => {
            return Err(Box::new(diag_deferred_member_noted(
                member,
                &past_the_struct(member, top, ctx),
                &note,
            )));
        }
    }
    // The footprint and row checks above bound every component to the array.
    let cell = |(x, y, z): Corner| {
        let grid = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        (
            grid(x).saturating_add(ctx.overhang),
            grid(y).saturating_add(y_offset),
            grid(z).saturating_add(ctx.overhang),
        )
    };
    let mut cells = corners.iter().map(|&(_, at)| cell(at));
    let first = cells.next().expect("a shape names at least one corner");
    let (low, mut high) = cells.fold((first, first), |(low, high), c| {
        (
            (low.0.min(c.0), low.1.min(c.1), low.2.min(c.2)),
            (high.0.max(c.0), high.1.max(c.1), high.2.max(c.2)),
        )
    });
    high.1 = high.1.saturating_add(shape.rise());
    Ok(CellBox { low, high })
}

/// Where the last of `repeat`'s copies puts the corner written `at`.
fn last_copy(at: Corner, repeat: Repeat) -> Corner {
    let (dx, dy, dz) = repeat.reach();
    (
        at.0.saturating_add(dx),
        at.1.saturating_add(dy),
        at.2.saturating_add(dz),
    )
}

/// A corner as a message names it, with where its last copy lands in a series; `under` completes
/// the phrase before the copy clause closes.
fn corner_named(key: &str, at: Corner, repeat: Repeat, under: &str) -> String {
    let (x, y, z) = at;
    let written = format!("`{key}={x},{y},{z}`");
    if !repeat.repeats() {
        return format!("{written}{under}");
    }
    let (fx, fy, fz) = last_copy(at, repeat);
    format!(
        "{written}, whose last of {} lands at {fx},{fy},{fz}{under},",
        repeat.copies(),
    )
}

/// A line's `key=` corner, or the refusal of a missing one or one that is not a triple.
fn corner(member: &Member, shape: Shape, key: &'static str) -> Result<Corner, Box<Diagnostic>> {
    let keyword = member.role.keyword();
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![shape.corners_note().to_owned()],
        ))
    };
    let Some(raw) = member.arguments.get(key) else {
        return Err(refuse(format!(
            "{keyword} has no `{key}=`, so nothing says {}",
            shape.holds()
        )));
    };
    match raw.value.kind {
        ValueKind::Coord { x, y, z } => Ok((x, y, z)),
        _ => Err(refuse(format!(
            "{keyword} `{key}=` must be a coordinate triple, not {}",
            raw.value.describe(),
        ))),
    }
}

/// The refusal of a corner outside the struct's footprint.
fn outside_the_footprint(
    member: &Member,
    shape: Shape,
    key: &str,
    at: Corner,
    repeat: Repeat,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let (width, depth) = (ctx.interior_w, ctx.interior_h);
    let mut notes = vec![format!(
        "{} `x` runs from 0 to {} and its `z` from 0 to {}",
        shape.bounded(),
        width.saturating_sub(1),
        depth.saturating_sub(1),
    )];
    if repeat.repeats() {
        notes.push(format!("lower the {}", repeat.keys()));
    }
    diag_invalid_member(
        member,
        format!(
            "{} {} lies outside the struct's `size={width}x{depth}` footprint",
            member.role.keyword(),
            corner_named(key, at, repeat, ""),
        ),
        notes,
    )
}

/// The highest of a line's corners, and the struct row it reaches.
#[derive(Debug, Clone, Copy)]
struct Top {
    key: &'static str,
    corner: Corner,
    repeat: Repeat,
    /// The `level y=` the line is written under.
    level: u32,
    /// The rows the shape rises above the corner.
    rise: u32,
    row: u64,
}

impl Top {
    fn of(key: &'static str, corner: Corner, repeat: Repeat, level: u32, rise: u32) -> Self {
        let row = last_copy(corner, repeat)
            .1
            .saturating_add(u64::from(level))
            .saturating_add(u64::from(rise));
        Self {
            key,
            corner,
            repeat,
            level,
            rise,
            row,
        }
    }

    /// The corner as a message names it, with the column's height and the struct row it reaches
    /// when either moves that row off the corner's.
    fn named(self) -> String {
        let height = if self.rise == 0 {
            String::new()
        } else {
            format!(" with `height={}`", self.rise.saturating_add(1))
        };
        let under = if self.level == 0 {
            String::new()
        } else {
            format!(" under `level y={}`", self.level)
        };
        let row = if self.rise > 0 {
            format!(" (its top in row {})", self.row)
        } else if self.level > 0 {
            format!(" (row {})", self.row)
        } else {
            String::new()
        };
        corner_named(
            self.key,
            self.corner,
            self.repeat,
            &format!("{height}{under}{row}"),
        )
    }
}

/// The primary of a line whose top row is at or above `dims.y`.
fn past_the_struct(member: &Member, top: Top, ctx: &StructCtx<'_>) -> String {
    format!(
        "{} {} does not fit in the struct (dims.y={})",
        member.role.keyword(),
        top.named(),
        ctx.dims.y,
    )
}

/// The refusal of a line reaching above every row the struct as written reaches.
fn above_the_struct(member: &Member, shape: Shape, top: Top, ctx: &StructCtx<'_>) -> Diagnostic {
    let keyword = member.role.keyword();
    let struct_top = ctx.written.height.saturating_sub(1);
    let level = top.level;
    let runs_to = |highest: u32| format!("{} `y` runs from 0 to {highest}", shape.bounded());
    let mut notes = vec![if level == 0 {
        format!("{}, the struct's top row", runs_to(struct_top))
    } else if let Some(highest) = struct_top.checked_sub(level) {
        format!(
            "{} under `level y={level}`, the struct's top row being {struct_top}",
            runs_to(highest),
        )
    } else {
        format!("the struct's top row is {struct_top}, below this {keyword}'s `level y={level}`")
    }];
    notes.extend(ctx.unread_roof_note());
    let built = ctx.dims.y;
    if built <= struct_top {
        let rows = if built == struct_top {
            format!("row {built} waits")
        } else {
            format!("rows {built} to {struct_top} wait")
        };
        notes.push(format!("{rows} on {}", ctx.shortfall().members()));
    }
    let lower = if level > struct_top {
        "`level`"
    } else {
        shape.lowered()
    };
    let copies = if top.repeat.repeats() {
        format!(" or its {}", top.repeat.keys())
    } else {
        String::new()
    };
    notes.push(format!(
        "lower the {keyword}'s {lower}{copies}, or raise the `walls` to reach its rows"
    ));
    diag_invalid_member(member, past_the_struct(member, top, ctx), notes)
}
