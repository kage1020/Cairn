//! Cutting the rectangles of a `window` member.

use crate::ast::ValueKind;
use crate::check::Diagnostic;
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
    // unmirrored, except on a window with `repeat=` greater than 1, which
    // `sym=true` refuses: that one is refused. Read before anything can
    // refuse the window, so it is reported in the same compile as a
    // refusal, with a note that says which of the three [`WindowCut`]
    // outcomes happened.
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
    // `offset=` defaults to 0 (the wall-local axis origin) when absent, so a
    // decorative repeat=N series can be authored as `window ... repeat=N
    // step=M size=WxH` without a redundant `offset=0`; `y=` and `size=` are
    // required. A key that is present but ill-shaped defers, with a reason
    // that says so rather than calling it missing. Read by
    // `block_array::openings::read_window_args`, which a `connect` port on this
    // window reads the same three with.
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
    // Resolved below the two geometry checks above, not before them: both
    // return without painting, and a palette entry claimed on the way to
    // one of them is a block `cairn info` counts and the `.nbt` ships for a
    // window that was never cut.
    //
    // A window without a `mat_slot=` is an *opening* rather than a fill:
    // the rectangle is carved to air, giving the `class=arrow_slit`
    // pattern themed-tower uses a way to punch narrow slits through a
    // stone wall without also picking a decorative species. Windows with
    // an explicit `mat_slot=` continue to resolve through the palette;
    // resolution failure still short-circuits so the resolver's own
    // diagnostic isn't shadowed here.
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
    // `repeat=` stamps the same rectangle multiple times along the wall,
    // separated by `step=` voxels. Both keys are optional: an absent
    // `repeat` collapses to a single instance (the pre-repeat
    // behaviour). Present-but-invalid keys defer via
    // `nonneg_int_or_defer` so a typo like `repeat=abc` earns a
    // diagnostic instead of silently rounding to 1. `repeat=0` also
    // defers because "stamp zero times" is almost always a bug (an
    // author who wants no window just deletes the line). `step=0
    // repeat>=2` would stamp on top of itself, which is caught below.
    // The `shape=` key (used by `class=arrow_slit` as `shape=slit`) is
    // read only for source-level acceptance — the block-array pass
    // doesn't alter the palette based on it yet, so it neither defers
    // nor changes the voxel output. The slit look-and-feel is a
    // follow-up.
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
    // whatever `sym=` says, so an unreadable `sym=` is not why it is not
    // cut, and the author hears about `step=` now rather than after
    // repairing `sym=`.
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
    // A window is a rectangle cut into a wall, so every row it cuts has
    // to be a row some `walls` member painted — not merely a row below
    // the roof. The three ways to leave the masonry all cut silently
    // before this asked the whole question: below the first course the
    // rectangle carves through the floor slab, above the top course it
    // punches through the roof voxels the envelope phase wrote at
    // `y = wall_top + 1`, and between two `level` courses it hangs its
    // glass in open air.
    if ctx.wall_column.contains_rows(y_start, sh) {
        return None;
    }
    Some(if ctx.wall_column.is_empty() {
        // The column holds the rows the walls will *paint*, so an
        // empty one means "no walls member puts a block anywhere" —
        // a `mat_slot=` that does not resolve empties it as surely
        // as a missing `height=` does, and naming only the height
        // sends an author who wrote one to the wrong line. Same
        // reason `carve_door`'s gate says the same thing.
        format!(
            "window at y={y_start} size={sw}x{sh} has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)",
        )
    } else if let Some(last) = y_start.checked_add(sh.saturating_sub(1)) {
        format!(
            "window rows y={y_start}..={last} are not all inside one wall course (size={sw}x{sh}; the walls occupy {})",
            ctx.wall_column,
        )
    } else {
        // Saturating here printed `y=4294967295..=4294967294`, a range
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
    // Reject overlapping mirrors: a `sym=true` window asks for a
    // *pair*, not one wide span. The `span_end` guard in
    // `window_span_refusal` refused `offset + size_w > wall_length`, so
    // neither subtraction saturates, and the two rectangles intersect
    // exactly when the window straddles the wall's midpoint
    // (2*offset < wall_length < 2*(offset + size_w)) — diagnose and
    // skip the mirror so the primary is still emitted cleanly. A
    // centred window (2*offset + size_w == wall_length) has a mirror
    // that is the same rectangle, the fullest overlap there is: the
    // author still asked for two windows and got one, so it is
    // reported like the rest.
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
                // span past the wall (`span_end = offset + step*(repeat-1) +
                // size.w` is at most the wall length, which also bounds the
                // `sym=true` mirror) and any row outside a wall course before
                // it got here — so the helper has nothing to reject. Reaching
                // here means one of those checks stopped agreeing with
                // `wall_local_to_grid`. Loud in debug builds; release builds
                // skip the cell rather than panic.
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
