//! Cutting the rectangles of a `window` member.

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::geometry::{Declared, outer_face};
use crate::block_array::openings::{WallSide, wall_local_to_grid};
use crate::block_array::{Bbox, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, side_name};
use super::opening::{WindowCut, WindowPlan, plan_window, read_sym};

pub(super) fn fill_window(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the window, so it is reported in the same compile.
    let (sym, sym_unread) = read_sym(member);
    let plan = plan_window(member, sym, y_offset, ctx, diagnostics);
    let note = match plan {
        WindowPlan::Cut(cut) => {
            cut_window(cut, ctx, palette, canvas);
            "the window is drawn without its mirror, as `sym=false` would draw it"
        }
        WindowPlan::Refused => {
            "this window is not cut either way — see the finding on the same line"
        }
        WindowPlan::RefusedForUnreadSym => {
            "this window has `repeat=` greater than 1, which `sym=true` does not yet support, so \
             it is not cut on the `sym=false` default while `sym=` is unreadable — see the \
             finding on the same line"
        }
    };
    diagnostics.extend(sym_unread.map(|unread| unread.report(note)));
}

/// Paint each rectangle `cut` names with its fill.
fn cut_window(
    cut: WindowCut,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
) {
    if let Some(declared) = window_declared(ctx, &cut) {
        canvas.declare(declared);
    }
    let palette_index = cut
        .fill
        .map_or(PaletteIndex::AIR, |state| palette.intern(state));
    for (offset, width) in cut.stamps {
        paint_window_rect(
            ctx,
            WindowRect {
                side: cut.side,
                offset,
                y_start: cut.row,
                width,
                height: cut.height,
                palette_index,
            },
            canvas,
        );
    }
}

/// What a window's line declares: the rectangles `cut` cuts and its mirror's, anchored at the cell
/// its `offset=` and `y=` place.
fn window_declared(ctx: &StructCtx<'_>, cut: &WindowCut) -> Option<Declared> {
    let grid = |u: u32, y: u32| {
        wall_local_to_grid(
            cut.side,
            u,
            y,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        )
    };
    let top = cut.row.saturating_add(cut.height.saturating_sub(1));
    let corners = cut
        .stamps
        .iter()
        .chain(&cut.mirror)
        .flat_map(|&(offset, width)| {
            let far = offset.saturating_add(width.saturating_sub(1));
            [grid(offset, cut.row), grid(far, top)]
        })
        .collect::<Option<Vec<_>>>();
    // INVARIANT(wall-grid-validated): `plan_window` refused off-wall spans and rows.
    debug_assert!(corners.is_some(), "a window corner fell off the wall grid");
    let (anchor_u, anchor_y) = cut.anchor;
    Some(Declared {
        anchor: grid(anchor_u, anchor_y)?,
        bbox: Bbox::around(corners?)?,
        host_face: Some(outer_face(cut.side)),
    })
}

#[derive(Debug, Clone, Copy)]
struct WindowRect {
    side: WallSide,
    offset: u32,
    y_start: u32,
    width: u32,
    height: u32,
    palette_index: PaletteIndex,
}

fn paint_window_rect(ctx: &StructCtx<'_>, rect: WindowRect, canvas: &mut MemberCanvas<'_>) {
    for du in 0..rect.width {
        for dv in 0..rect.height {
            let along = rect.offset.saturating_add(du);
            let row = rect.y_start.saturating_add(dv);
            let Some(cell) = wall_local_to_grid(
                rect.side,
                along,
                row,
                ctx.overhang,
                ctx.interior_w,
                ctx.interior_h,
                ctx.dims,
            ) else {
                // INVARIANT(wall-grid-validated): `plan_window` refused off-wall spans and rows.
                debug_assert!(
                    false,
                    "window cell u={along} y={row} on the {} wall is outside the wall grid; \
                     `plan_window`'s span check (`offset + step*(repeat-1) + size.w <= wall \
                     length`, which also bounds the mirror) / wall-course check stopped \
                     agreeing with `wall_local_to_grid`",
                    side_name(rect.side),
                );
                continue;
            };
            canvas.paint(cell, || rect.palette_index);
        }
    }
}
