//! The bodies a build holds, each paired with a body of the structure, and the cells of a pair
//! compared.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::block_array::{BlockArray, BlockArrayIr, BlockState, Dims, ResolvedMember};
use crate::error::Span;
use crate::intent::MemberAddress;
use crate::transliterate::{GridEntry, GridUnit, spelled};

use super::{BodyComparison, Counts, Difference, RegionKind, StateDifference, is_air};

/// The state lowering leaves out where no water overlaps a block, which the game then places dry.
const WATERLOGGED: &str = "waterlogged";

/// A cell, `(x, y, z)`.
pub(super) type Cell = (u32, u32, u32);

/// What last wrote a cell of a built body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum By {
    /// Nothing: the cell is air no member wrote.
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
    /// Each block id other than air it holds, with its cells in the body.
    pub(super) blocks: Vec<(String, Vec<Cell>)>,
}

/// One body of the build: a `struct`'s array, or a `site`'s arrays laid where they stand.
#[derive(Debug, Clone)]
pub(super) struct BuiltBody {
    pub(super) name: String,
    pub(super) dims: Dims,
    /// The states the cells index, `0` being air.
    palette: Vec<BlockState>,
    cells: Vec<u32>,
    by: Vec<By>,
    pub(super) members: Vec<Owned>,
}

impl BuiltBody {
    fn empty(name: String) -> Self {
        Self {
            name,
            dims: Dims { x: 0, y: 0, z: 0 },
            palette: vec![BlockState::air()],
            cells: Vec::new(),
            by: Vec::new(),
            members: Vec::new(),
        }
    }

    /// The block at `cell` other than air, and what wrote the cell.
    pub(super) fn at(&self, cell: Cell) -> (Option<&BlockState>, By) {
        match self.dims.index(cell.0, cell.1, cell.2) {
            Some(i) => {
                let slot = self.cells[i] as usize;
                ((slot != 0).then(|| &self.palette[slot]), self.by[i])
            }
            None => (None, By::Nothing),
        }
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
pub(super) fn built_bodies(built: &BlockArrayIr) -> Vec<BuiltBody> {
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
        .map(|(name, parts)| lay(name, &parts, built))
        .collect()
}

/// A body of `parts`, shifted so its lowest corner is `(0, 0, 0)`; a later part's block replaces
/// an earlier one's, and its air replaces nothing.
fn lay(name: String, parts: &[Part<'_>], built: &BlockArrayIr) -> BuiltBody {
    let low = |axis: fn(&Part<'_>) -> i64| parts.iter().map(axis).min().unwrap_or(0);
    let low = (
        low(|p| p.origin.0),
        low(|p| p.origin.1),
        low(|p| p.origin.2),
    );
    let high = |axis: fn(&Part<'_>) -> i64| parts.iter().map(axis).max().unwrap_or(0);
    let extent = |v: i64| u32::try_from(v).unwrap_or(0);
    let dims = Dims {
        x: extent(high(|p| p.origin.0 + i64::from(p.array.dims.x)) - low.0),
        y: extent(high(|p| p.origin.1 + i64::from(p.array.dims.y)) - low.1),
        z: extent(high(|p| p.origin.2 + i64::from(p.array.dims.z)) - low.2),
    };
    let mut body = BuiltBody {
        cells: vec![0; dims.volume()],
        by: vec![By::Nothing; dims.volume()],
        dims,
        ..BuiltBody::empty(name)
    };
    let mut slots: HashMap<String, u32> = HashMap::new();
    for part in parts {
        let shift = (
            extent(part.origin.0 - low.0),
            extent(part.origin.1 - low.1),
            extent(part.origin.2 - low.2),
        );
        let first = body.members.len();
        let mut owner = vec![None; part.array.dims.volume()];
        if !part.walkway {
            for (m, member) in built
                .members
                .get(part.key)
                .into_iter()
                .flatten()
                .enumerate()
            {
                body.members
                    .push(owned(member, shift, &mut owner, part.array.dims, first + m));
            }
        }
        for (i, slot) in part.array.voxels.iter().enumerate() {
            let Some((x, y, z)) = part.array.dims.position(i) else {
                continue;
            };
            let at = dims
                .index(x + shift.0, y + shift.1, z + shift.2)
                .expect("a part lies inside the body laid around it");
            let state = &part.array.palette.entries[usize::from(slot.0)];
            let by = match owner[i] {
                Some(member) => By::Member(member),
                None if part.walkway => By::Walkway,
                None => By::Nothing,
            };
            if !is_air(&state.id) {
                let next = u32::try_from(body.palette.len()).unwrap_or(u32::MAX);
                let slot = *slots.entry(spelled(state)).or_insert_with(|| {
                    body.palette.push(state.clone());
                    next
                });
                body.cells[at] = slot;
                body.by[at] = by;
            } else if body.cells[at] == 0 && by != By::Nothing {
                body.by[at] = by;
            }
        }
    }
    body
}

/// `member` as a body holds it, its cells moved by `shift`, each of its cells of `dims` marked in
/// `owner` as `index`'s.
fn owned(
    member: &ResolvedMember,
    shift: Cell,
    owner: &mut [Option<usize>],
    dims: Dims,
    index: usize,
) -> Owned {
    let mut blocks: Vec<(String, Vec<Cell>)> = Vec::new();
    for resolved in &member.resolved_state {
        let mut moved = Vec::new();
        for (x, y, z) in resolved.cells() {
            if let Some(i) = dims.index(x, y, z) {
                owner[i] = Some(index);
            }
            moved.push((x + shift.0, y + shift.1, z + shift.2));
        }
        if is_air(&resolved.state.id) {
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
        raw: matches!(member.role.as_str(), "raw_block" | "raw_fill"),
        blocks,
    }
}

/// Each unit with the body it is compared with: the one with the one when each side holds one,
/// else by name, each unpaired unit and body on its own.
pub(super) fn pair(
    units: &[GridUnit],
    bodies: Vec<BuiltBody>,
) -> Vec<(Option<&GridUnit>, Option<BuiltBody>)> {
    if units.len() == 1 && bodies.len() == 1 {
        return vec![(units.first(), bodies.into_iter().next())];
    }
    let mut left: Vec<Option<BuiltBody>> = bodies.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for unit in units {
        let named = left
            .iter_mut()
            .find(|body| body.as_ref().is_some_and(|body| body.name == unit.name))
            .and_then(Option::take);
        out.push((Some(unit), named));
    }
    out.extend(left.into_iter().flatten().map(|body| (None, Some(body))));
    out
}

/// The cells of `unit` and `body` compared over the box both lie in, from `(0, 0, 0)`.
pub(super) fn compare_pair(unit: Option<&GridUnit>, body: Option<BuiltBody>) -> BodyComparison {
    let structure = unit.map(|unit| unit.name.clone());
    let source = body.as_ref().map(|body| body.name.clone());
    let built = body.unwrap_or_else(|| BuiltBody::empty(String::new()));
    let grid_dims = unit.map_or(Dims { x: 0, y: 0, z: 0 }, |unit| unit.grid.dims);
    let extent = Dims {
        x: grid_dims.x.max(built.dims.x),
        y: grid_dims.y.max(built.dims.y),
        z: grid_dims.z.max(built.dims.z),
    };
    let expected_at = |(x, y, z): Cell| -> Option<&GridEntry> {
        let grid = &unit?.grid;
        let slot = grid.voxels.get(grid.dims.index(x, y, z)?)?;
        grid.palette
            .get(usize::from(slot.0))
            .filter(|entry| !is_air(&entry.state.id))
    };
    let mut counts = Counts::default();
    let mut differences = Vec::new();
    for y in 0..extent.y {
        for z in 0..extent.z {
            for x in 0..extent.x {
                let cell = (x, y, z);
                let expected = expected_at(cell);
                let (block, by) = built.at(cell);
                counts.structure_blocks += u64::from(expected.is_some());
                if block.is_some() {
                    counts.built_blocks += 1;
                    let raw = matches!(by, By::Member(m) if built.members[m].raw);
                    counts.residual += u64::from(raw);
                }
                let kind = match (expected, block) {
                    (None, None) => continue,
                    (Some(_), None) => RegionKind::Missing,
                    (None, Some(_)) => RegionKind::Extra,
                    (Some(expected), Some(block)) => {
                        counts.both_blocks += 1;
                        if expected.state.id == block.id {
                            counts.matched += 1;
                            let states = differing_states(expected, block);
                            if states.is_empty() {
                                counts.states_matched += 1;
                                continue;
                            }
                            differences.push(Difference {
                                cell,
                                kind: RegionKind::State,
                                expected: Some(expected.state.clone()),
                                built: Some(block.clone()),
                                states,
                                by,
                            });
                            continue;
                        }
                        RegionKind::Replaced
                    }
                };
                differences.push(Difference {
                    cell,
                    kind,
                    expected: expected.map(|entry| entry.state.clone()),
                    built: block.cloned(),
                    states: Vec::new(),
                    by,
                });
            }
        }
    }
    BodyComparison {
        structure,
        source,
        counts,
        built,
        differences,
    }
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
