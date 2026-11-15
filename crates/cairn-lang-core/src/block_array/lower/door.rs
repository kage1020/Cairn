//! Setting a `door` member's door block into the opening it cuts (`spec/blockstate` "Derive by
//! default, promote on override").

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::intent::{BindingKey, Member};

use crate::block_array::args::diag_deferred_member_reason;
use crate::block_array::openings::{DOOR_HEIGHT, WallSide, wall_local_to_grid};
use crate::block_array::roof::inward_cardinal;
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::frame::{Cut, Frame};
use super::members::{StructCtx, bound_block, side_name};
use super::opening::{DoorPlan, DoorStates, Hinge, plan_door, read_door_states};

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

/// Stand a `door`'s door block in the opening it cuts, adding it to `hung` once it stands, with the
/// frame a theme binds on it round that opening.
pub(super) fn fill_door(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    hung: &mut Vec<HungDoor>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Frame> {
    let frame = bound_block(member, BindingKey::Frame, ctx, diagnostics);
    // Read before anything can refuse the door, so every unreadable state is reported now.
    let (written, unread) = read_door_states(member);
    let door = stand_door(member, canvas.member(), written, y_offset, ctx, diagnostics);
    let built = door.is_some();
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(if built { consequence } else { NOT_BUILT }));
    }
    let (mut door, cut) = door?;
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
    frame.map(|state| Frame::new(canvas.member(), state, vec![cut]))
}

/// The consequence an unread state of a door that is not built reports.
const NOT_BUILT: &str = "this door is not built either way — see the finding on the same line, or \
                         on the material its `mat_slot=` names";

/// The door [`fill_door`] stands for the member at `index` of the flattened paint list, with the
/// opening it cuts, or `None` after the findings that refuse it.
fn stand_door(
    member: &Member,
    index: u32,
    written: DoorStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(HungDoor, Cut)> {
    let DoorPlan {
        side,
        at,
        base_row,
        hinge,
        id,
    } = plan_door(member, written, y_offset, ctx, diagnostics)?;
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
    let door = HungDoor {
        member: index,
        lower,
        side,
        id,
        hinge,
        open: written.open,
        painted: [PaletteIndex::AIR; 2],
    };
    let cut = Cut {
        side,
        offset: at,
        y_start: base_row,
        width: 1,
        height: DOOR_HEIGHT,
    };
    Some((door, cut))
}

/// Settle the hinge of each door in `hung` whose line writes none, from the wall beside it now
/// that every opening is cut, then judge the contests its cells took part in; `walls` are the
/// paint-list indices of the `walls` members.
pub(super) fn hang_doors(
    hung: &[HungDoor],
    walls: &[u32],
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
) {
    let derived: Vec<(&HungDoor, Hinge)> = hung
        .iter()
        .filter(|door| door.hinge.is_none())
        .map(|door| (door, derived_hinge(door, hung, walls, canvas)))
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
