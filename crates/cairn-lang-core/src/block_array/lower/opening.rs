//! Where a `door` or `window` line places its opening, judged the same way for the member that
//! cuts it and for a `beam` that rests on it.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::intent::{Member, MemberRole};

use crate::block_array::BlockState;
use crate::block_array::args::{
    NonNegRead, UnreadArgument, diag_deferred_member_noted, diag_deferred_member_reason,
    diag_invalid_member, ident_or_ignore, nonneg_int_or_refuse, read_or_ignore,
};
use crate::block_array::openings::{
    DOOR_HEIGHT, WallSide, WindowArgs, door_base_row, door_column, read_window_args, wall_length,
};

use super::fixture::Refusal;
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{
    MemberState, StructCtx, UNPAINTED_COURSE, default_block_id, resolve_member_state, side_name,
    side_of, unplaced_walls_note,
};
use super::stand::word;

/// The outlines a window's `shape=` takes.
const SHAPES: [&str; 2] = ["rect", "arch"];

/// Catalog token a `door`'s default material comes from; the pack spells it per edition.
pub(super) const DOOR_TOKEN: &str = "door.default";

/// Door id used when the pack cannot supply one: the Java spelling, Java being the base edition
/// (`spec/versioning-editions` "Backend = data tables").
pub(super) const DOOR_BASE_ID: &str = "minecraft:oak_door";

/// The side of a door its hinge is on, as seen from outside the wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hinge {
    Left,
    Right,
}

impl Hinge {
    /// Every hinge, in the order a note lists them.
    const ALL: [Self; 2] = [Self::Left, Self::Right];

    /// The hinge a `hinge=` identifier names, or `None`.
    fn from_ident(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hinge| hinge.as_str() == name)
    }

    /// The value of the block's `hinge` state.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// The `hinge=` and `open=` a door's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
pub(super) struct DoorStates<'m> {
    pub(super) hinge: Option<&'m str>,
    pub(super) open: bool,
}

/// The states a door's line writes, with each value it cannot read and what the door is built as
/// without it.
pub(super) fn read_door_states(
    member: &Member,
) -> (DoorStates<'_>, Vec<(UnreadArgument, &'static str)>) {
    let mut unread = Vec::new();
    let hinge = ident_or_ignore(member, "hinge", "`left` or `right`").unwrap_or_else(|argument| {
        unread.push((
            argument,
            "the door is built with the hinge the wall beside it gives",
        ));
        None
    });
    let open = read_or_ignore(
        member,
        "open",
        |kind| match kind {
            ValueKind::Bool(open) => Some(*open),
            _ => None,
        },
        "`true` or `false`",
    )
    .unwrap_or_else(|argument| {
        unread.push((
            argument,
            "the door is built closed, as `open=false` builds it",
        ));
        None
    });
    let states = DoorStates {
        hinge,
        open: open.unwrap_or(false),
    };
    (states, unread)
}

/// Where a door's line stands its door block.
#[derive(Debug, Clone)]
pub(super) struct DoorPlan {
    pub(super) side: WallSide,
    /// Its column along the wall.
    pub(super) at: u32,
    /// The row it opens at, from the struct's base.
    pub(super) base_row: u32,
    /// `hinge=` as the line writes it, or `None` when it is derived.
    pub(super) hinge: Option<Hinge>,
    pub(super) id: String,
}

/// Where the door `member` under `level y=y_offset` stands, or `None` after the findings that
/// refuse it.
///
/// Every argument and the material are judged whatever else refuses the door, so one build reports
/// each fault of its own; the course is judged only for a line with none.
pub(super) fn plan_door(
    member: &Member,
    written: DoorStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DoorPlan> {
    let side = side_of(member, diagnostics);
    let at = side.and_then(|side| {
        let length = wall_length(side, ctx.interior_w, ctx.interior_h);
        door_column(member, side, length)
            .map_err(|fault| {
                diagnostics.push(diag_invalid_member(
                    member,
                    fault.refusal(),
                    vec![fault.fix(length)],
                ));
            })
            .ok()
    });
    let hinge = written.hinge.map(|name| {
        Hinge::from_ident(name).ok_or_else(|| {
            let hinges: Vec<&str> = Hinge::ALL.map(Hinge::as_str).into();
            diagnostics.push(diag_invalid_member(
                member,
                format!("unknown door `hinge={name}`"),
                vec![format!("`hinge=` takes one of {}", hinges.join(", "))],
            ));
        })
    });
    let line = match (side, at, hinge.transpose()) {
        (Some(side), Some(at), Ok(hinge)) => Some((side, at, hinge)),
        _ => None,
    };
    // Judged against the course holding the door's rows, never the tallest wall row.
    let base_row = door_base_row(y_offset);
    let in_course = line.is_some()
        && base_row.is_some_and(|row| ctx.wall_column.contains_rows(row, DOOR_HEIGHT));
    if line.is_some() && !in_course {
        diagnostics.push(door_course_refusal(member, base_row, y_offset, ctx));
    }
    let id = door_block_id(member, ctx, diagnostics)
        .map_err(|refusal| diagnostics.push(*refusal))
        .ok()
        .flatten();
    let (Some((side, at, hinge)), true, Some(id), Some(base_row)) = (line, in_course, id, base_row)
    else {
        return None;
    };
    Some(DoorPlan {
        side,
        at,
        base_row,
        hinge,
        id,
    })
}

/// The block a door is built from: its `mat_slot=`, or the pack's default door; `None` for a
/// binding whose state literal the target refused, which builds no door.
///
/// # Errors
///
/// The `E_UNKNOWN_ID` for a default door the target does not declare.
fn door_block_id(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<String>, Box<Diagnostic>> {
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
        || default_block_id(member, ctx.registry, DOOR_TOKEN, DOOR_BASE_ID),
        &GeometryMemberDescription {
            subject: "door".to_owned(),
            states_from: "where it stands",
            requires: Some(Family::Door),
        },
        diagnostics,
    )
    .map(Some)
}

/// The finding that refuses a door under `level y=y_offset` at `base_row`, whose two rows no
/// wall course holds, or which opens past the highest row a build can address.
///
/// A row a `walls` member is written to hold and does not paint is that member's to repair.
fn door_course_refusal(
    member: &Member,
    base_row: Option<u32>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let written = &ctx.written;
    let Some(base_row) = base_row else {
        return diag_invalid_member(
            member,
            format!(
                "door under `level y={y_offset}` opens past the highest row a build can address \
                 (the walls occupy {})",
                written.walls,
            ),
            vec![
                "a door opens one row above the base of its `level`; lower the `level` with \
                  `y=`"
                    .to_owned(),
            ],
        );
    };
    if written.walls.contains_rows(base_row, DOOR_HEIGHT) {
        let primary = if ctx.wall_column.is_empty() {
            "door has no wall to cut into: no `walls` member on this struct paints".to_owned()
        } else {
            format!(
                "door stands at y={base_row}..={}, in a course the walls are written to ({}) but not in one they paint ({})",
                base_row.saturating_add(1),
                written.walls,
                ctx.wall_column,
            )
        };
        return diag_deferred_member_noted(member, &primary, UNPAINTED_COURSE);
    }
    if written.unplaced_walls {
        let primary = if written.walls.is_empty() {
            "door has no wall to cut into: every `walls` member on this struct sits under a \
             `level` that is not placed"
                .to_owned()
        } else {
            format!(
                "door stands at y={base_row}..={}, which no placed wall course holds (the placed walls occupy {})",
                base_row.saturating_add(1),
                written.walls,
            )
        };
        return diag_deferred_member_noted(
            member,
            &primary,
            &unplaced_walls_note("the rows the door stands in", "the door is set"),
        );
    }
    let (primary, fix) = if written.walls.is_empty() {
        (
            "door has no wall to cut into: this struct declares no `walls` with a positive `height=`"
                .to_owned(),
            if written.unsized_walls {
                "give the `walls` a positive `height=`, or add a `walls` member whose course holds \
                 the two rows the door stands in"
            } else {
                "add a `walls` member whose course holds the two rows the door stands in"
            },
        )
    } else if written.walls.course_top_at(base_row).is_some() {
        (
            format!(
                "door stands at y={base_row}..={}, and the wall course holding y={base_row} ends \
                 there (the walls occupy {})",
                base_row.saturating_add(1),
                written.walls,
            ),
            "a door block is two rows tall; raise the `walls` so their course holds the row above \
             the one the door opens at",
        )
    } else {
        (
            format!(
                "door opens at y={base_row}, which is not inside any wall course (the walls occupy {})",
                written.walls,
            ),
            "a door opens one row above the base of its `level`, or of the body outside one; \
             move it under a `level` whose walls hold that row and the one above, or add walls \
             there",
        )
    };
    diag_invalid_member(member, primary, vec![fix.to_owned()])
}

/// A window's `sym=`, `false` when its line writes none and `None` when it cannot be read, with
/// the value it could not read.
pub(super) fn read_sym(member: &Member) -> (Option<bool>, Option<UnreadArgument>) {
    match read_or_ignore(
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
    }
}

/// A window's `shape=` as an identifier, `None` when its line writes none, with the value it could
/// not read.
pub(super) fn read_shape(member: &Member) -> (Option<&str>, Option<UnreadArgument>) {
    match ident_or_ignore(member, "shape", "`rect` or `arch`") {
        Ok(shape) => (shape, None),
        Err(unread) => (None, Some(unread)),
    }
}

/// Whether a window's `shape=` rounds the top of what it cuts, or the refusal of a shape it does
/// not take or an arch too small to round (`spec/syntax` "Selectors").
fn window_arch(member: &Member, shape: Option<&str>, args: WindowArgs) -> Result<bool, Refusal> {
    if word(member, "shape", shape, &SHAPES)? != Some("arch") {
        return Ok(false);
    }
    if args.width >= 3 && args.height >= 2 {
        return Ok(true);
    }
    Err(Box::new(diag_invalid_member(
        member,
        format!(
            "window `shape=arch` needs a `size=` at least 3 wide and 2 high, not {}x{}",
            args.width, args.height,
        ),
        vec![
            "an arch leaves the two ends of its top row to the wall, so it needs a cell between \
             them and a row under them; widen or raise `size=`, or drop `shape=arch`"
                .to_owned(),
        ],
    )))
}

/// What a window's line cuts.
#[derive(Debug, Clone)]
pub(super) enum WindowPlan {
    /// The rectangles it cuts.
    Cut(WindowCut),
    /// Refused, and another finding says why.
    Refused,
    /// Refused because `repeat=` is above 1 and an unreadable `sym=` may have meant `true`.
    RefusedForUnreadSym,
}

/// The rectangles a window's line cuts, all on one wall and the same rows.
#[derive(Debug, Clone)]
pub(super) struct WindowCut {
    pub(super) side: WallSide,
    /// Its lowest row, from the struct's base.
    pub(super) row: u32,
    pub(super) height: u32,
    /// The `(offset, width)` of each rectangle: its `repeat=` series, then its `sym=true` mirror.
    pub(super) stamps: Vec<(u32, u32)>,
    /// The `(offset, width)` its `sym=true` mirror takes, declared even when the mirror is skipped.
    pub(super) mirror: Option<(u32, u32)>,
    /// The wall-local `(u, y)` of the cell its `offset=` and `y=` place.
    pub(super) anchor: (u32, u32),
    /// The block its `mat_slot=` binds, or `None` for an opening carved to air.
    pub(super) fill: Option<BlockState>,
    /// Whether `shape=arch` leaves the two ends of each rectangle's top row to the wall.
    pub(super) arch: bool,
}

/// What the window `member` under `level y=y_offset` cuts, with `sym=` read and `None` when it was
/// unreadable, and `shape=` as [`read_shape`] reads it.
pub(super) fn plan_window(
    member: &Member,
    sym: Option<bool>,
    shape: Option<&str>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> WindowPlan {
    let Some(side) = side_of(member, diagnostics) else {
        return WindowPlan::Refused;
    };
    let args = match read_window_args(member) {
        Ok(args) => args,
        Err(fault) => {
            diagnostics.push(diag_invalid_member(
                member,
                fault.refusal(),
                vec![fault.fix()],
            ));
            return WindowPlan::Refused;
        }
    };
    let (offset, sw, sh) = (args.column, args.width, args.height);
    let arch = match window_arch(member, shape, args) {
        Ok(arch) => arch,
        Err(refusal) => {
            diagnostics.push(*refusal);
            return WindowPlan::Refused;
        }
    };
    let y_start = args.row_under(y_offset);
    let Some((repeat, step)) = window_series(member, diagnostics) else {
        return WindowPlan::Refused;
    };
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    let refusal = window_span_refusal(side, args, (repeat, step), len)
        .map(|(reason, fix)| diag_invalid_member(member, reason, vec![fix]))
        .or_else(|| window_course_refusal(member, args, (y_start, y_offset), ctx));
    // The window's own line is asked before what the pass does not lower yet.
    let own_fault = refusal
        .as_ref()
        .is_some_and(|refusal| refusal.code == DiagnosticCode::InvalidMember);
    if !own_fault && let Some((deferral, plan)) = series_mirror_deferral(member, repeat, sym) {
        diagnostics.push(deferral);
        return plan;
    }
    if let Some(refusal) = refusal {
        diagnostics.push(refusal);
        return WindowPlan::Refused;
    }
    let Some(y_start) = y_start else {
        // INVARIANT: `window_course_refusal` refuses a window whose rows start past `u32`.
        debug_assert!(
            false,
            "window rows past `u32` under `level y={y_offset}` were not refused"
        );
        return WindowPlan::Refused;
    };
    // Resolved after the geometry checks, so a refused window names no material.
    let fill = if member.mat_slot.is_some() {
        let resolved = resolve_member_state(
            member,
            ctx.scope,
            ctx.registry,
            diagnostics,
            ctx.theme_missing,
        );
        let Some(state) = resolved.bound() else {
            return WindowPlan::Refused;
        };
        Some(state)
    } else {
        None
    };
    let mut stamps: Vec<(u32, u32)> = (0..repeat)
        .map(|i| (offset.saturating_add(step.saturating_mul(i)), sw))
        .collect();
    let mirror = (sym == Some(true)).then(|| (mirror_column(args, len), sw));
    if mirror.is_some() {
        stamps
            .extend(window_mirror(member, side, args, len, diagnostics).map(|mirror| (mirror, sw)));
    }
    WindowPlan::Cut(WindowCut {
        side,
        row: y_start,
        height: sh,
        stamps,
        mirror,
        anchor: (args.offset(), args.y().saturating_add(y_offset)),
        fill,
        arch,
    })
}

/// A window's `repeat=` and `step=`, or `None` after the finding that refuses the window.
fn window_series(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<(u32, u32)> {
    // `repeat=0` is refused: an author who wants no window drops the line.
    let repeat = match nonneg_int_or_refuse(member, "repeat", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_invalid_member(
                member,
                "window `repeat=0` would stamp no instances; drop the window instead".to_owned(),
                Vec::new(),
            ));
            return None;
        }
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 1,
        NonNegRead::Refused => return None,
    };
    let step = match nonneg_int_or_refuse(member, "step", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Refused => return None,
    };
    if repeat > 1 && step == 0 {
        diagnostics.push(diag_invalid_member(
            member,
            "window `repeat=` requires a positive `step=` so instances do not overlap".to_owned(),
            Vec::new(),
        ));
        return None;
    }
    Some((repeat, step))
}

/// The deferral of a series of `repeat` windows whose `sym=` is `true` or unreadable, which the
/// pass does not cut yet, with what [`plan_window`] answers.
fn series_mirror_deferral(
    member: &Member,
    repeat: u32,
    sym: Option<bool>,
) -> Option<(Diagnostic, WindowPlan)> {
    if repeat <= 1 {
        return None;
    }
    match sym {
        Some(false) => None,
        Some(true) => Some((
            diag_deferred_member_reason(
                member,
                "window with both `repeat=` and `sym=true` is not yet supported",
            ),
            WindowPlan::Refused,
        )),
        None => Some((
            diag_deferred_member_reason(
                member,
                "window with `repeat=` greater than 1 is not cut while its `sym=` is unreadable: \
                 `sym=true` with such a `repeat=` is not yet supported, and `sym=false` is not \
                 what was written",
            ),
            WindowPlan::RefusedForUnreadSym,
        )),
    }
}

/// Why a window series does not fit along its `len`-long wall and how to make it fit, or `None`
/// when it does.
fn window_span_refusal(
    side: WallSide,
    args: WindowArgs,
    (repeat, step): (u32, u32),
    len: u32,
) -> Option<(String, String)> {
    let (sw, sh) = (args.width, args.height);
    let span_end = args
        .column
        .saturating_add(step.saturating_mul(repeat.saturating_sub(1)))
        .saturating_add(sw);
    let first = args.column_term();
    (span_end > len).then(|| {
        let fix = if repeat > 1 {
            format!(
                "the last instance ends at `{first} + step * (repeat - 1) + size.w`, which must \
                 be at most {len}; lower `offset=`, `step=` or `repeat=`, or narrow `size=`"
            )
        } else {
            format!(
                "the window ends at `{first} + size.w`, which must be at most {len}; lower \
                 `offset=` or narrow `size=`"
            )
        };
        (
            format!(
                "window extends beyond the `{}` wall (offset={}{} size={sw}x{sh} repeat={repeat} step={step}, wall length={len})",
                side_name(side),
                args.offset(),
                args.anchor_written(),
            ),
            fix,
        )
    })
}

/// The finding that refuses a window, `args` under `level y=y_offset` with its lowest row at
/// `y_start`, that no one wall course holds, or `None`.
///
/// Rows a `walls` member is written to hold and does not paint are that member's to repair.
fn window_course_refusal(
    member: &Member,
    args: WindowArgs,
    (y_start, y_offset): (Option<u32>, u32),
    ctx: &StructCtx<'_>,
) -> Option<Diagnostic> {
    let (sw, sh) = (args.width, args.height);
    if y_start.is_some_and(|start| ctx.wall_column.contains_rows(start, sh)) {
        return None;
    }
    let at = format!("y={}{}", args.y(), args.anchor_written());
    let written = &ctx.written;
    let last = y_start.and_then(|start| start.checked_add(sh.saturating_sub(1)));
    let (Some(y_start), Some(last)) = (y_start, last) else {
        // Saturating would print a range that ends before it starts.
        let (from, lower) = match y_start {
            Some(start) => (format!("y={start}"), "the window"),
            None => (
                format!("y={} under `level y={y_offset}`", args.row),
                "the window or its `level`",
            ),
        };
        return Some(diag_invalid_member(
            member,
            format!(
                "window rows from {from} run past the highest row a build can address (size={sw}x{sh}; the walls occupy {})",
                written.walls,
            ),
            vec![format!(
                "lower {lower} with `y=`, or shorten it with `size=`"
            )],
        ));
    };
    let rows = format!("window rows y={y_start}..={last}");
    if written.walls.contains_rows(y_start, sh) {
        let primary = if ctx.wall_column.is_empty() {
            format!(
                "window at {at} size={sw}x{sh} has no wall to cut into: no `walls` member on this struct paints",
            )
        } else {
            format!(
                "{rows} lie in a course the walls are written to ({}) but not in one they paint ({})",
                written.walls, ctx.wall_column,
            )
        };
        return Some(diag_deferred_member_noted(
            member,
            &primary,
            UNPAINTED_COURSE,
        ));
    }
    if written.unplaced_walls {
        let primary = if written.walls.is_empty() {
            format!(
                "window at {at} size={sw}x{sh} has no wall to cut into: every `walls` member on this struct sits under a `level` that is not placed",
            )
        } else {
            format!(
                "{rows} are not all inside one placed wall course (size={sw}x{sh}; the placed walls occupy {})",
                written.walls,
            )
        };
        return Some(diag_deferred_member_noted(
            member,
            &primary,
            &unplaced_walls_note("the window's rows", "the window is cut"),
        ));
    }
    let (primary, fix) = if written.walls.is_empty() {
        (
            format!(
                "window at {at} size={sw}x{sh} has no wall to cut into: this struct declares no `walls` with a positive `height=`",
            ),
            if written.unsized_walls {
                "give the `walls` a positive `height=`, or add a `walls` member whose course holds \
                 the window's rows"
            } else {
                "add a `walls` member whose course holds the window's rows"
            },
        )
    } else {
        (
            format!(
                "{rows} are not all inside one wall course (size={sw}x{sh}; the walls occupy {})",
                written.walls,
            ),
            "move the window with `y=`, shorten it with `size=`, or raise the walls' `height=`, \
             so one course holds every row it cuts",
        )
    };
    Some(diag_invalid_member(member, primary, vec![fix.to_owned()]))
}

/// The offset of a `sym=true` window's mirror across the middle of its wall, or `None` after the
/// finding that skips it.
fn window_mirror(
    member: &Member,
    side: WallSide,
    args: WindowArgs,
    len: u32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    let (offset, sw, sh) = (args.column, args.width, args.height);
    let mirror_offset = mirror_column(args, len);
    let primary_end = offset.saturating_add(sw);
    let mirror_end = mirror_offset.saturating_add(sw);
    // Strict: a mirror that only touches the primary shares no cell with it, so it is still a
    // pair (`spec/syntax` "Selectors").
    let overlap = offset < mirror_end && mirror_offset < primary_end;
    if overlap {
        let relation = if mirror_offset == offset {
            "coincides with"
        } else {
            "would overlap"
        };
        let note = if len < 2 {
            "a wall 1 long holds no `sym=true` pair; drop `sym=true` to keep the one opening \
             that was cut"
                .to_owned()
        } else {
            // On an odd wall the middle column is in neither half, so the two bounds differ.
            let (left_half_end, right_half_start) = (len / 2, len.div_ceil(2));
            let first = args.column_term();
            format!(
                "a `sym=true` window and its mirror are a pair only when the window lies in one \
                 half of the wall (here `{first} + size.w <= {left_half_end}` or \
                 `{first} >= {right_half_start}`); move or narrow the window to cut a pair, or \
                 drop `sym=true` to keep the one opening that was cut",
            )
        };
        diagnostics.push(diag_deferred_member_noted(
            member,
            &format!(
                "`sym=true` window at offset={}{} size={sw}x{sh} on the `{}` wall {relation} its mirror (wall length={len}); the mirror was skipped",
                args.offset(),
                args.anchor_written(),
                side_name(side),
            ),
            &note,
        ));
        return None;
    }
    Some(mirror_offset)
}

/// The column a `sym=true` window's mirror starts at on a wall `len` long.
fn mirror_column(args: WindowArgs, len: u32) -> u32 {
    len.saturating_sub(args.column).saturating_sub(args.width)
}

/// The part of its wall an opening cuts, as a `beam` laid over it reads it.
#[derive(Debug, Clone)]
pub(super) struct Opening {
    pub(super) side: WallSide,
    /// Its highest row, from the struct's base.
    pub(super) top: u32,
    /// The `(offset, width)` of each rectangle it cuts.
    pub(super) stamps: Vec<(u32, u32)>,
}

/// What the `door` or `window` `member` under `level y=y_offset` cuts, or `None` when its line
/// does not place it; asked without a finding, as its own line reports each.
pub(super) fn placed_opening(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Option<Opening> {
    let mut reported_on_its_line = Vec::new();
    if matches!(member.role, MemberRole::Door) {
        let (states, _) = read_door_states(member);
        let door = plan_door(member, states, y_offset, ctx, &mut reported_on_its_line)?;
        return Some(Opening {
            side: door.side,
            top: door.base_row.saturating_add(DOOR_HEIGHT - 1),
            stamps: vec![(door.at, 1)],
        });
    }
    let (sym, _) = read_sym(member);
    let (shape, _) = read_shape(member);
    match plan_window(member, sym, shape, y_offset, ctx, &mut reported_on_its_line) {
        WindowPlan::Cut(cut) => Some(Opening {
            side: cut.side,
            top: cut.row.saturating_add(cut.height.saturating_sub(1)),
            stamps: cut.stamps,
        }),
        WindowPlan::Refused | WindowPlan::RefusedForUnreadSym => None,
    }
}
