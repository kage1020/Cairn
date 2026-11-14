//! The constraint catalog's checks on one lowered body: a block the game drops for want of a face
//! to hang on or of support, a block that falls, and water that flows out of the build
//! (`spec/lint` "Constraint checks").

use std::collections::HashSet;

use crate::block_array::constraint::{Constraint, Support};
use crate::block_array::joins::Face;
use crate::block_array::material::TargetRegistry;
use crate::block_array::{BlockArray, BlockState, PaletteIndex, ResolvedMember};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};

use super::waterlogged::WATERLOGGED;

/// A cell of the array, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// The ways water flows out of the cell holding it.
const FLOWS: [Face; 5] = [Face::Down, Face::North, Face::East, Face::South, Face::West];

/// The fluids a gravity block falls through as it does through air.
const FLUIDS: [&str; 2] = [BlockState::WATER_ID, "minecraft:lava"];

/// The findings of every check `registry`'s catalog asks of `members`, the members of `array`,
/// whose interior starts `overhang` cells in along x and z.
pub(super) fn constraint_findings(
    members: &[ResolvedMember],
    array: &BlockArray,
    overhang: u32,
    registry: &dyn TargetRegistry,
) -> Vec<Diagnostic> {
    let grid = Grid { array, overhang };
    members
        .iter()
        .flat_map(|member| member_findings(member, &grid, registry))
        .collect()
}

/// The array the checks read, and where its interior starts.
struct Grid<'a> {
    array: &'a BlockArray,
    overhang: u32,
}

impl<'a> Grid<'a> {
    /// The cell `face` leads to from `cell`, with its slot and state; `None` outside the array.
    fn beside(&self, cell: Cell, face: Face) -> Option<(Cell, PaletteIndex, &'a BlockState)> {
        let (dx, dy, dz) = face.step();
        let next = (
            cell.0.checked_add_signed(dx)?,
            cell.1.checked_add_signed(dy)?,
            cell.2.checked_add_signed(dz)?,
        );
        let slot = self.array.voxels[self.array.dims.index(next.0, next.1, next.2)?];
        Some((next, slot, &self.array.palette.entries[usize::from(slot.0)]))
    }

    /// `cell` as a member's `at=` names it, counted from the interior's corner.
    fn named(&self, cell: Cell) -> String {
        let inward = i64::from(self.overhang);
        format!(
            "{},{},{}",
            i64::from(cell.0) - inward,
            cell.1,
            i64::from(cell.2) - inward
        )
    }
}

/// One block a check fails on: its state and cell, and the state of the cell it fails against.
struct Failed<'a> {
    state: &'a BlockState,
    cell: Cell,
    /// The way from the block to the cell it fails against.
    toward: Face,
    beside: &'a BlockState,
    beside_cell: Cell,
}

/// The first block of a member one check fails on, and how many it fails on.
struct Tally<T> {
    first: T,
    count: usize,
}

impl<T> Tally<T> {
    fn add(slot: &mut Option<Self>, found: T) {
        match slot {
            Some(tally) => tally.count += 1,
            None => {
                *slot = Some(Self {
                    first: found,
                    count: 1,
                });
            }
        }
    }
}

/// The findings on one member: one per check that fails on any of its blocks.
fn member_findings(
    member: &ResolvedMember,
    grid: &Grid<'_>,
    registry: &dyn TargetRegistry,
) -> Vec<Diagnostic> {
    let mut hangs: Option<Tally<Failed<'_>>> = None;
    let mut rests: Option<Tally<(Failed<'_>, Support)>> = None;
    let mut falls: Option<Tally<Failed<'_>>> = None;
    let mut spill: Option<Failed<'_>> = None;
    let mut flooded: HashSet<Cell> = HashSet::new();
    for block in &member.resolved_state {
        let state = &block.state;
        let constraint = registry.constraint(state);
        let water = holds_water(state);
        if constraint.is_none() && !water {
            continue;
        }
        for cell in block.cells() {
            match constraint {
                Some(Constraint::Hangs { host }) => {
                    if let Some(failed) = unheld(grid, registry, state, cell, host) {
                        Tally::add(&mut hangs, failed);
                    }
                }
                Some(Constraint::Rests { host, needs }) => {
                    if let Some(failed) = unheld(grid, registry, state, cell, host) {
                        Tally::add(&mut rests, (failed, needs));
                    }
                }
                Some(Constraint::Falls) => {
                    if let Some(failed) = falling(grid, state, cell) {
                        Tally::add(&mut falls, failed);
                    }
                }
                None => {}
            }
            if water {
                for failed in spilling(grid, registry, state, cell) {
                    if flooded.insert(failed.beside_cell) && spill.is_none() {
                        spill = Some(failed);
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    if let Some(tally) = hangs {
        found.push(diag_no_attachment_face(member, grid, &tally));
    }
    if let Some(tally) = rests {
        found.push(diag_unsupported(member, grid, &tally));
    }
    if let Some(tally) = falls {
        found.push(diag_falls(member, grid, &tally));
    }
    if let Some(first) = spill {
        found.push(diag_water_spill(member, grid, &first, flooded.len()));
    }
    found
}

/// Whether `state` holds water: a water block, or one waterlogged.
fn holds_water(state: &BlockState) -> bool {
    state.id == BlockState::WATER_ID
        || state
            .properties
            .get(WATERLOGGED)
            .is_some_and(|value| value == "true")
}

/// The block `state` at `cell` as failing, when the cell `host` leads to does not hold it; `None`
/// when it does, lies outside the array, or the registry cannot say.
fn unheld<'a>(
    grid: &Grid<'a>,
    registry: &dyn TargetRegistry,
    state: &'a BlockState,
    cell: Cell,
    host: Face,
) -> Option<Failed<'a>> {
    let (beside_cell, slot, beside) = grid.beside(cell, host)?;
    let holds = slot != PaletteIndex::AIR && registry.holds(state, beside)?;
    (!holds).then_some(Failed {
        state,
        cell,
        toward: host,
        beside,
        beside_cell,
    })
}

/// The gravity block at `cell` as failing, when the cell below it holds air or a fluid.
fn falling<'a>(grid: &Grid<'a>, state: &'a BlockState, cell: Cell) -> Option<Failed<'a>> {
    let (beside_cell, slot, beside) = grid.beside(cell, Face::Down)?;
    (slot == PaletteIndex::AIR || FLUIDS.contains(&beside.id.as_str())).then_some(Failed {
        state,
        cell,
        toward: Face::Down,
        beside,
        beside_cell,
    })
}

/// Each cell of air the water `state` holds at `cell` flows into, through a face of it the
/// registry says does not keep the water in.
fn spilling<'a>(
    grid: &Grid<'a>,
    registry: &dyn TargetRegistry,
    state: &'a BlockState,
    cell: Cell,
) -> impl Iterator<Item = Failed<'a>> {
    FLOWS.into_iter().filter_map(move |face| {
        let (beside_cell, slot, beside) = grid.beside(cell, face)?;
        if slot != PaletteIndex::AIR {
            return None;
        }
        let open = registry.keeps_water(state, face) == Some(false);
        open.then_some(Failed {
            state,
            cell,
            toward: face,
            beside,
            beside_cell,
        })
    })
}

/// `state` as a state literal writes it, such as `minecraft:lantern[hanging=true]`.
fn written(state: &BlockState) -> String {
    if state.properties.is_empty() {
        state.id.clone()
    } else {
        format!("{}[{}]", state.id, state.properties_text())
    }
}

/// How a primary names `count` failing blocks, what one or more of them do, and the first of
/// them: `a block {one}: {first}` for one, `3 blocks {many}: the first, {first},` for more.
fn counted(count: usize, [one, many]: [&str; 2], first: &str) -> String {
    if count == 1 {
        format!("a block {one}: {first}")
    } else {
        format!("{count} blocks {many}: the first, {first},")
    }
}

/// What a failing block meets: air, or the face of the block it meets.
fn met(failed: &Failed<'_>) -> String {
    if failed.beside.id == BlockState::AIR_ID {
        "air".to_owned()
    } else {
        format!(
            "the {} face of `{}`",
            failed.toward.opposite().as_str(),
            written(failed.beside)
        )
    }
}

fn note(message: &str) -> DiagnosticNote {
    DiagnosticNote {
        span: None,
        message: message.to_owned(),
    }
}

fn diag_no_attachment_face(
    member: &ResolvedMember,
    grid: &Grid<'_>,
    tally: &Tally<Failed<'_>>,
) -> Diagnostic {
    let first = &tally.first;
    Diagnostic {
        code: DiagnosticCode::NoAttachmentFace,
        span: member.span.clone(),
        primary: format!(
            "`{}` builds {} hangs on {}",
            member.role,
            counted(
                tally.count,
                ["the game drops for want of a whole face to hang on"; 2],
                &format!("`{}` at {}", written(first.state), grid.named(first.cell)),
            ),
            met(first),
        ),
        notes: vec![
            note(
                "it hangs only on a whole face: a full block's side, or the face of a slab or \
                 stair that covers its cell; a glass pane, fence, wall or air gives none",
            ),
            note("put a full block where it hangs, or move the member so it hangs on one"),
        ],
        data: None,
    }
}

fn diag_unsupported(
    member: &ResolvedMember,
    grid: &Grid<'_>,
    tally: &Tally<(Failed<'_>, Support)>,
) -> Diagnostic {
    let (first, needs) = &tally.first;
    let (meets, below_or_above) = if first.toward == Face::Up {
        ("hangs from", "above")
    } else {
        ("stands on", "below")
    };
    let valid = match needs {
        Support::Sturdy => format!(
            "it needs the whole face of the block {below_or_above} it: a full block, or a slab \
             or stair whose face covers its cell"
        ),
        Support::Center => format!(
            "it needs the centre of the face of the block {below_or_above} it: a full block, a \
             slab or stair whose face covers its cell, or the post of a fence, wall, pane or chain"
        ),
    };
    Diagnostic {
        code: DiagnosticCode::UnsupportedBlock,
        span: member.span.clone(),
        primary: format!(
            "`{}` builds {} {meets} {}",
            member.role,
            counted(
                tally.count,
                ["the game drops for want of support"; 2],
                &format!("`{}` at {}", written(first.state), grid.named(first.cell)),
            ),
            met(first),
        ),
        notes: vec![
            note(&valid),
            note(&format!(
                "put a block that gives it that face {below_or_above} it, or move the member \
                 onto one"
            )),
        ],
        data: None,
    }
}

fn diag_falls(member: &ResolvedMember, grid: &Grid<'_>, tally: &Tally<Failed<'_>>) -> Diagnostic {
    let first = &tally.first;
    let under = if first.beside.id == BlockState::AIR_ID {
        "air".to_owned()
    } else {
        format!("`{}`", written(first.beside))
    };
    Diagnostic {
        code: DiagnosticCode::UnsupportedBlock,
        span: member.span.clone(),
        primary: format!(
            "`{}` builds {} stands on {under}",
            member.role,
            counted(
                tally.count,
                ["that falls", "that fall"],
                &format!("`{}` at {}", written(first.state), grid.named(first.cell)),
            ),
        ),
        notes: vec![
            note("it stays only on a block, and falls through air or a fluid"),
            note("put a block under it, or build it of a block that does not fall"),
        ],
        data: None,
    }
}

fn diag_water_spill(
    member: &ResolvedMember,
    grid: &Grid<'_>,
    first: &Failed<'_>,
    cells: usize,
) -> Diagnostic {
    let into = if cells == 1 {
        format!("the air at {}", grid.named(first.beside_cell))
    } else {
        format!(
            "{cells} cells of air: the first, at {},",
            grid.named(first.beside_cell)
        )
    };
    Diagnostic {
        code: DiagnosticCode::WaterSpill,
        span: member.span.clone(),
        primary: format!(
            "`{}` holds water that flows into {into} from `{}` at {}",
            member.role,
            written(first.state),
            grid.named(first.cell),
        ),
        notes: vec![
            note(
                "water stays where each face it flows through, down and sideways, meets a \
                 block; a waterlogged block keeps it in only through a whole face",
            ),
            note(
                "wall the water in, fill the air it flows into, or write `waterlogged=false` on \
                 a block that should hold none",
            ),
        ],
        data: None,
    }
}
