//! The bodies a build holds, each paired with a body of the structure, and the cells of a pair
//! compared.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::block_array::{BlockArray, BlockArrayIr, BlockState, Dims, ResolvedMember};
use crate::error::Span;
use crate::intent::MemberAddress;
use crate::transliterate::{GridEntry, GridUnit, spelled, unit_names};

use super::{
    BodyComparison, Counts, Difference, RegionKind, StateDifference, StructureBodies, is_air,
};

/// The state lowering leaves out where no water overlaps a block, which the game then places dry.
const WATERLOGGED: &str = "waterlogged";

/// A cell, `(x, y, z)`.
pub(super) type Cell = (u32, u32, u32);

/// A box of cells, its low corner included and its high corner not.
type Extent = (Cell, Cell);

/// What a comparison keeps of each pair beyond its counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Detail {
    /// The counts alone.
    Counts,
    /// The counts, each cell that differs and the cells each member holds.
    Cells,
}

/// What last wrote a cell of a built body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum By {
    /// Nothing: no member or walkway owns the cell.
    Nothing,
    /// The member at this index of [`BuiltBody::members`].
    Member(usize),
    /// A walkway of the site.
    Walkway,
}

/// A member of a built body, with the blocks it holds there.
#[derive(Debug, Clone)]
pub(super) struct Owned {
    /// The keyword its line starts with.
    pub(super) role: String,
    /// Its `id=`.
    pub(super) id: Option<String>,
    /// Its address in the body it is written in.
    pub(super) address: Option<MemberAddress>,
    /// Its line.
    pub(super) span: Span,
    /// Whether it is a `raw_block` or a `raw_fill`.
    pub(super) raw: bool,
    /// The low corner of the box its line declares, in the body.
    pub(super) declared_low: Cell,
    /// Each block id other than air it holds, with its cells in the body; empty under
    /// [`Detail::Counts`].
    pub(super) blocks: Vec<(String, Vec<Cell>)>,
}

/// One array of a body, laid where it stands.
#[derive(Debug, Clone)]
struct LaidPart {
    /// Where the array's `(0, 0, 0)` cell stands in the body.
    shift: Cell,
    dims: Dims,
    /// Each cell's state as an index into [`BuiltBody::palette`], `0` being air.
    slots: Vec<u32>,
    /// What wrote each cell.
    by: Vec<By>,
}

impl LaidPart {
    /// The offset of the body's `cell` into this part's cells, `None` outside it.
    fn index(&self, (x, y, z): Cell) -> Option<usize> {
        let (sx, sy, sz) = self.shift;
        self.dims
            .index(x.checked_sub(sx)?, y.checked_sub(sy)?, z.checked_sub(sz)?)
    }

    /// The box the part fills in the body.
    fn extent(&self) -> Extent {
        extent(self.shift, self.dims)
    }
}

/// One body of the build: a `struct`'s array, or a `site`'s arrays laid where they stand.
#[derive(Debug, Clone)]
pub(super) struct BuiltBody {
    pub(super) name: String,
    /// The states the parts' cells index, `0` being air.
    palette: Vec<BlockState>,
    /// The arrays, in the order they are laid.
    parts: Vec<LaidPart>,
    pub(super) members: Vec<Owned>,
}

impl BuiltBody {
    fn empty(name: String) -> Self {
        Self {
            name,
            palette: vec![BlockState::air()],
            parts: Vec::new(),
            members: Vec::new(),
        }
    }

    /// The state at `cell` as an index into the palette, `0` being air, and what wrote the cell:
    /// the last part laid with a block there, else the last one owning the cell.
    fn at(&self, cell: Cell) -> (u32, By) {
        let mut owner = By::Nothing;
        for part in self.parts.iter().rev() {
            let Some(i) = part.index(cell) else {
                continue;
            };
            if part.slots[i] != 0 {
                return (part.slots[i], part.by[i]);
            }
            if owner == By::Nothing {
                owner = part.by[i];
            }
        }
        (0, owner)
    }
}

/// One array of a body, and where its `(0, 0, 0)` cell stands.
struct Part<'a> {
    key: &'a str,
    array: &'a BlockArray,
    origin: (i64, i64, i64),
    walkway: bool,
}

/// The bodies `built` holds, in the order their first array is listed: each `struct`, and each
/// `site` with its placements and walkways laid at their origins.
pub(super) fn built_bodies(built: &BlockArrayIr, detail: Detail) -> Vec<BuiltBody> {
    let mut order: IndexMap<String, Vec<Part<'_>>> = IndexMap::new();
    for (key, array) in &built.structures {
        let at = |(x, y, z): (i32, i32, i32)| (i64::from(x), i64::from(y), i64::from(z));
        let (name, part) = if let Some(placement) = built.placements.get(key) {
            let origin = at(placement.origin);
            (placement.site.to_string(), (origin, false))
        } else if let Some(walkway) = built.walkways.get(key.as_str()) {
            (walkway.site.to_string(), (at(walkway.origin), true))
        } else if let Some(name) = key.strip_prefix("struct::") {
            (name.to_owned(), ((0, 0, 0), false))
        } else {
            continue;
        };
        order.entry(name).or_default().push(Part {
            key,
            array,
            origin: part.0,
            walkway: part.1,
        });
    }
    order
        .into_iter()
        .map(|(name, parts)| lay(name, &parts, built, detail))
        .collect()
}

/// A body of `parts`, shifted so its lowest corner is `(0, 0, 0)`; a later part's block replaces
/// an earlier one's, and its air replaces nothing.
fn lay(name: String, parts: &[Part<'_>], built: &BlockArrayIr, detail: Detail) -> BuiltBody {
    let low = |axis: fn(&Part<'_>) -> i64| parts.iter().map(axis).min().unwrap_or(0);
    let low = (
        low(|p| p.origin.0),
        low(|p| p.origin.1),
        low(|p| p.origin.2),
    );
    // Origins are `i32`, so one less the lowest fits a `u32`.
    let offset = |v: i64| u32::try_from(v).expect("an origin less the lowest one fits a u32");
    let mut body = BuiltBody::empty(name);
    let mut slots: HashMap<String, u32> = HashMap::new();
    for part in parts {
        let shift = (
            offset(part.origin.0 - low.0),
            offset(part.origin.1 - low.1),
            offset(part.origin.2 - low.2),
        );
        let mut remap = Vec::with_capacity(part.array.palette.entries.len());
        for state in &part.array.palette.entries {
            if is_air(&state.id) {
                remap.push(0);
                continue;
            }
            let next = u32::try_from(body.palette.len()).unwrap_or(u32::MAX);
            let slot = *slots.entry(spelled(state)).or_insert_with(|| {
                body.palette.push(state.clone());
                next
            });
            remap.push(slot);
        }
        let dims = part.array.dims;
        let unowned = if part.walkway {
            By::Walkway
        } else {
            By::Nothing
        };
        let mut by = vec![unowned; dims.volume()];
        if !part.walkway {
            let first = body.members.len();
            for (m, member) in built
                .members
                .get(part.key)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let owned = owned(member, shift, dims, detail, first + m, &mut by);
                body.members.push(owned);
            }
        }
        let slots = part
            .array
            .voxels
            .iter()
            .map(|slot| remap[usize::from(slot.0)])
            .collect();
        body.parts.push(LaidPart {
            shift,
            dims,
            slots,
            by,
        });
    }
    body
}

/// `member` as a body holds it, its cells moved by `shift`; each of its cells of `dims` is marked
/// in `by` as the member at `index`'s.
fn owned(
    member: &ResolvedMember,
    shift: Cell,
    dims: Dims,
    detail: Detail,
    index: usize,
    by: &mut [By],
) -> Owned {
    let mut blocks: Vec<(String, Vec<Cell>)> = Vec::new();
    for resolved in &member.resolved_state {
        let keep = detail == Detail::Cells && !is_air(&resolved.state.id);
        let mut moved = Vec::new();
        for cell in resolved.cells() {
            if let Some(i) = dims.index(cell.0, cell.1, cell.2) {
                by[i] = By::Member(index);
            }
            if keep {
                moved.push(shifted(cell, shift));
            }
        }
        if !keep {
            continue;
        }
        match blocks.iter_mut().find(|(id, _)| *id == resolved.state.id) {
            Some((_, cells)) => cells.extend(moved),
            None => blocks.push((resolved.state.id.clone(), moved)),
        }
    }
    Owned {
        role: member.role.clone(),
        id: member.id.clone(),
        address: member.address.clone(),
        span: member.span.clone(),
        raw: member.is_raw(),
        declared_low: shifted(member.geometry.declared_bbox.min(), shift),
        blocks,
    }
}

/// `cell` moved by `shift`.
fn shifted((x, y, z): Cell, (dx, dy, dz): Cell) -> Cell {
    (
        x.saturating_add(dx),
        y.saturating_add(dy),
        z.saturating_add(dz),
    )
}

/// One body of the structure: a grid, or several laid where they stand in the file.
pub(super) struct StructureBody<'a> {
    name: String,
    /// Each grid with where its `(0, 0, 0)` cell stands in the body, in the order they are laid.
    parts: Vec<(Cell, &'a GridUnit)>,
}

impl<'a> StructureBody<'a> {
    fn one(unit: &'a GridUnit) -> Self {
        Self {
            name: unit.name.clone(),
            parts: vec![((0, 0, 0), unit)],
        }
    }

    /// `units` at their corners, shifted so the lowest is `(0, 0, 0)`, named after `stem` as an
    /// import names the body of a file of one structure.
    fn laid(stem: &str, units: &'a [GridUnit]) -> Self {
        let low = |axis: fn(&GridUnit) -> i32| units.iter().map(axis).min().unwrap_or(0);
        let low = (
            low(|unit| unit.corner.0),
            low(|unit| unit.corner.1),
            low(|unit| unit.corner.2),
        );
        // Corners are `i32`, so one less the lowest fits a `u32`.
        let offset = |v: i32, low: i32| {
            u32::try_from(i64::from(v) - i64::from(low))
                .expect("a corner less the lowest fits a u32")
        };
        let parts: Vec<(Cell, &GridUnit)> = units
            .iter()
            .map(|unit| {
                let (x, y, z) = unit.corner;
                ((offset(x, low.0), offset(y, low.1), offset(z, low.2)), unit)
            })
            .collect();
        let high = |axis: fn(&Extent) -> u32| {
            parts
                .iter()
                .map(|&(shift, unit)| axis(&extent(shift, unit.grid.dims())))
                .max()
        };
        let dims = Dims {
            x: high(|(_, high)| high.0).unwrap_or(0),
            y: high(|(_, high)| high.1).unwrap_or(0),
            z: high(|(_, high)| high.2).unwrap_or(0),
        };
        Self {
            name: unit_names([(stem, dims)]).remove(0),
            parts,
        }
    }

    /// The block at `cell`, with the part and palette slot it is read from: the last part laid
    /// with one there.
    fn at(&self, (x, y, z): Cell) -> Option<((usize, u16), &'a GridEntry)> {
        self.parts
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, &((sx, sy, sz), unit))| {
                let grid = &unit.grid;
                let index = grid.dims().index(
                    x.checked_sub(sx)?,
                    y.checked_sub(sy)?,
                    z.checked_sub(sz)?,
                )?;
                let slot = *grid.voxels().get(index)?;
                let entry = grid.palette().get(usize::from(slot.0))?;
                (!is_air(&entry.state.id)).then_some(((i, slot.0), entry))
            })
    }
}

/// The box an array of `dims` fills from `shift`.
fn extent(shift: Cell, Dims { x, y, z }: Dims) -> Extent {
    let (sx, sy, sz) = shift;
    (
        shift,
        (
            sx.saturating_add(x),
            sy.saturating_add(y),
            sz.saturating_add(z),
        ),
    )
}

/// Each body of `structure` with the body of the build it is compared with: the whole structure
/// with the build's one body, its regions laid as one, else by name, each unpaired body on its
/// own.
pub(super) fn pair(
    structure: StructureBodies<'_>,
    bodies: Vec<BuiltBody>,
) -> Vec<(Option<StructureBody<'_>>, Option<BuiltBody>)> {
    let units = structure.units;
    if bodies.len() == 1 && !units.is_empty() {
        let whole = match units {
            [unit] => StructureBody::one(unit),
            _ => StructureBody::laid(structure.stem, units),
        };
        return vec![(Some(whole), bodies.into_iter().next())];
    }
    let mut left: Vec<Option<BuiltBody>> = bodies.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for unit in units {
        let named = left
            .iter_mut()
            .find(|body| body.as_ref().is_some_and(|body| body.name == unit.name))
            .and_then(Option::take);
        out.push((Some(StructureBody::one(unit)), named));
    }
    out.extend(left.into_iter().flatten().map(|body| (None, Some(body))));
    out
}

/// The cells of `unit` and `body` compared over the boxes their arrays fill, from `(0, 0, 0)`,
/// keeping what `detail` asks for.
pub(super) fn compare_pair(
    unit: Option<&StructureBody<'_>>,
    body: Option<BuiltBody>,
    detail: Detail,
) -> BodyComparison {
    let structure = unit.map(|unit| unit.name.clone());
    let source = body.as_ref().map(|body| body.name.clone());
    let built = body.unwrap_or_else(|| BuiltBody::empty(String::new()));
    let mut boxes: Vec<Extent> = built.parts.iter().map(LaidPart::extent).collect();
    if let Some(unit) = unit {
        boxes.extend(
            unit.parts
                .iter()
                .map(|&(shift, unit)| extent(shift, unit.grid.dims())),
        );
    }
    let expected_at = |cell: Cell| unit?.at(cell);
    let mut counts = Counts::default();
    let mut differences = Vec::new();
    let mut states: HashMap<((usize, u16), u32), Vec<StateDifference>> = HashMap::new();
    each_cell(&boxes, |cell| {
        let expected = expected_at(cell);
        let (slot, by) = built.at(cell);
        let block = (slot != 0).then(|| &built.palette[slot as usize]);
        counts.structure_blocks += u64::from(expected.is_some());
        if block.is_some() {
            counts.built_blocks += 1;
            let raw = matches!(by, By::Member(m) if built.members[m].raw);
            counts.residual += u64::from(raw);
        }
        let mut differing: &[StateDifference] = &[];
        let kind = match (expected, block) {
            (None, None) => return,
            (Some(_), None) => RegionKind::Missing,
            (None, Some(_)) => RegionKind::Extra,
            (Some((_, expected)), Some(block)) if expected.state.id != block.id => {
                counts.both_blocks += 1;
                RegionKind::Replaced
            }
            (Some((expected_slot, expected)), Some(block)) => {
                counts.both_blocks += 1;
                counts.matched += 1;
                differing = states
                    .entry((expected_slot, slot))
                    .or_insert_with(|| differing_states(expected, block));
                if differing.is_empty() {
                    counts.states_matched += 1;
                    return;
                }
                RegionKind::State
            }
        };
        if detail == Detail::Cells {
            differences.push(Difference {
                cell,
                kind,
                expected: expected.map(|(_, entry)| entry.state.clone()),
                built: block.cloned(),
                states: differing.to_vec(),
                by,
            });
        }
    });
    BodyComparison {
        structure,
        source,
        counts,
        built,
        differences,
    }
}

/// Visit each cell that lies in one of `boxes` once, in `(y, z, x)` order.
fn each_cell(boxes: &[Extent], mut visit: impl FnMut(Cell)) {
    for (low_y, high_y) in merged(boxes.iter().map(|(low, high)| (low.1, high.1))) {
        for y in low_y..high_y {
            let row: Vec<&Extent> = boxes
                .iter()
                .filter(|(low, high)| (low.1..high.1).contains(&y))
                .collect();
            for (low_z, high_z) in merged(row.iter().map(|(low, high)| (low.2, high.2))) {
                for z in low_z..high_z {
                    let runs = row
                        .iter()
                        .filter(|(low, high)| (low.2..high.2).contains(&z))
                        .map(|(low, high)| (low.0, high.0));
                    for (low_x, high_x) in merged(runs) {
                        for x in low_x..high_x {
                            visit((x, y, z));
                        }
                    }
                }
            }
        }
    }
}

/// The spans `start..end`, the empty ones left out, joined where they meet or overlap, in order.
fn merged(spans: impl Iterator<Item = (u32, u32)>) -> Vec<(u32, u32)> {
    let mut sorted: Vec<(u32, u32)> = spans.filter(|(start, end)| start < end).collect();
    sorted.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (start, end) in sorted {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Each property `built` holds otherwise than `expected`, as the target builds them: a property
/// one side leaves out reads as the target's default where `expected` gives one, `waterlogged` as
/// `false`, and one the build leaves out with no default is not compared.
fn differing_states(expected: &GridEntry, built: &BlockState) -> Vec<StateDifference> {
    let mut compared = GridEntry {
        state: BlockState::bare(expected.state.id.clone()),
        defaults: expected.defaults.clone(),
        unread: expected.unread.clone(),
    };
    compared
        .defaults
        .entry(WATERLOGGED.to_owned())
        .or_insert_with(|| "false".to_owned());
    for (key, value) in &expected.state.properties {
        let left_out = !built.properties.contains_key(key);
        if !left_out || compared.defaults.contains_key(key) {
            compared.state.properties.insert(key.clone(), value.clone());
        }
    }
    compared
        .differing_states(built)
        .into_iter()
        .map(|(key, structure, built)| StateDifference {
            key,
            structure,
            built,
        })
        .collect()
}
