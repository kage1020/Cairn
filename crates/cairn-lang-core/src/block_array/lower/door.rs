//! Setting a `door` member's door block into the opening it cuts (`spec/blockstate` "Derive by
//! default, promote on override").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::{
    UnreadArgument, diag_deferred_member_noted, diag_invalid_member, ident_or_ignore,
    read_or_ignore,
};
use crate::block_array::openings::{
    WallSide, door_anchor_offset, door_at_refusal, wall_length, wall_local_to_grid,
};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{
    StructCtx, UNPAINTED_COURSE, default_block_id, resolve_member_state, side_name, side_of,
    unplaced_walls_note,
};
use super::stair::inward_cardinal;

/// Catalog token a `door`'s default material comes from; the pack spells it per edition.
pub(super) const DOOR_TOKEN: &str = "door.default";

/// Door id used when the pack cannot supply one: the Java spelling, Java being the base edition
/// (`spec/versioning-editions` "Backend = data tables").
pub(super) const DOOR_BASE_ID: &str = "minecraft:oak_door";

/// Rows a door block stands, from the row it opens at.
const DOOR_HEIGHT: u32 = 2;

/// The side of a door its hinge is on, as seen from outside the wall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hinge {
    Left,
    Right,
}

impl Hinge {
    /// The value of the block's `hinge` state.
    fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// A door block a `door` member stood in its opening, for [`hang_doors`] to settle its hinge.
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
    if let Some(door) = door {
        for (half, pos) in door.cells() {
            let state = door.state(half, door.hinge.unwrap_or(Hinge::Left));
            canvas.paint(pos, || palette.intern(state));
        }
        hung.push(door);
    }
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(if built {
            consequence
        } else {
            "this door is not built either way — see the finding on the same line"
        }));
    }
}

/// The `hinge=` and `open=` a door's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    hinge: Option<&'m str>,
    open: bool,
}

/// The door [`fill_door`] stands for the member at `index` of the flattened paint list, or `None`
/// after the finding that refuses it.
fn stand_door(
    member: &Member,
    index: u32,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<HungDoor> {
    let side = side_of(member, diagnostics)?;
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
            return None;
        }
    };
    let hinge = match written.hinge {
        None => None,
        Some("left") => Some(Hinge::Left),
        Some("right") => Some(Hinge::Right),
        Some(other) => {
            diagnostics.push(diag_invalid_member(
                member,
                format!("unknown door `hinge={other}`"),
                vec!["`hinge=` takes one of left, right".to_owned()],
            ));
            return None;
        }
    };
    // Judged against the course holding the door's rows, never the tallest wall row.
    let base_row = y_offset.saturating_add(1);
    if !ctx.wall_column.contains_rows(base_row, DOOR_HEIGHT) {
        diagnostics.push(door_course_refusal(member, base_row, ctx));
        return None;
    }
    let id = door_block_id(member, ctx, diagnostics)
        .map_err(|refusal| diagnostics.push(*refusal))
        .ok()?;
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
        return None;
    };
    Some(HungDoor {
        member: index,
        lower,
        side,
        id,
        hinge,
        open: written.open,
    })
}

/// The block a door is built from: its `mat_slot=`, or the pack's default door.
fn door_block_id(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<String, Box<Diagnostic>> {
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
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
}

/// Settle the hinge of each door in `hung` whose line writes none, from the wall beside it now
/// that every opening is cut; `walls` are the paint-list indices of the `walls` members.
pub(super) fn hang_doors(
    hung: &[HungDoor],
    walls: &[u32],
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
) {
    let settled: Vec<(&HungDoor, Hinge)> = hung
        .iter()
        .filter(|door| door.hinge.is_none())
        .map(|door| (door, derived_hinge(door, hung, walls, canvas)))
        .collect();
    for (door, hinge) in settled {
        // `fill_door` stood every door hinged on the left.
        if hinge == Hinge::Left {
            continue;
        }
        for (half, pos) in door.cells() {
            // A cell a later opening took is that opening's.
            if canvas.owner_at(pos) != Some(door.member) {
                continue;
            }
            let state = door.state(half, hinge);
            canvas
                .for_member(door.member)
                .paint(pos, || palette.intern(state));
        }
    }
}

/// The hinge `spec/blockstate` "Derive by default, promote on override" gives `door`: on the right
/// with a door on its left and none on its right, or with more wall on its right than its left.
fn derived_hinge(door: &HungDoor, hung: &[HungDoor], walls: &[u32], canvas: &Canvas) -> Hinge {
    let (dx, dz) = door.side.along();
    let beside = |toward: i32| {
        let step = |(x, y, z): (u32, u32, u32)| {
            Some((
                x.checked_add_signed(dx * toward)?,
                y,
                z.checked_add_signed(dz * toward)?,
            ))
        };
        let wall = door
            .cells()
            .into_iter()
            .filter_map(|(_, pos)| step(pos))
            .filter(|&pos| {
                canvas
                    .owner_at(pos)
                    .is_some_and(|owner| walls.contains(&owner))
            })
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
