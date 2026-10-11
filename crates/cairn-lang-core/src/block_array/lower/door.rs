//! Cutting the opening of a `door` member.

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::PaletteIndex;
use crate::block_array::args::diag_deferred_member_reason;
use crate::block_array::openings::{
    door_anchor_offset, door_at_deferral, wall_length, wall_local_to_grid,
};

use super::canvas::MemberCanvas;
use super::members::{StructCtx, side_name, side_of};

/// Rows a doorway opens where the course it is cut into has room for
/// them: the head height of a Minecraft door.
///
/// A ceiling rather than a size. The rows are counted from the row the
/// door opens at, that row included, so a course whose top *is* that row
/// yields an opening one row tall — the row above a short course is the
/// roof or the gap over the wall, not masonry to cut. See [`carve_door`].
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
    // A `connect` port on this door reads `at=` and words its refusal
    // through these same functions, so the cut, the walkway and both
    // messages agree. Asked before the wall, as `fill_window` reads its
    // rectangle first, so an `at=` typo is reported even on a body whose
    // walls are wrong too.
    let at = match door_anchor_offset(member, len) {
        Ok(at) => at,
        Err(written) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &door_at_deferral(&written),
            ));
            return;
        }
    };
    // Gate and cap come from the course holding the row the door opens
    // at, `y_offset + 1` (the floor slab owns the base plane), never from
    // the tallest wall row: over a shorter course stands the gap or the
    // roof at `wall_top + 1`, which a door judged against `wall_top` would
    // carve. `fill_window` asks the same column, so the two agree.
    let base_row = y_offset.saturating_add(1);
    let Some(course_top) = ctx.wall_column.course_top_at(base_row) else {
        // "Nowhere" and "not here" are different findings. An empty
        // column means no `walls` paints, which a positive `height=` does
        // not settle: an unresolved `mat_slot=` empties it too, so naming
        // only the height would send the author of `height=4` over a
        // themeless struct to the wrong line. Same split as `fill_window`'s.
        let reason = if ctx.wall_column.is_empty() {
            "door requires a `walls` member that paints — a positive `height=` and a `mat_slot=` that resolves — to carve into".to_owned()
        } else {
            format!(
                "door opens at y={base_row}, which is not inside any wall course (the walls occupy {})",
                ctx.wall_column,
            )
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return;
    };
    // Only the opening is cut; no door block (`oak_door` with hinge,
    // half, facing, open) is placed. A door's `mat_slot=` is what that
    // block would read, so `MemberRole::unread_arguments` lists it as
    // unreached on a `door`.
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
            // INVARIANT(wall-grid-validated): `at < len` by the match
            // above and `door_height` keeps every row within `course_top`,
            // so `wall_local_to_grid`'s `u < length` / `v < dims.y` cannot
            // refuse. Release builds skip the cell rather than panic.
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
