//! Cutting the opening of a `door` member.

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::PaletteIndex;
use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member};
use crate::block_array::openings::{
    door_anchor_offset, door_at_refusal, wall_length, wall_local_to_grid,
};

use super::canvas::MemberCanvas;
use super::members::{StructCtx, UNPAINTED_COURSE, side_name, side_of};

/// Most rows a doorway opens, counted from the row it opens at; a shorter course cuts fewer.
const DOOR_HEIGHT: u32 = 2;

pub(super) fn carve_door(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    // Asked before the wall, so an `at=` typo is reported even when the walls are wrong too.
    let at = match door_anchor_offset(member, len) {
        Ok(at) => at,
        Err(written) => {
            diagnostics.push(diag_invalid_member(
                member,
                door_at_refusal(&written),
                Vec::new(),
            ));
            return;
        }
    };
    // Judged against the course holding the door's row, never the tallest wall row.
    let base_row = y_offset.saturating_add(1);
    let Some(course_top) = ctx.wall_column.course_top_at(base_row) else {
        diagnostics.push(door_course_refusal(member, base_row, ctx));
        return;
    };
    // Only the opening is cut; no door block is placed.
    let door_height = course_top
        .saturating_sub(base_row)
        .saturating_add(1)
        .min(DOOR_HEIGHT);
    for v_local in 0..door_height {
        let v = base_row.saturating_add(v_local);
        let Some((x, y, z)) = wall_local_to_grid(
            side,
            at,
            v,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            // INVARIANT(wall-grid-validated): `at < len`, and `door_height` stays in `course_top`.
            debug_assert!(
                false,
                "door row y={v} at u={at} on the {} wall is outside the wall grid; \
                 `at=` against `wall_length` / `door_height` against the course \
                 stopped agreeing with `wall_local_to_grid`",
                side_name(side),
            );
            continue;
        };
        canvas.paint((x, y, z), || PaletteIndex::AIR);
    }
}

/// The finding that refuses a door opening at `base_row`, which no wall course holds.
///
/// A row a `walls` member is written to hold and does not paint is that member's to repair.
fn door_course_refusal(member: &Member, base_row: u32, ctx: &StructCtx<'_>) -> Diagnostic {
    if ctx.written_walls.course_top_at(base_row).is_some() {
        let primary = if ctx.wall_column.is_empty() {
            "door has no wall to cut into: no `walls` member on this struct paints".to_owned()
        } else {
            format!(
                "door opens at y={base_row}, in a course the walls are written to ({}) but not in one they paint ({})",
                ctx.written_walls, ctx.wall_column,
            )
        };
        return diag_deferred_member_noted(member, &primary, UNPAINTED_COURSE);
    }
    let (primary, fix) = if ctx.written_walls.is_empty() {
        (
            "door has no wall to cut into: this struct declares no `walls` with a positive `height=`"
                .to_owned(),
            "add a `walls` member whose course holds the row the door opens at",
        )
    } else {
        (
            format!(
                "door opens at y={base_row}, which is not inside any wall course (the walls occupy {})",
                ctx.written_walls,
            ),
            "a door opens one row above the base of its `level`, or of the body outside one; \
             move it under a `level` whose walls hold that row, or add walls there",
        )
    };
    diag_invalid_member(member, primary, vec![fix.to_owned()])
}
