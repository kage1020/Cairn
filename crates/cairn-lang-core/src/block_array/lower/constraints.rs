//! The constraint catalog's checks on one lowered body: a block the game drops for want of a face
//! to hang on or of support, a block that falls, water that flows out of the build, an entity
//! whose box something clips, and one that hangs on nothing (`spec/lint` "Constraint checks",
//! "Entity AABB").

use std::collections::HashSet;

use crate::block_array::constraint::{Constraint, EntityBox, Support};
use crate::block_array::joins::Face;
use crate::block_array::material::TargetRegistry;
use crate::block_array::{
    BlockArray, BlockState, Entity, EntityData, PaletteIndex, ResolvedBlock, ResolvedMember,
};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;

use super::waterlogged::WATERLOGGED;

/// A cell of the array, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// The ways water flows out of the cell holding it.
const FLOWS: [Face; 5] = [Face::Down, Face::North, Face::East, Face::South, Face::West];

/// The fluids a gravity block falls through as it does through air.
const FLUIDS: [&str; 2] = [BlockState::WATER_ID, "minecraft:lava"];

/// One lowered body the checks read: its members, its array, and where its interior starts along
/// x and z.
pub(super) struct Built<'a> {
    pub(super) members: &'a [ResolvedMember],
    pub(super) array: &'a BlockArray,
    pub(super) overhang: u32,
}

/// The findings of every check `registry`'s catalog asks of the blocks of `built`.
pub(super) fn constraint_findings(
    built: &Built<'_>,
    registry: &dyn TargetRegistry,
) -> Vec<Diagnostic> {
    let grid = Grid {
        array: built.array,
        overhang: built.overhang,
    };
    built
        .members
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

/// An entity a member placed, with the member's keyword and line.
pub(super) struct PlacedEntity<'a> {
    pub(super) role: &'a str,
    pub(super) span: &'a Span,
    pub(super) entity: &'a Entity,
}

/// How far two boxes must reach into each other to overlap, in blocks; any less is touching.
const TOUCHING: f64 = 1e-9;

/// The keyword of the member whose cells an entity's box may not reach into.
const DOOR: &str = "door";

/// A box in the array's coordinates, by its low and high corners.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Aabb {
    low: [f64; 3],
    high: [f64; 3],
}

impl Aabb {
    /// Whether the box holds no space, as a display's, which nothing collides with.
    fn is_empty(&self) -> bool {
        (0..3).any(|axis| self.high[axis] - self.low[axis] <= TOUCHING)
    }

    /// Whether `self` and `other` overlap, more than meeting at a face.
    fn overlaps(&self, other: &Self) -> bool {
        (0..3).all(|axis| {
            self.low[axis] < other.high[axis] - TOUCHING
                && other.low[axis] < self.high[axis] - TOUCHING
        })
    }

    /// Each cell of `array` the box reaches into, in `Dims::index` order.
    fn cells(&self, array: &BlockArray) -> Vec<Cell> {
        let span = |axis: usize, extent: u32| {
            let low = (self.low[axis] + TOUCHING)
                .floor()
                .clamp(0.0, f64::from(extent));
            let high = (self.high[axis] - TOUCHING)
                .ceil()
                .clamp(0.0, f64::from(extent));
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0..=extent, which a u32 holds whole"
            )]
            let range = (low as u32)..(high as u32);
            range
        };
        let dims = array.dims;
        let mut cells = Vec::new();
        for y in span(1, dims.y) {
            for z in span(2, dims.z) {
                for x in span(0, dims.x) {
                    cells.push((x, y, z));
                }
            }
        }
        cells
    }
}

/// The box `shape` gives `entity`: standing on its position, or out of the face it hangs on
/// (`spec/lint` "Entity AABB").
fn entity_aabb(entity: &Entity, shape: EntityBox) -> Aabb {
    let Some(depth) = shape.depth else {
        let (x, y, z) = entity.pos;
        let half = shape.width / 2.0;
        return Aabb {
            low: [x - half, y, z - half],
            high: [x + half, y + shape.height, z + half],
        };
    };
    let (across, up) = match entity.data {
        EntityData::Painting { width, height, .. } => (width, height),
        _ => (1, 1),
    };
    let facing = entity.facing;
    // A wall's face runs to the right of the way the entity faces and up; a floor's or a
    // ceiling's along x and z.
    let (right, upward) = match facing.counter_clockwise() {
        Some(right) => (right, Face::Up),
        None => (Face::East, Face::South),
    };
    let (x, y, z) = entity.block_pos;
    let hung = [f64::from(x), f64::from(y), f64::from(z)];
    let mut low = hung;
    let mut high = hung.map(|at| at + 1.0);
    for (face, blocks, size) in [(right, across, shape.width), (upward, up, shape.height)] {
        let (axis, toward) = axis_of(face);
        // The hung cell is `(blocks - 1) / 2` blocks from the painting's first.
        let first = hung[axis] - toward * f64::from((blocks - 1) / 2);
        let last = first + toward * f64::from(blocks - 1);
        let middle = (first.min(last) + first.max(last) + 1.0) / 2.0;
        let half = f64::from(blocks) * size / 2.0;
        low[axis] = middle - half;
        high[axis] = middle + half;
    }
    let (out, toward) = axis_of(facing);
    if toward > 0.0 {
        high[out] = low[out] + depth;
    } else {
        low[out] = high[out] - depth;
    }
    Aabb { low, high }
}

/// The axis `face` points along, 0 for x, 1 for y and 2 for z, and its sign along it.
fn axis_of(face: Face) -> (usize, f64) {
    match face.step() {
        (dx, 0, 0) => (0, f64::from(dx)),
        (0, dy, 0) => (1, f64::from(dy)),
        (_, _, dz) => (2, f64::from(dz)),
    }
}

/// What an entity's box reaches into.
enum Clipped<'a> {
    /// A full block, at its cell.
    Block(&'a BlockState, Cell),
    /// A cell of a door.
    Door(Cell),
    /// The box of an entity a line written before it places.
    Entity(&'a PlacedEntity<'a>),
}

/// The findings on each of `entities` the catalog gives a box: `W_ENTITY_CLIPPING` where the box
/// reaches into a full block of `built`, a door's cell or the box of an earlier line's entity, and
/// `W_NO_ATTACHMENT_FACE` where nothing stands behind one that hangs (`spec/lint` "Entity AABB").
pub(super) fn entity_findings(
    built: &Built<'_>,
    entities: &[PlacedEntity<'_>],
    registry: &dyn TargetRegistry,
) -> Vec<Diagnostic> {
    let grid = Grid {
        array: built.array,
        overhang: built.overhang,
    };
    let doors: HashSet<Cell> = built
        .members
        .iter()
        .filter(|member| member.role == DOOR)
        .flat_map(|member| member.resolved_state.iter().flat_map(ResolvedBlock::cells))
        .collect();
    let boxes: Vec<Option<(Aabb, bool)>> = entities
        .iter()
        .map(|placed| {
            let shape = registry.entity_box(&placed.entity.id)?;
            Some((entity_aabb(placed.entity, shape), shape.depth.is_some()))
        })
        .collect();
    let mut found = Vec::new();
    for (at, placed) in entities.iter().enumerate() {
        let Some((aabb, hangs)) = boxes[at] else {
            continue;
        };
        if hangs {
            let unheld: Vec<(Cell, &BlockState)> = aabb
                .cells(built.array)
                .into_iter()
                .filter_map(|cell| {
                    let (behind, slot, state) =
                        grid.beside(cell, placed.entity.facing.opposite())?;
                    (slot == PaletteIndex::AIR || FLUIDS.contains(&state.id.as_str()))
                        .then_some((behind, state))
                })
                .collect();
            if let Some(&first) = unheld.first() {
                found.push(diag_entity_unheld(placed, &grid, first, unheld.len()));
            }
        }
        if aabb.is_empty() {
            continue;
        }
        let mut clipped: Vec<Clipped<'_>> = aabb
            .cells(built.array)
            .into_iter()
            .filter_map(|cell| {
                if doors.contains(&cell) {
                    return Some(Clipped::Door(cell));
                }
                let (array, (x, y, z)) = (built.array, cell);
                let slot = array.voxels[array.dims.index(x, y, z)?];
                let state = &array.palette.entries[usize::from(slot.0)];
                (slot != PaletteIndex::AIR && registry.solid(state) == Some(true))
                    .then_some(Clipped::Block(state, cell))
            })
            .collect();
        clipped.extend(
            (0..entities.len())
                .filter(|&other| {
                    entities[other].span.start < placed.span.start
                        && boxes[other]
                            .is_some_and(|(other, _)| !other.is_empty() && other.overlaps(&aabb))
                })
                .map(|before| Clipped::Entity(&entities[before])),
        );
        if let Some(first) = clipped.first() {
            found.push(diag_entity_clipping(placed, &grid, first, clipped.len()));
        }
    }
    found
}

/// How a finding names an entity a member places: the member, the entity and its cell.
fn placing(placed: &PlacedEntity<'_>, grid: &Grid<'_>) -> String {
    format!(
        "`{}` places `{}` at {}",
        placed.role,
        placed.entity.id,
        grid.named(placed.entity.block_pos),
    )
}

fn diag_entity_clipping(
    placed: &PlacedEntity<'_>,
    grid: &Grid<'_>,
    first: &Clipped<'_>,
    count: usize,
) -> Diagnostic {
    let (what, fix) = match first {
        Clipped::Block(state, cell) => (
            format!(
                "the full block `{}` at {}",
                written(state),
                grid.named(*cell)
            ),
            "move the member to cells its box fits in, or take the block out of them",
        ),
        Clipped::Door(cell) => (
            format!(
                "the door at {}, whose swing passes through it",
                grid.named(*cell)
            ),
            "move the member out of the door's cells",
        ),
        Clipped::Entity(other) => (
            format!(
                "the box of `{}` at {}, which the `{}` line before it places",
                other.entity.id,
                grid.named(other.entity.block_pos),
                other.role,
            ),
            "move one of the two members so that their boxes do not overlap",
        ),
    };
    let into = if count == 1 {
        what
    } else {
        format!("{count} places it must stay clear of: the first, {what},")
    };
    Diagnostic {
        code: DiagnosticCode::EntityClipping,
        span: placed.span.clone(),
        primary: format!("{}, and its box reaches into {into}", placing(placed, grid)),
        notes: vec![
            note(
                "an entity's box stays clear of a full block, a door's cells and another \
                 entity's box, though it may meet one at a face",
            ),
            note(fix),
        ],
        data: None,
    }
}

fn diag_entity_unheld(
    placed: &PlacedEntity<'_>,
    grid: &Grid<'_>,
    (behind, state): (Cell, &BlockState),
    count: usize,
) -> Diagnostic {
    let holds = if state.id == BlockState::AIR_ID {
        "air".to_owned()
    } else {
        format!("`{}`", written(state))
    };
    let at = if count == 1 {
        format!("the cell behind it, {}, holds {holds}", grid.named(behind))
    } else {
        format!(
            "{count} cells behind it hold nothing it hangs on: the first, {}, holds {holds}",
            grid.named(behind)
        )
    };
    Diagnostic {
        code: DiagnosticCode::NoAttachmentFace,
        span: placed.span.clone(),
        primary: format!("{}, where the game drops it: {at}", placing(placed, grid)),
        notes: vec![
            note(
                "an item frame or a painting hangs only on blocks the game counts as solid, \
                 one behind each cell it covers; air, water and lava hold none",
            ),
            note("put a block behind it, or move the member onto a wall"),
        ],
        data: None,
    }
}
