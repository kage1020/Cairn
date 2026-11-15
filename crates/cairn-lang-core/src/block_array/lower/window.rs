//! Cutting the rectangles of a `window` member.

use crate::diagnostic::Diagnostic;
use crate::intent::{BindingKey, Member};

use crate::block_array::PaletteIndex;
use crate::block_array::openings::{WallSide, wall_local_to_grid};

use super::canvas::{MemberCanvas, ScopePalette};
use super::frame::{Cut, Frame};
use super::members::{StructCtx, bound_block, side_name};
use super::opening::{WindowCut, WindowPlan, plan_window, read_sym};

/// Cut a `window`'s rectangles, with the frame a theme binds on it round the ones it cut.
pub(super) fn fill_window(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Frame> {
    let frame = bound_block(member, BindingKey::Frame, ctx, diagnostics);
    // Read before anything can refuse the window, so it is reported in the same compile.
    let (sym, sym_unread) = read_sym(member);
    let plan = plan_window(member, sym, y_offset, ctx, diagnostics);
    let (cuts, note) = match plan {
        WindowPlan::Cut(cut) => (
            Some(cut_window(cut, ctx, palette, canvas)),
            "the window is drawn without its mirror, as `sym=false` would draw it",
        ),
        WindowPlan::Refused => (
            None,
            "this window is not cut either way — see the finding on the same line",
        ),
        WindowPlan::RefusedForUnreadSym => (
            None,
            "this window has `repeat=` greater than 1, which `sym=true` does not yet support, so \
             it is not cut on the `sym=false` default while `sym=` is unreadable — see the \
             finding on the same line",
        ),
    };
    diagnostics.extend(sym_unread.map(|unread| unread.report(note)));
    frame
        .zip(cuts)
        .map(|(state, cuts)| Frame::new(canvas.member(), state, cuts))
}

/// Paint each rectangle `cut` names with its fill, returning the rectangles.
fn cut_window(
    cut: WindowCut,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
) -> Vec<Cut> {
    let palette_index = cut
        .fill
        .map_or(PaletteIndex::AIR, |state| palette.intern(state));
    cut.stamps
        .into_iter()
        .map(|(offset, width)| {
            let rect = WindowRect {
                side: cut.side,
                offset,
                y_start: cut.row,
                width,
                height: cut.height,
                palette_index,
            };
            paint_window_rect(ctx, rect, canvas);
            rect.cut()
        })
        .collect()
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

impl WindowRect {
    fn cut(self) -> Cut {
        Cut {
            side: self.side,
            offset: self.offset,
            y_start: self.y_start,
            width: self.width,
            height: self.height,
        }
    }
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
