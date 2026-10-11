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
    // An unreadable `sym=` (`sym=yes`, `sym="true"`) draws the window
    // unmirrored, unless its `repeat=` is above 1, which `sym=true`
    // refuses, and then the window is refused. Read before anything can
    // refuse the window, so it is reported in the same compile as a
    // refusal, with a note saying which `WindowCut` happened.
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

/// What [`cut_window`] did with the window, which is what the note on an
/// unreadable `sym=` has to say.
#[derive(Clone, Copy)]
enum WindowCut {
    /// The primary rectangle was cut.
    Cut,
    /// The window was refused, and another finding says why. When its
    /// `sym=` is unreadable, that reason holds whatever `sym=` had said.
    Refused,
    /// The window has `repeat=` greater than 1, which `sym=true` refuses,
    /// and its `sym=` is unreadable: it was refused instead of carried on
    /// with the `sym=false` default, which the source may not mean.
    RefusedForUnreadSym,
}

/// [`fill_window`]'s refusals and paint, with `sym=` already read: `None`
/// when the value written is unreadable, in which case the window is
/// painted unmirrored, or refused if its `repeat=` is greater than 1, which
/// `sym=true` refuses.
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
    // `offset=` defaults to `0`, so a `repeat=N step=M` series needs no
    // `offset=0`; `y=` and `size=` are required, and a key present but
    // ill-shaped defers as such, not as missing. A `connect` port on this
    // window reads all three through `read_window_args` too.
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
    // Resolved after the geometry checks, which return without painting,
    // so a refused window claims no palette entry.
    //
    // A window with no `mat_slot=` is an *opening*, carved to air, as
    // `themed-tower.crn`'s `class=arrow_slit` slits are. One with a
    // `mat_slot=` resolves through the palette, and a failure, already
    // reported, refuses the window.
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

/// A window's `repeat=` and `step=`: how many copies of its rectangle it
/// stamps, and how far apart along the wall. `Err` carries the refusal,
/// after the finding that says why.
fn window_series(
    member: &Member,
    sym: Option<bool>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(u32, u32), WindowCut> {
    // An invalid `repeat=` or `step=` defers rather than rounding to one
    // window. `repeat=0` defers too: stamping nothing is almost always a
    // typo, since an author who wants no window drops the line. A `shape=`
    // key is accepted and changes no voxel.
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
    // Before `sym=`: a series without a positive `step=` is refused
    // whatever `sym=` says, so the author hears about `step=` in this
    // compile rather than after repairing `sym=`.
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
            // `sym=true` refuses a window with `repeat=` greater than 1, so
            // carrying on with the `sym=false` default could build a window
            // the source refused.
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

/// Why a window series of `repeat` rectangles `sw` wide, `step` apart
/// from `offset`, does not fit along its `len`-long `side` wall, or `None`
/// when it does.
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

/// Why a window `sh` tall from row `y_start` does not sit inside one
/// wall course, or `None` when it does.
fn window_course_refusal(
    y_start: u32,
    (sw, sh): (u32, u32),
    ctx: &StructCtx<'_>,
) -> Option<String> {
    // Every row a window cuts must be one a `walls` member painted, not
    // merely one below the roof: below the first course it would carve
    // the floor slab, above the top course the roof at `wall_top + 1`,
    // and between two `level` courses it would hang in open air.
    if ctx.wall_column.contains_rows(y_start, sh) {
        return None;
    }
    Some(if ctx.wall_column.is_empty() {
        // An empty column means no `walls` paints: an unresolved
        // `mat_slot=` empties it as surely as a missing `height=`, so
        // naming only the height would send an author who wrote one to
        // the wrong line. `carve_door` words it the same way.
        format!(
            "window at y={y_start} size={sw}x{sh} has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)",
        )
    } else if let Some(last) = y_start.checked_add(sh.saturating_sub(1)) {
        format!(
            "window rows y={y_start}..={last} are not all inside one wall course (size={sw}x{sh}; the walls occupy {})",
            ctx.wall_column,
        )
    } else {
        // Saturating would print `y=4294967295..=4294967294`, a range
        // that ends before it starts.
        format!(
            "window rows from y={y_start} run past the highest row a build can address (size={sw}x{sh}; the walls occupy {})",
            ctx.wall_column,
        )
    })
}

/// Cut a `sym=true` window's mirror, the copy of `rect` reflected across
/// the middle of its `len`-long wall, or report why it is skipped.
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
    // A `sym=true` window asks for a *pair*, not one wide span.
    // `window_span_refusal` refused `offset + size_w > wall_length`, so
    // neither subtraction saturates, and the two intersect exactly when
    // the window straddles the midpoint
    // (2*offset < wall_length < 2*(offset + size_w)); the mirror is then
    // skipped and the primary kept. A centred window's mirror is the same
    // rectangle: two windows asked for, one got, so it is reported too.
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
                // INVARIANT(wall-grid-validated): `fill_window` refused any
                // span past the wall (`offset + step*(repeat-1) + size.w`,
                // which also bounds the mirror) and any row outside a wall
                // course. Release builds skip the cell rather than panic.
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
