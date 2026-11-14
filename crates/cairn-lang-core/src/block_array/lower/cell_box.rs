//! The cells a line names with coordinate triples: a `fill`'s or `raw_fill`'s box between `from=`
//! and `to=`, or a `raw_block`'s one cell at `at=` (`spec/syntax` "Literals and separators"), and
//! for a raw member under a `raw_repeat`, where its copies go.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member};

use super::members::{StructCtx, unplaced_walls_note};
use super::repeat::Repeat;

/// A cell of the block array, `(x, y, z)`.
pub(super) type Cell = (u32, u32, u32);

/// A corner as its line writes it, measured from the footprint's low corner.
type Corner = (u64, u64, u64);

/// What a line's triples name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shape {
    /// A box between two opposite corners, `from=` and `to=`.
    Box,
    /// The one cell `at=` names.
    Cell,
}

impl Shape {
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Box => &["from", "to"],
            Self::Cell => &["at"],
        }
    }

    /// The note on a corner that is missing or not a triple, saying what one is.
    fn corners_note(self) -> &'static str {
        match self {
            Self::Box => {
                "write `from=` and `to=` as two opposite corners of the box, each a coordinate \
                 triple `x,y,z`, as in `from=1,1,1 to=3,2,3`"
            }
            Self::Cell => {
                "write `at=` as the coordinate triple `x,y,z` of the cell the block goes in, as \
                 in `at=1,1,1`"
            }
        }
    }

    fn holds(self) -> &'static str {
        match self {
            Self::Box => "which cells its box holds",
            Self::Cell => "which cell its block goes in",
        }
    }

    /// What a deferral note says happens once the member it waits on builds.
    fn built(self) -> &'static str {
        match self {
            Self::Box => "the box is filled",
            Self::Cell => "the block is placed",
        }
    }

    fn rows(self) -> &'static str {
        match self {
            Self::Box => "these rows",
            Self::Cell => "this row",
        }
    }

    fn lowered(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Cell => "`at=`",
        }
    }

    /// Whose components a bounds note names.
    fn bounded(self) -> &'static str {
        match self {
            Self::Box => "a corner's",
            Self::Cell => "the cell's",
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
    /// The box moved by `by`, which must keep it inside the block array.
    pub(super) fn shifted(self, by: Cell) -> Self {
        let add = |c: Cell| (c.0 + by.0, c.1 + by.1, c.2 + by.2);
        Self {
            low: add(self.low),
            high: add(self.high),
        }
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
    let top = Top::of(key, highest, repeat, y_offset);
    let written = &ctx.written;
    let above_written = top.row >= written.height;
    if above_written && !written.unplaced_walls && top.row >= written.reach {
        return Err(Box::new(above_the_struct(member, shape, top, ctx)));
    }
    // From here the repair is on another member, so the line only defers.
    let deferral = if above_written && written.unplaced_walls {
        Some(unplaced_walls_note(shape.rows(), shape.built()))
    } else if above_written {
        Some(format!(
            "a `roof` whose `kind=` or `slope_to=` does not read may rise to {}; {} once that \
             roof draws",
            shape.rows(),
            shape.built(),
        ))
    } else if top.row >= ctx.dims.y {
        Some(ctx.shortfall().note(shape.rows(), shape.built()))
    } else {
        None
    };
    if let Some(note) = deferral {
        return Err(Box::new(diag_deferred_member_noted(
            member,
            &past_the_struct(member, top, ctx),
            &note,
        )));
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
    let (low, high) = cells.fold((first, first), |(low, high), c| {
        (
            (low.0.min(c.0), low.1.min(c.1), low.2.min(c.2)),
            (high.0.max(c.0), high.1.max(c.1), high.2.max(c.2)),
        )
    });
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

/// A corner as a message names it, with where its last copy lands under a `raw_repeat`; `under`
/// completes the phrase before the copy clause closes.
fn corner_named(key: &str, at: Corner, repeat: Repeat, under: &str) -> String {
    let (x, y, z) = at;
    let written = format!("`{key}={x},{y},{z}`");
    if !repeat.repeats() {
        return format!("{written}{under}");
    }
    let (fx, fy, fz) = last_copy(at, repeat);
    format!(
        "{written}, whose last of {} `raw_repeat` copies lands at {fx},{fy},{fz}{under},",
        repeat.count(),
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
        notes.push("lower the `raw_repeat`'s `count=` or `step=`".to_owned());
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
    row: u32,
}

impl Top {
    fn of(key: &'static str, corner: Corner, repeat: Repeat, level: u32) -> Self {
        let y = u32::try_from(last_copy(corner, repeat).1).unwrap_or(u32::MAX);
        Self {
            key,
            corner,
            repeat,
            level,
            row: y.saturating_add(level),
        }
    }

    /// The corner as a message names it, with the level and struct row under a raised level.
    fn named(self) -> String {
        let under = if self.level == 0 {
            String::new()
        } else {
            format!(" under `level y={}` (row {})", self.level, self.row)
        };
        corner_named(self.key, self.corner, self.repeat, &under)
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
        " or its `raw_repeat`'s `count=` or `step=`"
    } else {
        ""
    };
    notes.push(format!(
        "lower the {keyword}'s {lower}{copies}, or raise the `walls` to reach its rows"
    ));
    diag_invalid_member(member, past_the_struct(member, top, ctx), notes)
}
