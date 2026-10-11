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
    // `at=` is read by `block_array::openings::door_anchor_offset`, the function
    // a `connect` port on this door reads it with, and a refusal is worded
    // by `door_at_deferral` from the same classification the port's note
    // uses — so the cut, the walkway and both messages cannot disagree
    // about what `at=` says. Asked before the wall below, the way
    // `fill_window` reads its rectangle before asking where the masonry
    // is, so an `at=` typo is reported as one on a body whose walls are
    // also wrong.
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
    // Gate and cap both come from the course holding the row this door
    // opens at — `y_offset + 1`, one above the level's base plane, which
    // the floor slab owns — and never from the struct's tallest wall
    // row: what stands over a shorter course is the gap above it or the
    // roof the envelope phase wrote at `wall_top + 1`, so a door judged
    // against `wall_top` carved that air, and one gated on "does the
    // column hold anything" carved it without a word. `fill_window` asks
    // the same column where its rectangle lands, so the two members
    // answer one question.
    let base_row = y_offset.saturating_add(1);
    let Some(course_top) = ctx.wall_column.course_top_at(base_row) else {
        // "Nowhere" and "not here" are different findings. An empty
        // column means no `walls` member paints at all — which a
        // positive `height=` does not settle, since a `mat_slot=` that
        // does not resolve empties it too, and naming only the height
        // would send an author who wrote `height=4` over a themeless
        // struct to the wrong line. Same split as `fill_window`'s.
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
    // The door block itself (`oak_door`, hinge / half / facing / open) is
    // not yet placed; that landed deferred along with per-theme door
    // materials. A door's `mat_slot=` is the material that block would
    // read, which is why `MemberRole::unread_arguments` lists it as
    // unreached on a `door`: placing the block is the day that entry goes.
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
            // INVARIANT(wall-grid-validated): `at` is below `len` by the
            // match above, and every row the loop asks for is at most
            // `course_top` because `door_height` is clamped to the course
            // holding `base_row` — so the helper has nothing to reject.
            // Reaching here means one of those two stopped agreeing with
            // `wall_local_to_grid`'s `u < length` / `v < dims.y`. Loud in
            // debug builds; release builds skip the cell rather than panic
            // over one voxel.
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
