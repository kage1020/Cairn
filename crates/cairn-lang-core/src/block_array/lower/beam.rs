//! Laying a `beam`, a run of pillar blocks on their side, along the top row of a wall or over a
//! `door` or `window` (`spec/blockstate` "Derive by default, promote on override").

use std::collections::BTreeSet;

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::BlockState;
use crate::block_array::args::{
    diag_deferred_member_noted, diag_deferred_member_reason, diag_invalid_member, ident_or_ignore,
};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::Refusal;
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{
    MemberState, StructCtx, default_block_id, resolve_member_state, side_name, unplaced_walls_note,
};
use super::opening::placed_opening;
use super::stand::word;

/// Catalog token a `beam`'s default material comes from; the pack spells it per edition.
const BEAM_TOKEN: &str = "beam.default";

/// Beam id used when the pack cannot supply one, in the base edition's spelling.
const BEAM_BASE_ID: &str = "minecraft:oak_log";

/// The note on a `kind=` that names no beam.
const KINDS: &str =
    "`kind=` takes `pillar`, a run of pillar blocks such as logs laid on their side";

/// The note on an `at=` that names nothing a beam rests on, saying what does.
const ANCHORS: &str = "write `at=<side>.top` to lay the beam along the top row of a wall, where \
                       `<side>` is front, back, left or right, or `at=<id>.top` to lay it over \
                       the `door` or `window` with that `id=`";

/// The side names an anchor takes.
const SIDES: [&str; 4] = ["front", "back", "left", "right"];

/// Lay a `beam` where its `at=` names, along its wall unless its line turns it.
pub(super) fn fill_beam(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (axis, unread) = match ident_or_ignore(member, "axis", "`x`, `y` or `z`") {
        Ok(axis) => (axis, None),
        Err(argument) => (None, Some(argument)),
    };
    let not_laid = match lay_beam(member, axis, y_offset, ctx, palette, canvas, diagnostics) {
        Ok(true) => None,
        Ok(false) => Some(
            "this beam is not laid either way — see the finding on the material its `mat_slot=` \
             names",
        ),
        Err(refusal) => {
            diagnostics.push(*refusal);
            Some("this beam is not laid either way — see the finding on the same line")
        }
    };
    diagnostics.extend(unread.map(|argument| {
        argument.report(not_laid.unwrap_or("the beam lies along its wall, as with no `axis=`"))
    }));
}

/// [`fill_beam`], answering whether the beam was laid, or the finding that refuses it; a state
/// literal the target refuses is reported where it is written and lays nothing.
fn lay_beam(
    member: &Member,
    axis: Option<&str>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<bool, Refusal> {
    beam_kind(member)?;
    let axis = word(member, "axis", axis, &["x", "y", "z"])?;
    let run = match rest_of(member, ctx)? {
        Rest::WallTop(side) => wall_top_run(member, side, y_offset, ctx)?,
        Rest::Opening { opening, level } => opening_run(member, opening, level, ctx)?,
    };
    let Some(id) = beam_block_id(member, axis.is_some(), ctx, diagnostics)? else {
        return Ok(false);
    };
    let cells: Vec<_> = run
        .columns
        .iter()
        .filter_map(|&u| {
            wall_local_to_grid(
                run.side,
                u,
                run.row,
                ctx.overhang,
                ctx.interior_w,
                ctx.interior_h,
                ctx.dims,
            )
        })
        .collect();
    if cells.len() != run.columns.len() {
        return Err(Box::new(diag_deferred_member_reason(
            member,
            "beam resolved to a voxel outside the struct's block array",
        )));
    }
    let mut properties = IndexMap::new();
    let axis = axis.unwrap_or_else(|| along(run.side));
    properties.insert("axis".to_owned(), axis.to_owned());
    let index = palette.intern(BlockState { id, properties });
    for cell in cells {
        canvas.paint(cell, || index);
    }
    Ok(true)
}

/// The refusal of a beam whose `kind=` is missing or names no beam.
fn beam_kind(member: &Member) -> Result<(), Refusal> {
    let refuse = |primary: String, notes: Vec<String>| {
        Err(Box::new(diag_invalid_member(member, primary, notes)))
    };
    let Some(raw) = member.arguments.get("kind") else {
        return refuse(
            "beam has no `kind=`, so nothing says what it builds".to_owned(),
            vec![KINDS.to_owned()],
        );
    };
    match &raw.value.kind {
        ValueKind::Ident(kind) if kind == "pillar" => Ok(()),
        ValueKind::Ident(kind) => {
            let mut notes = vec![KINDS.to_owned()];
            notes.extend(nearest_match(kind, ["pillar"]).map(|_| did_you_mean("kind=pillar")));
            refuse(format!("unknown beam `kind={kind}`"), notes)
        }
        _ => refuse(
            format!(
                "beam `kind=` must be `pillar`, not {}",
                raw.value.describe()
            ),
            vec![KINDS.to_owned()],
        ),
    }
}

/// What a beam's `at=` rests it on.
enum Rest<'a> {
    /// The top row of a wall.
    WallTop(WallSide),
    /// The row above a `door` or `window`, written under `level y=level`.
    Opening { opening: &'a Member, level: u32 },
}

/// Parse a beam's `at=`, or the refusal of one that names nothing it rests on.
fn rest_of<'a>(member: &Member, ctx: &StructCtx<'a>) -> Result<Rest<'a>, Refusal> {
    let refuse = |primary: String, hint: Option<String>| {
        let mut notes = vec![ANCHORS.to_owned()];
        notes.extend(hint);
        Box::new(diag_invalid_member(member, primary, notes))
    };
    let Some(raw) = member.arguments.get("at") else {
        return Err(refuse(
            "beam has no `at=`, so nothing says where it lies".to_owned(),
            None,
        ));
    };
    let ValueKind::DotRef(dotref) = &raw.value.kind else {
        return Err(refuse(
            format!(
                "beam `at=` must be a two-segment reference, not {}",
                raw.value.describe()
            ),
            None,
        ));
    };
    let [head, tail] = dotref.segments() else {
        return Err(refuse(
            format!(
                "beam `at={dotref}` has {} segments, and a beam's anchor has two",
                dotref.segments().len(),
            ),
            None,
        ));
    };
    let ids = || {
        ctx.openings
            .iter()
            .filter_map(|(_, opening)| opening.id.as_deref())
    };
    let side = WallSide::from_ident(head);
    let opening = ctx
        .openings
        .iter()
        .find(|(_, opening)| opening.id.as_deref() == Some(head.as_str()));
    let near = || nearest_match(head, SIDES.into_iter().chain(ids()));
    if tail != "top" {
        let known = (side.is_some() || opening.is_some()).then_some(head.as_str());
        return Err(refuse(
            format!("beam `at={dotref}` does not end in `.top`, the top a beam rests on"),
            known
                .or_else(near)
                .map(|head| did_you_mean(&format!("at={head}.top"))),
        ));
    }
    if let Some(side) = side {
        return Ok(Rest::WallTop(side));
    }
    if let Some(&(level, opening)) = opening {
        return Ok(Rest::Opening { opening, level });
    }
    let near = near();
    Err(refuse(
        format!(
            "beam `at={dotref}`: `{head}` is no wall side, and no `door` or `window` in this body \
             has `id={head}`"
        ),
        near.map(|near| did_you_mean(&format!("at={near}.top"))),
    ))
}

/// Where a beam lies: the wall it runs along, the row, and its cells' `u` along that wall.
struct Run {
    side: WallSide,
    row: u32,
    columns: BTreeSet<u32>,
}

/// The length of the `side` wall a beam at `anchor` lies on, or the refusal of a wall with no cell
/// between its corners.
fn between_corners(
    member: &Member,
    anchor: &str,
    side: WallSide,
    ctx: &StructCtx<'_>,
) -> Result<u32, Refusal> {
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    if length >= 3 {
        return Ok(length);
    }
    let name = side_name(side);
    Err(Box::new(diag_invalid_member(
        member,
        format!(
            "beam `{anchor}`: the {name} wall is {length} long, so it has no cell between its \
             corners"
        ),
        vec![
            "a beam runs between the corners of the wall it lies on, so that wall needs a length \
             of at least 3"
                .to_owned(),
        ],
    )))
}

/// The top row of the walls of the beam's own level, those written under the same `level` or,
/// outside one, outside any, between the wall's corners.
fn wall_top_run(
    member: &Member,
    side: WallSide,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Run, Refusal> {
    let name = side_name(side);
    let anchor = format!("at={name}.top");
    let length = between_corners(member, &anchor, side, ctx)?;
    let top = |walls: &[(u32, u32)]| {
        walls
            .iter()
            .filter(|(level, _)| *level == y_offset)
            .map(|(level, height)| level.saturating_add(*height))
            .max()
    };
    let Some(written) = top(&ctx.written.placed_walls) else {
        let base = if y_offset == 0 {
            "the struct's base".to_owned()
        } else {
            format!("`level y={y_offset}`")
        };
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "beam `{anchor}` has no wall to rest on: no `walls` member with a positive \
                 `height=` rises from {base}"
            ),
            vec![
                "a beam along a wall's top rests on the walls of its own level; add a `walls` \
                 member with a positive `height=` beside the beam, or move the beam beside the \
                 walls it rests on"
                    .to_owned(),
            ],
        )));
    };
    if top(&ctx.painting_walls) != Some(written) {
        return Err(Box::new(diag_deferred_member_noted(
            member,
            &format!(
                "beam `{anchor}` rests on row {written} of a `walls` member that paints nothing"
            ),
            "the beam is laid once that member's material resolves to a block",
        )));
    }
    Ok(Run {
        side,
        row: written,
        columns: (1..length - 1).collect(),
    })
}

/// The row above the opening `opening` cuts, written under `level y=level`, across its columns and
/// one more on each side, between the wall's corners.
fn opening_run(
    member: &Member,
    opening: &Member,
    level: u32,
    ctx: &StructCtx<'_>,
) -> Result<Run, Refusal> {
    let keyword = opening.role.keyword();
    let anchor = format!("at={}.top", opening.id.as_deref().unwrap_or_default());
    let Some(cut) = placed_opening(opening, level, ctx) else {
        return Err(Box::new(diag_deferred_member_noted(
            member,
            &format!(
                "beam `{anchor}` rests on a {keyword} that is not placed, so nothing says where its \
                 top is"
            ),
            &format!(
                "the beam is laid once the {keyword} is; see the finding on the {keyword}'s line"
            ),
        )));
    };
    let length = between_corners(member, &anchor, cut.side, ctx)?;
    let columns: BTreeSet<u32> = cut
        .stamps
        .into_iter()
        .flat_map(|(offset, width)| offset.saturating_sub(1)..=offset.saturating_add(width))
        .filter(|&u| u >= 1 && u + 1 < length)
        .collect();
    let row = cut.top.saturating_add(1);
    if ctx.wall_column.course_top_at(row).is_none() {
        return Err(no_wall(
            member,
            &anchor,
            row,
            ctx,
            format!(
                "beam `{anchor}` would lie on row {row}, above the {keyword}, and no `walls` \
                 member reaches that row"
            ),
            format!("raise the `walls` to reach row {row}, or lower the {keyword}"),
        ));
    }
    Ok(Run {
        side: cut.side,
        row,
        columns,
    })
}

/// The finding on a beam over an opening whose `row` no painting `walls` member holds: a deferral
/// when a written one would, else the refusal `primary` with `fix`.
fn no_wall(
    member: &Member,
    anchor: &str,
    row: u32,
    ctx: &StructCtx<'_>,
    primary: String,
    fix: String,
) -> Refusal {
    let written = &ctx.written;
    if written.walls.course_top_at(row).is_some() {
        return Box::new(diag_deferred_member_noted(
            member,
            &format!("beam `{anchor}` rests on row {row} of a `walls` member that paints nothing"),
            "the beam is laid once that member's material resolves to a block",
        ));
    }
    if written.unplaced_walls {
        return Box::new(diag_deferred_member_noted(
            member,
            &format!("beam `{anchor}` finds no wall on row {row}"),
            &unplaced_walls_note("this row", "the beam is laid"),
        ));
    }
    Box::new(diag_invalid_member(member, primary, vec![fix]))
}

/// The `axis` a beam along a `side` wall lies on.
fn along(side: WallSide) -> &'static str {
    match side {
        WallSide::Front | WallSide::Back => "x",
        WallSide::Left | WallSide::Right => "z",
    }
}

/// The block a beam is built from: its `mat_slot=`, or the pack's default beam; `None` for a
/// binding whose state literal the target refused, which lays no beam.
fn beam_block_id(
    member: &Member,
    axis_written: bool,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<String>, Refusal> {
    let resolved = match resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    ) {
        MemberState::Bound(state) => Some(state),
        MemberState::Unbound => None,
        MemberState::Refused => return Ok(None),
    };
    geometry_material_id(
        member,
        ctx.scope,
        ctx.registry,
        resolved.as_ref(),
        || default_block_id(member, ctx.registry, BEAM_TOKEN, BEAM_BASE_ID),
        &GeometryMemberDescription {
            subject: "beam".to_owned(),
            states_from: if axis_written {
                "its line's `axis=`"
            } else {
                "the wall it lies along"
            },
            requires: Some(Family::Pillar),
        },
        diagnostics,
    )
    .map(Some)
}
