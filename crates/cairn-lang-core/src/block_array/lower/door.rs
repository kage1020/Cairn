//! Setting a `door` member's door block into the opening it cuts (`spec/blockstate` "Derive by
//! default, promote on override").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{
    UnreadArgument, diag_deferred_member_noted, diag_deferred_member_reason, diag_invalid_member,
    ident_or_ignore, read_or_ignore,
};
use crate::block_array::openings::{
    DOOR_HEIGHT, WallSide, door_anchor_offset, door_at_refusal, wall_length, wall_local_to_grid,
};
use crate::block_array::roof::inward_cardinal;
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{
    MemberState, StructCtx, UNPAINTED_COURSE, default_block_id, resolve_member_state, side_name,
    side_of, unplaced_walls_note,
};
use super::phase::Phase;

/// Catalog token a `door`'s default material comes from; the pack spells it per edition.
pub(super) const DOOR_TOKEN: &str = "door.default";

/// Door id used when the pack cannot supply one: the Java spelling, Java being the base edition
/// (`spec/versioning-editions` "Backend = data tables").
pub(super) const DOOR_BASE_ID: &str = "minecraft:oak_door";

/// The side of a door its hinge is on, as seen from outside the wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hinge {
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
    fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// A door block a `door` member stood in its opening, which [`hang_doors`] hinges or counts as a
/// neighbour.
pub(super) struct HungDoor {
    /// The member's index in the flattened paint list.
    member: u32,
    /// The cell of its lower half; the upper half is the one above it.
    lower: (u32, u32, u32),
    side: WallSide,
    id: String,
    /// `hinge=` as the line writes it, or `None` when it is derived.
    hinge: Option<Hinge>,
    open: bool,
    /// The slots [`fill_door`] painted its halves with, lower first.
    painted: [PaletteIndex; 2],
}

impl HungDoor {
    /// The door's two cells, each with the `half` it holds.
    fn cells(&self) -> [(&'static str, (u32, u32, u32)); 2] {
        let (x, y, z) = self.lower;
        [
            ("lower", self.lower),
            ("upper", (x, y.saturating_add(1), z)),
        ]
    }

    /// The state the door's `half` cell holds with `hinge`.
    fn state(&self, half: &str, hinge: Hinge) -> BlockState {
        let mut properties = IndexMap::new();
        properties.insert(
            "facing".to_owned(),
            inward_cardinal(self.side).as_str().to_owned(),
        );
        properties.insert("half".to_owned(), half.to_owned());
        properties.insert("hinge".to_owned(), hinge.as_str().to_owned());
        properties.insert("open".to_owned(), self.open.to_string());
        BlockState {
            id: self.id.clone(),
            properties,
        }
    }
}

/// Stand a `door`'s door block in the opening it cuts, adding it to `hung` once it stands.
pub(super) fn fill_door(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    hung: &mut Vec<HungDoor>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the door, so every unreadable state is reported now.
    let mut unread: Vec<(UnreadArgument, &str)> = Vec::new();
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
    let written = WrittenStates {
        hinge,
        open: open.unwrap_or(false),
    };
    let door = stand_door(member, canvas.member(), written, y_offset, ctx, diagnostics);
    let built = door.is_some();
    if let Some(mut door) = door {
        if door.hinge.is_none() {
            canvas.hold_contests();
        }
        // A derived door stands hinged on the left until `hang_doors` settles it.
        let hinge = door.hinge.unwrap_or(Hinge::Left);
        let mut painted = [PaletteIndex::AIR; 2];
        for (slot, (half, pos)) in painted.iter_mut().zip(door.cells()) {
            canvas.paint(pos, || {
                *slot = palette.intern(door.state(half, hinge));
                *slot
            });
        }
        door.painted = painted;
        hung.push(door);
    }
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(if built { consequence } else { NOT_BUILT }));
    }
}

/// The consequence an unread state of a door that is not built reports.
const NOT_BUILT: &str = "this door is not built either way — see the finding on the same line, or \
                         on the material its `mat_slot=` names";

/// The `hinge=` and `open=` a door's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    hinge: Option<&'m str>,
    open: bool,
}

/// The door [`fill_door`] stands for the member at `index` of the flattened paint list, or `None`
/// after the findings that refuse it.
///
/// Every argument and the material are judged whatever else refuses the door, so one build reports
/// each fault of its own; the course is judged only for a line with none.
fn stand_door(
    member: &Member,
    index: u32,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<HungDoor> {
    let side = side_of(member, diagnostics);
    let at = side.and_then(|side| {
        door_anchor_offset(member, wall_length(side, ctx.interior_w, ctx.interior_h))
            .map_err(|written| {
                diagnostics.push(diag_invalid_member(
                    member,
                    door_at_refusal(&written),
                    Vec::new(),
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
    let base_row = y_offset.saturating_add(1);
    let in_course = line.is_some() && ctx.wall_column.contains_rows(base_row, DOOR_HEIGHT);
    if line.is_some() && !in_course {
        diagnostics.push(door_course_refusal(member, base_row, ctx));
    }
    let id = door_block_id(member, ctx, diagnostics)
        .map_err(|refusal| diagnostics.push(*refusal))
        .ok()
        .flatten();
    let (Some((side, at, hinge)), true, Some(id)) = (line, in_course, id) else {
        return None;
    };
    let Some(lower) = wall_local_to_grid(
        side,
        at,
        base_row,
        ctx.overhang,
        ctx.interior_w,
        ctx.interior_h,
        ctx.dims,
    ) else {
        // INVARIANT(wall-grid-validated): `at < len`, and both rows sit in a painted course.
        debug_assert!(
            false,
            "door row y={base_row} at u={at} on the {} wall is outside the wall grid; `at=` \
             against `wall_length` / the rows against the course stopped agreeing with \
             `wall_local_to_grid`",
            side_name(side),
        );
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door resolved to cells outside the struct's block array",
        ));
        return None;
    };
    Some(HungDoor {
        member: index,
        lower,
        side,
        id,
        hinge,
        open: written.open,
        painted: [PaletteIndex::AIR; 2],
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

/// Settle the hinge of each door in `hung` whose line writes none, from the wall beside it now
/// that every opening is cut, then judge the contests its cells took part in.
pub(super) fn hang_doors(hung: &[HungDoor], palette: &mut ScopePalette, canvas: &mut Canvas) {
    let derived: Vec<(&HungDoor, Hinge)> = hung
        .iter()
        .filter(|door| door.hinge.is_none())
        .map(|door| (door, derived_hinge(door, hung, canvas)))
        .collect();
    let mut settled: Vec<(u32, PaletteIndex, PaletteIndex)> = Vec::new();
    for (door, hinge) in derived {
        for ((half, pos), painted) in door.cells().into_iter().zip(door.painted) {
            let slot = palette.intern(door.state(half, hinge));
            // A cell a later opening took is that opening's.
            if canvas.owner_at(pos) == Some(door.member) {
                canvas.for_member(door.member).paint(pos, || slot);
            } else {
                canvas.name(slot);
            }
            settled.push((door.member, painted, slot));
        }
    }
    canvas.settle_contests(|member, slot| {
        settled
            .iter()
            .find(|&&(door, painted, _)| door == member && painted == slot)
            .map_or(slot, |&(_, _, settled)| settled)
    });
}

/// The members of `hung` that hold one of their door's two cells, a later opening having taken
/// the other.
pub(super) fn split_doors(hung: &[HungDoor], canvas: &Canvas) -> Vec<u32> {
    hung.iter()
        .filter(|door| {
            door.cells()
                .into_iter()
                .filter(|&(_, pos)| canvas.owner_at(pos) == Some(door.member))
                .count()
                == 1
        })
        .map(|door| door.member)
        .collect()
}

/// The hinge `spec/blockstate` "Derive by default, promote on override" gives `door`: on the right
/// with a door on its left and none on its right, or with more wall on its right than its left.
fn derived_hinge(door: &HungDoor, hung: &[HungDoor], canvas: &Canvas) -> Hinge {
    let (dx, dz) = door.side.along();
    let beside = |toward: i32| {
        let step = |(x, y, z): (u32, u32, u32)| {
            Some((
                x.checked_add_signed(dx * toward)?,
                y,
                z.checked_add_signed(dz * toward)?,
            ))
        };
        // Above row 0, where a door stands, only `walls` members paint in the massing phase.
        let wall = door
            .cells()
            .into_iter()
            .filter_map(|(_, pos)| step(pos))
            .filter(|&pos| canvas.written_in(pos, Phase::Massing))
            .count();
        let has_door = step(door.lower).is_some_and(|pos| {
            hung.iter()
                .any(|other| other.lower == pos && canvas.owner_at(pos) == Some(other.member))
        });
        (wall, has_door)
    };
    let (left_wall, left_door) = beside(-1);
    let (right_wall, right_door) = beside(1);
    if (left_door && !right_door) || right_wall > left_wall {
        Hinge::Right
    } else {
        Hinge::Left
    }
}

/// The finding that refuses a door at `base_row`, whose two rows no wall course holds.
///
/// A row a `walls` member is written to hold and does not paint is that member's to repair.
fn door_course_refusal(member: &Member, base_row: u32, ctx: &StructCtx<'_>) -> Diagnostic {
    let written = &ctx.written;
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
