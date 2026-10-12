//! Cutting the rectangles of a `window` member.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::PaletteIndex;
use crate::block_array::args::{
    NonNegRead, diag_deferred_member_reason, nonneg_int_or_defer, read_or_ignore,
};
use crate::block_array::openings::{
    WallSide, WindowArgs, read_window_args, wall_length, wall_local_to_grid,
};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, palette_index_for, side_name, side_of};

pub(super) fn fill_window(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the window, so it is reported in the same compile.
    let (sym, sym_unread) = match read_or_ignore(
        member,
        "sym",
        |kind| match kind {
            ValueKind::Bool(b) => Some(*b),
            _ => None,
        },
        "`true` or `false`",
    ) {
        Ok(sym) => (Some(sym.unwrap_or(false)), None),
        Err(unread) => (None, Some(unread)),
    };
    let cut = cut_window(member, sym, y_offset, ctx, palette, canvas, diagnostics);
    diagnostics.extend(sym_unread.map(|unread| {
        unread.report(match cut {
            WindowCut::Cut => {
                "the window is drawn without its mirror, as `sym=false` would draw it"
            }
            WindowCut::Refused => {
                "this window is not cut either way — see the finding on the same line"
            }
            WindowCut::RefusedForUnreadSym => {
                "this window has `repeat=` greater than 1, which `sym=true` does not yet \
                 support, so it is not cut on the `sym=false` default while `sym=` is \
                 unreadable — see the finding on the same line"
            }
        })
    }));
}

/// What [`cut_window`] did with the window, which picks the note on an unreadable `sym=`.
#[derive(Clone, Copy)]
enum WindowCut {
    /// The primary rectangle was cut.
    Cut,
    /// The window was refused, and another finding says why.
    Refused,
    /// Refused because `repeat=` is above 1 and an unreadable `sym=` may have meant `true`.
    RefusedForUnreadSym,
}

/// [`fill_window`]'s refusals and paint, with `sym=` read; `None` when it was unreadable.
fn cut_window(
    member: &Member,
    sym: Option<bool>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> WindowCut {
    let Some(side) = side_of(member, diagnostics) else {
        return WindowCut::Refused;
    };
    let WindowArgs {
        offset,
        y: y_start_local,
        width: sw,
        height: sh,
    } = match read_window_args(member) {
        Ok(args) => args,
        Err(fault) => {
            diagnostics.push(diag_deferred_member_reason(member, &fault.deferral()));
            return WindowCut::Refused;
        }
    };
    let y_start = y_start_local.saturating_add(y_offset);
    let (repeat, step) = match window_series(member, sym, diagnostics) {
        Ok(series) => series,
        Err(refused) => return refused,
    };
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    let refusal = window_span_refusal(side, offset, (sw, sh), (repeat, step), len)
        .or_else(|| window_course_refusal(y_start, (sw, sh), ctx));
    if let Some(reason) = refusal {
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return WindowCut::Refused;
    }
    // Resolved after the geometry checks, so a refused window claims no palette entry.
    // With no `mat_slot=`, a window is an opening carved to air.
    let idx = if member.mat_slot.is_some() {
        let Some(idx) = palette_index_for(
            member,
            ctx.scope,
            ctx.registry,
            palette,
            diagnostics,
            ctx.theme_missing,
        ) else {
            return WindowCut::Refused;
        };
        idx
    } else {
        PaletteIndex::AIR
    };
    let base_rect = WindowRect {
        side,
        offset,
        y_start,
        width: sw,
        height: sh,
        palette_index: idx,
    };
    for i in 0..repeat {
        let stamped_offset = offset.saturating_add(step.saturating_mul(i));
        paint_window_rect(
            ctx,
            WindowRect {
                offset: stamped_offset,
                ..base_rect
            },
            canvas,
        );
    }
    if sym == Some(true) {
        cut_window_mirror(member, base_rect, len, ctx, canvas, diagnostics);
    }
    WindowCut::Cut
}

/// A window's `repeat=` and `step=`, or the refusal after the finding that says why.
fn window_series(
    member: &Member,
    sym: Option<bool>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(u32, u32), WindowCut> {
    // `repeat=0` defers: an author who wants no window drops the line.
    let repeat = match nonneg_int_or_defer(member, "repeat", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "window `repeat=0` would stamp no instances; drop the window instead",
            ));
            return Err(WindowCut::Refused);
        }
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 1,
        NonNegRead::Deferred => return Err(WindowCut::Refused),
    };
    let step = match nonneg_int_or_defer(member, "step", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return Err(WindowCut::Refused),
    };
    // Before `sym=`, so the author hears about `step=` in this compile.
    if repeat > 1 && step == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window `repeat=` requires a positive `step=` so instances do not overlap",
        ));
        return Err(WindowCut::Refused);
    }
    if repeat > 1 {
        match sym {
            Some(false) => {}
            Some(true) => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "window with both `repeat=` and `sym=true` is not yet supported",
                ));
                return Err(WindowCut::Refused);
            }
            None => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "window with `repeat=` greater than 1 is not cut while its `sym=` is \
                     unreadable: `sym=true` with such a `repeat=` is not yet supported, and \
                     `sym=false` is not what was written",
                ));
                return Err(WindowCut::RefusedForUnreadSym);
            }
        }
    }
    Ok((repeat, step))
}

/// Why a window series does not fit along its `len`-long wall, or `None` when it does.
fn window_span_refusal(
    side: WallSide,
    offset: u32,
    (sw, sh): (u32, u32),
    (repeat, step): (u32, u32),
    len: u32,
) -> Option<String> {
    let span_end = offset
        .saturating_add(step.saturating_mul(repeat.saturating_sub(1)))
        .saturating_add(sw);
    (span_end > len).then(|| {
        format!(
            "window extends beyond the `{}` wall (offset={offset} size={sw}x{sh} repeat={repeat} step={step}, wall length={len})",
            side_name(side),
        )
    })
}

/// Why a window `sh` tall from row `y_start` does not sit inside one wall course, or `None`.
fn window_course_refusal(
    y_start: u32,
    (sw, sh): (u32, u32),
    ctx: &StructCtx<'_>,
) -> Option<String> {
    // Every row must be one a `walls` member painted, not merely one below the roof.
    if ctx.wall_column.contains_rows(y_start, sh) {
        return None;
    }
    Some(if ctx.wall_column.is_empty() {
        // An unresolved `mat_slot=` empties the column as surely as a missing `height=`.
        format!(
            "window at y={y_start} size={sw}x{sh} has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)",
        )
    } else if let Some(last) = y_start.checked_add(sh.saturating_sub(1)) {
        format!(
            "window rows y={y_start}..={last} are not all inside one wall course (size={sw}x{sh}; the walls occupy {})",
            ctx.wall_column,
        )
    } else {
        // Saturating would print a range that ends before it starts.
        format!(
            "window rows from y={y_start} run past the highest row a build can address (size={sw}x{sh}; the walls occupy {})",
            ctx.wall_column,
        )
    })
}

/// Cut a `sym=true` window's mirror across the middle of its wall, or report why it is skipped.
fn cut_window_mirror(
    member: &Member,
    rect: WindowRect,
    len: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let WindowRect {
        side,
        offset,
        width: sw,
        height: sh,
        ..
    } = rect;
    let mirror_offset = len.saturating_sub(offset).saturating_sub(sw);
    // A pair is asked for, so a mirror meeting the primary is skipped and reported.
    let primary_end = offset.saturating_add(sw);
    let mirror_end = mirror_offset.saturating_add(sw);
    let overlap = offset < mirror_end && mirror_offset < primary_end;
    if overlap {
        let relation = if mirror_offset == offset {
            "coincides with"
        } else {
            "would overlap"
        };
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "`sym=true` window at offset={offset} size={sw}x{sh} on the `{}` wall {relation} its mirror (wall length={len}); the mirror was skipped",
                side_name(side),
            ),
        ));
        return;
    }
    paint_window_rect(
        ctx,
        WindowRect {
            offset: mirror_offset,
            ..rect
        },
        canvas,
    );
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
                // INVARIANT(wall-grid-validated): `fill_window` refused off-wall spans and rows.
                debug_assert!(
                    false,
                    "window cell u={along} y={row} on the {} wall is outside the wall grid; \
                     `fill_window`'s span check (`offset + step*(repeat-1) + size.w <= wall \
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
