//! Where a one-cell member set against a wall, a `pressure_plate` or a `chest`, stands
//! (`spec/redstone` "Signal binding").

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{
    diag_deferred_member_noted, diag_deferred_member_reason, diag_invalid_member,
};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::members::{StructCtx, side_name, unplaced_walls_note};
use super::stair::shift_outward;

/// A one-cell member set against a wall, as its findings name it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fixture {
    /// Its keyword: `pressure_plate`.
    pub(super) keyword: &'static str,
    /// What a note calls it: `plate`.
    pub(super) noun: &'static str,
}

/// The note on an `at=` that names no anchor, saying what one is.
const ANCHOR_FORMS: &str = "write `at=<side>.outside` to sit outside a wall or `at=inside.<side>` \
                            to sit inside it, where `<side>` is front, back, left or right";

/// The note on an `at=inside.<side>` fixture on a struct with no interior.
const INTERIOR_NEEDS: &str = "an interior needs a size of at least 3 on both axes";

/// The finding that leaves a fixture unplaced.
pub(super) type Refusal = Box<Diagnostic>;

/// The side names an anchor takes.
const SIDE_NAMES: [&str; 4] = ["front", "back", "left", "right"];

/// The words that say which face of the wall an anchor sits on.
const PLACE_NAMES: [&str; 2] = ["outside", "inside"];

/// Shift a wall voxel's `(x, z)` one voxel toward the interior, saturating at 0 and unchecked.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(-dx), z.saturating_add_signed(-dz))
}

/// Which side of the wall a fixture's `at=<side>.outside | inside.<side>` sets it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Anchor {
    Outside(WallSide),
    Inside(WallSide),
}

impl Anchor {
    /// The wall it stands by.
    pub(super) fn side(self) -> WallSide {
        match self {
            Self::Outside(s) | Self::Inside(s) => s,
        }
    }
}

/// Parse a fixture's `at=` value, or the refusal of any other shape.
fn anchor_of(fixture: Fixture, member: &Member) -> Result<Anchor, Refusal> {
    let keyword = fixture.keyword;
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![ANCHOR_FORMS.to_owned()],
        ))
    };
    let Some(raw) = member.arguments.get("at") else {
        return Err(refuse(format!(
            "{keyword} has no `at=`, so nothing says which wall it sits by"
        )));
    };
    let ValueKind::DotRef(dotref) = &raw.value.kind else {
        let mut refusal = refuse(format!(
            "{keyword} `at=` must be a two-segment reference, not {}",
            raw.value.describe(),
        ));
        if let ValueKind::Ident(word) = &raw.value.kind
            && let Some(side) = known_or_nearest(word, &SIDE_NAMES)
        {
            refusal.notes.push(DiagnosticNote {
                span: None,
                message: format!("did you mean `at={side}.outside` or `at=inside.{side}`?"),
            });
        }
        return Err(refusal);
    };
    let segments = dotref.segments();
    if segments.len() != 2 {
        return Err(refuse(format!(
            "{keyword} `at={dotref}` has {} segments, and an anchor has two",
            segments.len(),
        )));
    }
    let (head, tail) = (segments[0].as_str(), segments[1].as_str());
    let refuse_with_suggestion = |primary: String, note: String| {
        let mut notes = vec![note];
        notes.extend(intended_anchor(head, tail).map(|anchor| did_you_mean(&anchor)));
        Box::new(diag_invalid_member(member, primary, notes))
    };
    let not_a_side = |name: &str, example: &str| {
        refuse_with_suggestion(
            format!("{keyword} `at={dotref}`: `{name}` is not a wall side"),
            format!("a side is one of front, back, left, right, as in `at={example}`"),
        )
    };
    if tail == "outside" {
        return WallSide::from_ident(head)
            .map(Anchor::Outside)
            .ok_or_else(|| not_a_side(head, "front.outside"));
    }
    if head == "inside" {
        return WallSide::from_ident(tail)
            .map(Anchor::Inside)
            .ok_or_else(|| not_a_side(tail, "inside.front"));
    }
    Err(refuse_with_suggestion(
        format!("{keyword} `at={dotref}` is not an anchor"),
        ANCHOR_FORMS.to_owned(),
    ))
}

/// `word` as it appears in `vocabulary`, or the entry one typo from it.
fn known_or_nearest(word: &str, vocabulary: &[&'static str]) -> Option<&'static str> {
    vocabulary
        .iter()
        .copied()
        .find(|known| *known == word)
        .or_else(|| nearest_match(word, vocabulary.iter().copied()))
}

/// The `at=` anchor `head.tail` was likely meant as, when a segment is misspelled or the two are
/// swapped.
fn intended_anchor(head: &str, tail: &str) -> Option<String> {
    let (side, place) = match (
        known_or_nearest(head, &SIDE_NAMES),
        known_or_nearest(tail, &PLACE_NAMES),
    ) {
        (Some(side), Some(place)) => (side, place),
        _ => (
            known_or_nearest(tail, &SIDE_NAMES)?,
            known_or_nearest(head, &PLACE_NAMES)?,
        ),
    };
    Some(if place == "outside" {
        format!("at={side}.outside")
    } else {
        format!("at=inside.{side}")
    })
}

/// Read a fixture's `key=` as a non-negative integer, `0` when absent.
fn coordinate(fixture: Fixture, member: &Member, key: &str, counts: &str) -> Result<u32, Refusal> {
    let Some(raw) = member.arguments.get(key) else {
        return Ok(0);
    };
    member.nonneg_u32(key).ok_or_else(|| {
        Box::new(diag_invalid_member(
            member,
            format!(
                "{} `{key}=` must be a non-negative integer that fits in u32, not {}",
                fixture.keyword,
                raw.value.describe(),
            ),
            vec![format!("`{key}=` counts {counts}")],
        ))
    })
}

/// The anchor a fixture's line names and the cell it takes there, or the finding that leaves it
/// unplaced.
///
/// `<side>.outside` lands only in the overhang ring, and `inside.<side>` only strictly inside the
/// wall ring.
pub(super) fn fixture_cell(
    fixture: Fixture,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<(Anchor, (u32, u32, u32)), Refusal> {
    let anchor = anchor_of(fixture, member)?;
    let side = anchor.side();
    let offset = coordinate(
        fixture,
        member,
        "offset",
        "cells along the wall from 0, its left end seen from outside",
    )?;
    let row = Row {
        y: coordinate(
            fixture,
            member,
            "y",
            &format!(
                "rows up from 0, the base row of the struct or of the {}'s `level`",
                fixture.noun
            ),
        )?,
        level: y_offset,
    };
    let y_world = row.world();
    // The fixture's own line is asked first, so its fault is an error even when another member is
    // missing too.
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    if offset >= length {
        return Err(Box::new(past_the_wall(
            fixture, member, anchor, offset, length, ctx,
        )));
    }
    if let Anchor::Inside(_) = anchor
        && let Some((primary, notes)) = inside_refusal(fixture, side, offset, length, y_world, ctx)
    {
        return Err(Box::new(diag_invalid_member(member, primary, notes)));
    }
    let written = &ctx.written;
    let above_written = y_world >= written.height;
    if above_written && !written.unplaced_walls && y_world >= written.reach {
        return Err(Box::new(above_the_struct(
            fixture, member, anchor, row, ctx,
        )));
    }
    let outside = matches!(anchor, Anchor::Outside(_));
    if outside && written.overhang == 0 {
        return Err(Box::new(no_exterior_voxel(
            fixture, member, side, length, row, ctx,
        )));
    }
    // From here the repair is on another member, so the fixture only defers.
    if let Some((primary, note)) = displaced(fixture, side, row, outside, ctx) {
        return Err(Box::new(diag_deferred_member_noted(
            member, &primary, &note,
        )));
    }
    let Some((wx, _wy, wz)) = wall_local_to_grid(
        side,
        offset,
        y_world,
        ctx.overhang,
        ctx.interior_w,
        ctx.interior_h,
        ctx.dims,
    ) else {
        // INVARIANT(wall-grid-validated): the `offset` and `dims.y` refusals above are every
        // rejection `wall_local_to_grid` makes.
        debug_assert!(
            false,
            "{} cell u={offset} y={y_world} on the {} wall is outside the wall grid; the \
             `offset < wall_length` / `y_world < dims.y` checks stopped agreeing with \
             `wall_local_to_grid`",
            fixture.keyword,
            side_name(side),
        );
        return Err(Box::new(outside_the_array(fixture, member)));
    };
    let (sx, sz) = match anchor {
        Anchor::Outside(_) => shift_outward(side, wx, wz),
        Anchor::Inside(_) => shift_inward(side, wx, wz),
    };
    Ok((anchor, (sx, y_world, sz)))
}

/// The primary and note of the deferral that leaves a fixture unplaced until another member is
/// repaired, or `None` when nothing displaces it.
fn displaced(
    fixture: Fixture,
    side: WallSide,
    row: Row,
    outside: bool,
    ctx: &StructCtx<'_>,
) -> Option<(String, String)> {
    let written = &ctx.written;
    let y_world = row.world();
    let above_written = y_world >= written.height;
    let fits = format!("the {} fits", fixture.noun);
    if above_written && written.unplaced_walls {
        Some((
            row_past_the_struct(fixture, row, ctx),
            unplaced_walls_note("this row", &fits),
        ))
    } else if above_written {
        Some((
            row_past_the_struct(fixture, row, ctx),
            format!(
                "a `roof` whose `kind=` or `slope_to=` does not read may rise to this row; {fits} \
                 once that roof draws"
            ),
        ))
    } else if y_world >= ctx.dims.y {
        Some((
            row_past_the_struct(fixture, row, ctx),
            ctx.shortfall().note(&fits),
        ))
    } else if outside && ctx.overhang == 0 {
        // An overhang a roof is written with and does not draw is the roof's to repair.
        Some((
            no_exterior_primary(fixture, side, row),
            format!(
                "a `roof` written with an `overhang=` draws nothing, so the struct has no overhang \
                 to hold the {}; it fits once that roof draws",
                fixture.noun,
            ),
        ))
    } else {
        None
    }
}

/// The finding on a fixture whose cell lies outside the struct's block array.
pub(super) fn outside_the_array(fixture: Fixture, member: &Member) -> Diagnostic {
    diag_deferred_member_reason(
        member,
        &format!(
            "{} resolved to a voxel outside the struct's block array",
            fixture.keyword
        ),
    )
}

/// A fixture's `y=` and the `level y=` it is written under.
#[derive(Debug, Clone, Copy)]
struct Row {
    y: u32,
    level: u32,
}

impl Row {
    /// The struct row the fixture stands on.
    fn world(self) -> u32 {
        self.y.saturating_add(self.level)
    }

    /// The row as a message names it: `y=N`, or under a raised level the level and world row too.
    fn named(self) -> String {
        if self.level == 0 {
            format!("y={}", self.y)
        } else {
            format!(
                "y={} under `level y={}` (row {})",
                self.y,
                self.level,
                self.world()
            )
        }
    }
}

/// The primary of a fixture whose row is at or above `dims.y`.
fn row_past_the_struct(fixture: Fixture, row: Row, ctx: &StructCtx<'_>) -> String {
    format!(
        "{} {} does not fit in the struct (dims.y={})",
        fixture.keyword,
        row.named(),
        ctx.dims.y,
    )
}

/// The refusal of a fixture whose row is above every row the struct as written reaches.
fn above_the_struct(
    fixture: Fixture,
    member: &Member,
    anchor: Anchor,
    row: Row,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let noun = fixture.noun;
    let top = ctx.written.height.saturating_sub(1);
    let level = row.level;
    let runs_to = |highest: u32| {
        if highest == 0 {
            "`y=` takes only 0".to_owned()
        } else {
            format!("`y=` runs from 0 to {highest}")
        }
    };
    let mut notes = vec![if level == 0 {
        format!("{}, the struct's top row", runs_to(top))
    } else if let Some(highest) = top.checked_sub(level) {
        format!(
            "{} under `level y={level}`, the struct's top row being {top}",
            runs_to(highest),
        )
    } else {
        format!("the struct's top row is {top}, below this {noun}'s `level y={level}`")
    }];
    notes.extend(ctx.unread_roof_note());
    let built = ctx.dims.y;
    if built <= top {
        let rows = if built == top {
            format!("row {built} waits")
        } else {
            format!("rows {built} to {top} wait")
        };
        notes.push(format!("{rows} on {}", ctx.shortfall().members()));
    }
    // Under a level above the top row, no `y=` brings the fixture down far enough.
    notes.push(if level > top {
        format!("lower the {noun}'s `level`, or raise the `walls` to reach its row")
    } else {
        format!("lower the {noun}, or raise the `walls` to reach its row")
    });
    notes.extend(outside_needs_a_roof(fixture, anchor, ctx));
    diag_invalid_member(member, row_past_the_struct(fixture, row, ctx), notes)
}

/// The fix in a struct's body an `at=<side>.outside` fixture needs too, when no roof on it is
/// written with an overhang.
fn outside_needs_a_roof(fixture: Fixture, anchor: Anchor, ctx: &StructCtx<'_>) -> Option<String> {
    match anchor {
        Anchor::Outside(side) if ctx.written.overhang == 0 => Some(format!(
            "an `at={}.outside` {} also needs a `roof` in the struct's body with an \
             `overhang=` of at least 1",
            side_name(side),
            fixture.noun,
        )),
        _ => None,
    }
}

/// The refusal of a fixture whose `offset=` is at or past its wall's length.
fn past_the_wall(
    fixture: Fixture,
    member: &Member,
    anchor: Anchor,
    offset: u32,
    length: u32,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let name = side_name(anchor.side());
    let noun = fixture.noun;
    let thin = thin_axes(ctx);
    let notes = match anchor {
        Anchor::Outside(_) => std::iter::once(format!(
            "an `at={name}.outside` {noun} takes an `offset=` from 0 to {}",
            length - 1,
        ))
        .chain(outside_needs_a_roof(fixture, anchor, ctx))
        .collect(),
        Anchor::Inside(_) if thin.is_empty() => vec![format!(
            "an `at=inside.{name}` {noun} takes an `offset=` {}, clear of the wall's corners",
            inside_offsets(length),
        )],
        Anchor::Inside(_) => vec![
            format!(
                "the struct's {}, so no `offset=` reaches an interior voxel; {INTERIOR_NEEDS}",
                thin_sizes(&thin),
            ),
            format!(
                "grow {} and use an `offset=` {}",
                thin_growth(&thin),
                inside_offsets(length),
            ),
        ],
    };
    diag_invalid_member(
        member,
        format!(
            "{} `offset={offset}` runs past the {name} wall (length {length})",
            fixture.keyword
        ),
        notes,
    )
}

/// The size axes of `ctx` under 3, each with its extent, `w` before `h`.
fn thin_axes(ctx: &StructCtx<'_>) -> Vec<(&'static str, u32)> {
    [("w", ctx.interior_w), ("h", ctx.interior_h)]
        .into_iter()
        .filter(|&(_, extent)| extent < 3)
        .collect()
}

/// `thin` as "size.w is 2 and size.h is 1".
fn thin_sizes(thin: &[(&str, u32)]) -> String {
    thin.iter()
        .map(|(axis, extent)| format!("size.{axis} is {extent}"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// `thin` as the growth that gives the struct an interior, completing "grow …".
fn thin_growth(thin: &[(&str, u32)]) -> String {
    thin.iter()
        .map(|(axis, _)| format!("`size.{axis}` to at least 3"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The `offset=` values an `at=inside.<side>` fixture on a wall `length` long takes, completing
/// "an `offset=` …".
fn inside_offsets(length: u32) -> String {
    match length {
        0..=2 => "away from both ends of the wall".to_owned(),
        3 => "of 1".to_owned(),
        _ => format!("from 1 to {}", length - 2),
    }
}

/// The primary of an `at=<side>.outside` fixture with no exterior voxel beside it.
fn no_exterior_primary(fixture: Fixture, side: WallSide, row: Row) -> String {
    format!(
        "{} `at={}.outside` at {} has no exterior voxel to sit on",
        fixture.keyword,
        side_name(side),
        row.named(),
    )
}

/// The refusal of an `at=<side>.outside` fixture beside no roof written with an overhang, on a
/// wall `length` long.
fn no_exterior_voxel(
    fixture: Fixture,
    member: &Member,
    side: WallSide,
    length: u32,
    row: Row,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let roof = "give a `roof` in the struct's body an `overhang=` of at least 1";
    let fix = if thin_axes(ctx).is_empty() {
        // On row 0 an inside fixture would replace a floor block, one below the floor's top.
        let (raised, on_the_floor) = if row.world() == 0 && ctx.written.floor {
            (" y=1", ", on top of the floor")
        } else {
            ("", "")
        };
        format!(
            "{roof}, or use `at=inside.{}{raised}` with an `offset=` {}{on_the_floor}",
            side_name(side),
            inside_offsets(length),
        )
    } else {
        roof.to_owned()
    };
    diag_invalid_member(
        member,
        no_exterior_primary(fixture, side, row),
        vec![
            "no roof on this struct draws an overhang, so it has no voxel outside its walls"
                .to_owned(),
            fix,
        ],
    )
}

/// Why an `at=inside.<side>` fixture at an `offset=` within its wall's `length` has no interior
/// cell, as a primary and its notes, or `None`.
///
/// The interior is strictly inside the footprint's wall ring, whether or not walls paint it.
fn inside_refusal(
    fixture: Fixture,
    side: WallSide,
    offset: u32,
    length: u32,
    y: u32,
    ctx: &StructCtx<'_>,
) -> Option<(String, Vec<String>)> {
    let name = side_name(side);
    let keyword = fixture.keyword;
    let thin = thin_axes(ctx);
    let corner = offset == 0 || offset == length - 1;
    if thin.is_empty() && !corner {
        return None;
    }
    let valid_offsets = inside_offsets(length);
    if thin.is_empty() {
        let inside_it = if ctx.written.walls.course_top_at(y).is_some() {
            "belongs to the neighbouring wall"
        } else {
            "is not an interior cell of this struct"
        };
        return Some((
            format!(
                "{keyword} `at=inside.{name} offset={offset}` is at a corner of the {name} \
                 wall, so the voxel inside it {inside_it}",
            ),
            vec![format!(
                "use an `offset=` {valid_offsets} to reach an interior voxel"
            )],
        ));
    }
    let also_corner = if corner {
        format!(", and `offset={offset}` is at a corner of the {name} wall besides")
    } else {
        String::new()
    };
    let use_offset = if corner {
        format!(" and use an `offset=` {valid_offsets}")
    } else {
        String::new()
    };
    // `<side>.outside` finds a cell only in a drawn overhang.
    let outside = if ctx.overhang > 0 && y < ctx.dims.y {
        format!(", or anchor the {} with `at={name}.outside`", fixture.noun)
    } else {
        String::new()
    };
    Some((
        format!(
            "{keyword} `at=inside.{name} offset={offset}`: the struct's {}, so it has no \
             interior voxel{also_corner}",
            thin_sizes(&thin),
        ),
        vec![
            INTERIOR_NEEDS.to_owned(),
            format!("grow {}{use_offset}{outside}", thin_growth(&thin)),
        ],
    ))
}
