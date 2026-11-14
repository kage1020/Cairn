//! What each cell of a unit is written as, and the `.crn` text that writes it.

use std::fmt::Write as _;

use crate::CAIRN_VERSION;
use crate::block_array::{BlockArrayIr, BlockState, Dims, TargetRegistry};
use crate::resolve::place_scope_key;

use super::boxes::{Cell, Folded, cut, fold};
use super::{CHUNK_FOOTPRINT, GridEntry, GridUnit, Stamp, TransliterateError, WrittenBody};

/// The theme a chunked unit's `place` rows name; it binds nothing, as no raw member reads a slot.
const CHUNK_THEME: &str = "imported";

/// How one grid entry is written: with the connection states the compile derives left out, and
/// with every one of them.
#[derive(Debug, Clone)]
struct Spellings {
    derived: BlockState,
    written: BlockState,
}

/// One body of a unit: the whole grid, or one chunk of it.
#[derive(Debug, Clone)]
struct Chunk {
    /// The `def`'s name, or the unit's for a `struct`.
    name: String,
    /// Where its `(0, 0, 0)` cell lies in the unit's grid.
    origin: Cell,
    dims: Dims,
    /// Per cell, whether its line writes its connection states out.
    written: Vec<bool>,
}

/// A unit, with what each of its cells is written as so far.
#[derive(Debug)]
pub(super) struct UnitPlan<'a> {
    unit: &'a GridUnit,
    /// Per grid entry, `None` for air.
    spellings: Vec<Option<Spellings>>,
    chunks: Vec<Chunk>,
}

impl<'a> UnitPlan<'a> {
    /// The plan that first writes every cell with its connection states derived.
    pub(super) fn new(
        unit: &'a GridUnit,
        registry: &dyn TargetRegistry,
    ) -> Result<Self, TransliterateError> {
        let grid = &unit.grid;
        let malformed = |reason: String| TransliterateError::Malformed {
            unit: unit.name.clone(),
            reason,
        };
        if grid.dims.x == 0 || grid.dims.y == 0 || grid.dims.z == 0 {
            return Err(malformed(format!(
                "its extents {}x{}x{} hold no cell",
                grid.dims.x, grid.dims.y, grid.dims.z
            )));
        }
        if grid.voxels.len() != grid.dims.volume() {
            return Err(malformed(format!(
                "it lists {} voxels for {}x{}x{} cells",
                grid.voxels.len(),
                grid.dims.x,
                grid.dims.y,
                grid.dims.z
            )));
        }
        if let Some(slot) = grid
            .voxels
            .iter()
            .find(|slot| usize::from(slot.0) >= grid.palette.len())
        {
            return Err(malformed(format!(
                "a voxel names palette entry {} of {}",
                slot.0,
                grid.palette.len()
            )));
        }
        let spellings: Vec<Option<Spellings>> = grid
            .palette
            .iter()
            .map(|entry| spellings(entry, registry))
            .collect();
        if grid
            .voxels
            .iter()
            .all(|slot| spellings[usize::from(slot.0)].is_none())
        {
            return Err(TransliterateError::Empty {
                unit: unit.name.clone(),
            });
        }
        Ok(Self {
            unit,
            spellings,
            chunks: chunks(&unit.name, grid.dims),
        })
    }

    /// Whether the unit is written as a `site` of chunks.
    fn chunked(&self) -> bool {
        self.chunks.len() > 1
    }

    /// How many chunks lie along `z`.
    fn rows(&self) -> usize {
        self.chunks
            .iter()
            .filter(|chunk| chunk.origin.0 == 0)
            .count()
    }

    /// The grid entry at `cell` of `chunk`.
    fn entry_at(&self, chunk: &Chunk, cell: Cell) -> usize {
        let grid = &self.unit.grid;
        let (x, y, z) = (
            chunk.origin.0 + cell.0,
            chunk.origin.1 + cell.1,
            chunk.origin.2 + cell.2,
        );
        let index = grid
            .dims
            .index(x, y, z)
            .expect("a chunk lies inside its grid");
        usize::from(grid.voxels[index].0)
    }

    /// What each cell of `chunk` is written as: `2 * entry`, or that plus one with its connection
    /// states written where they differ from the derived spelling; `None` for air.
    fn slots(&self, chunk: &Chunk) -> Vec<Option<u32>> {
        (0..chunk.dims.volume())
            .map(|i| {
                let cell = chunk.dims.position(i).expect("a chunk's index");
                let entry = self.entry_at(chunk, cell);
                let spelling = self.spellings[entry].as_ref()?;
                let entry = u32::try_from(entry).expect("a palette is indexed by u16");
                let written = chunk.written[i] && spelling.written != spelling.derived;
                Some(2 * entry + u32::from(written))
            })
            .collect()
    }

    /// The state `slot` writes.
    fn state(&self, slot: u32) -> &BlockState {
        let spelling = self.spellings[slot as usize / 2]
            .as_ref()
            .expect("an air cell has no slot");
        if slot % 2 == 1 {
            &spelling.written
        } else {
            &spelling.derived
        }
    }

    /// Each cell the compile built as another block than the grid holds, with its chunk and the
    /// block built.
    ///
    /// # Errors
    ///
    /// [`TransliterateError::Malformed`] when the compile built no body of the size written.
    pub(super) fn unmatched(
        &self,
        ir: &BlockArrayIr,
    ) -> Result<Vec<(usize, Cell, BlockState)>, TransliterateError> {
        let mut out = Vec::new();
        for (c, chunk) in self.chunks.iter().enumerate() {
            let key = self.scope_key(chunk);
            let array = ir
                .structures
                .get(&key)
                .filter(|array| array.dims == chunk.dims)
                .ok_or_else(|| TransliterateError::Malformed {
                    unit: self.unit.name.clone(),
                    reason: format!(
                        "compiling its source builds no `{key}` of {}x{}x{} cells",
                        chunk.dims.x, chunk.dims.y, chunk.dims.z
                    ),
                })?;
            if self.chunked() {
                let expected = placed_origin(self.rows(), chunk.origin);
                let placed = ir.placements.get(&key).map(|placement| placement.origin);
                if placed != Some(expected) {
                    return Err(TransliterateError::Malformed {
                        unit: self.unit.name.clone(),
                        reason: format!(
                            "compiling its source places `{}` at {placed:?}, not at {expected:?}",
                            chunk.name
                        ),
                    });
                }
            }
            for (i, slot) in array.voxels.iter().enumerate() {
                let cell = chunk.dims.position(i).expect("a chunk's index");
                let built = &array.palette.entries[usize::from(slot.0)];
                let entry = &self.unit.grid.palette[self.entry_at(chunk, cell)];
                if !entry.is_built_as(built) {
                    out.push((c, cell, built.clone()));
                }
            }
        }
        Ok(out)
    }

    /// The finding that a cell is built otherwise with every state written.
    pub(super) fn mismatch(
        &self,
        chunk: usize,
        cell: Cell,
        built: &BlockState,
    ) -> TransliterateError {
        let chunk = &self.chunks[chunk];
        let entry = &self.unit.grid.palette[self.entry_at(chunk, cell)];
        TransliterateError::Unmatched {
            unit: self.unit.name.clone(),
            cell: (
                chunk.origin.0 + cell.0,
                chunk.origin.1 + cell.1,
                chunk.origin.2 + cell.2,
            ),
            expected: spelled(&entry.state),
            built: spelled(built),
        }
    }

    /// Write the connection states of each of `cells`.
    pub(super) fn write_connections(&mut self, cells: impl Iterator<Item = (usize, Cell)>) {
        for (chunk, cell) in cells {
            let chunk = &mut self.chunks[chunk];
            let index = chunk
                .dims
                .index(cell.0, cell.1, cell.2)
                .expect("a chunk's cell");
            chunk.written[index] = true;
        }
    }

    /// Write the connection states of every cell.
    pub(super) fn write_every_connection(&mut self) {
        for chunk in &mut self.chunks {
            chunk.written.fill(true);
        }
    }

    /// The key the compile files `chunk`'s array under.
    fn scope_key(&self, chunk: &Chunk) -> String {
        if self.chunked() {
            place_scope_key(&self.unit.name, &chunk.name)
        } else {
            format!("struct::{}", self.unit.name)
        }
    }
}

/// How `entry` is written, or `None` for air: every state it says but those the target reads the
/// same at any value and those at the target's default, which no rule but the connections sets,
/// and the connection states left out or written.
fn spellings(entry: &GridEntry, registry: &dyn TargetRegistry) -> Option<Spellings> {
    if entry.state.id == BlockState::AIR_ID {
        return None;
    }
    let connections = registry
        .joins(&entry.state)
        .and_then(|joins| joins.kind)
        .map_or(&[][..], |kind| kind.connection_keys());
    let mut written = BlockState::bare(entry.state.id.clone());
    for (key, value) in &entry.state.properties {
        let connection = connections.contains(&key.as_str());
        let unread = entry.unread.contains(key);
        let defaulted = entry.defaults.get(key) == Some(value);
        if unread || (defaulted && !connection) {
            continue;
        }
        written.properties.insert(key.clone(), value.clone());
    }
    written.properties.sort_keys();
    let mut derived = written.clone();
    derived
        .properties
        .retain(|key, _| !connections.contains(&key.as_str()));
    Some(Spellings { derived, written })
}

/// The bodies `name`'s grid of `dims` is written as: itself, or columns of at most
/// [`CHUNK_FOOTPRINT`] each way, the whole height.
fn chunks(name: &str, dims: Dims) -> Vec<Chunk> {
    if dims.x <= CHUNK_FOOTPRINT && dims.z <= CHUNK_FOOTPRINT {
        return vec![Chunk {
            name: name.to_owned(),
            origin: (0, 0, 0),
            dims,
            written: vec![false; dims.volume()],
        }];
    }
    let mut out = Vec::new();
    for i in 0..dims.x.div_ceil(CHUNK_FOOTPRINT) {
        for j in 0..dims.z.div_ceil(CHUNK_FOOTPRINT) {
            let origin = (i * CHUNK_FOOTPRINT, 0, j * CHUNK_FOOTPRINT);
            let dims = Dims {
                x: CHUNK_FOOTPRINT.min(dims.x - origin.0),
                y: dims.y,
                z: CHUNK_FOOTPRINT.min(dims.z - origin.2),
            };
            out.push(Chunk {
                name: format!("{name}_x{i}_z{j}"),
                origin,
                dims,
                written: vec![false; dims.volume()],
            });
        }
    }
    out
}

/// `state` as a `mat=` writes it: the id, and its properties sorted by name in brackets.
pub(super) fn spelled(state: &BlockState) -> String {
    let (id, properties) = state.canonical_key();
    if properties.is_empty() {
        return id.to_owned();
    }
    let joined: Vec<String> = properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    format!("{id}[{}]", joined.join(","))
}

/// The `.crn` source writing every unit of `plans` for `stamp`, and how each was written.
pub(super) fn source(plans: &[UnitPlan<'_>], stamp: &Stamp) -> (String, Vec<WrittenBody>) {
    let mut out = String::new();
    for line in &stamp.comment {
        comment(&mut out, line);
    }
    let edition = stamp.edition.as_str();
    let mc = &stamp.mc_version;
    let _ = writeln!(out, "@cairn {CAIRN_VERSION}");
    let _ = writeln!(out, "@requires {edition} version>={mc}");
    let _ = writeln!(out, "@intended_targets [\"{mc}\"]");
    if plans.iter().any(UnitPlan::chunked) {
        let _ = write!(out, "\ntheme {CHUNK_THEME}\n");
    }
    let mut bodies = Vec::with_capacity(plans.len());
    for plan in plans {
        out.push('\n');
        for line in &plan.unit.comment {
            comment(&mut out, line);
        }
        let mut lines = 0;
        let keyword = if plan.chunked() { "def" } else { "struct" };
        for (c, chunk) in plan.chunks.iter().enumerate() {
            if c > 0 {
                out.push('\n');
            }
            let _ = writeln!(
                out,
                "{keyword} {} size={}x{}",
                chunk.name, chunk.dims.x, chunk.dims.z
            );
            lines += members(&mut out, plan, chunk);
        }
        if plan.chunked() {
            out.push('\n');
            site(&mut out, plan);
        }
        bodies.push(WrittenBody {
            name: plan.unit.name.clone(),
            chunks: if plan.chunked() {
                plan.chunks.iter().map(|chunk| chunk.name.clone()).collect()
            } else {
                Vec::new()
            },
            lines,
        });
    }
    (out, bodies)
}

/// A `#` comment line holding `text`.
fn comment(out: &mut String, text: &str) {
    let _ = writeln!(out, "# {}", text.replace(['\n', '\r'], " "));
}

/// The member lines of one body, and how many were written.
fn members(out: &mut String, plan: &UnitPlan<'_>, chunk: &Chunk) -> usize {
    let folded = fold(cut(chunk.dims, &plan.slots(chunk)));
    let mut lines = 0;
    let mut done = vec![false; folded.len()];
    for (i, item) in folded.iter().enumerate() {
        if done[i] {
            continue;
        }
        if item.count == 1 {
            raw_line(out, "  ", plan, item);
            lines += 1;
            continue;
        }
        let (dx, dy, dz) = item.step;
        let _ = writeln!(out, "  raw_repeat count={} step={dx},{dy},{dz}", item.count);
        lines += 1;
        for (j, other) in folded.iter().enumerate().skip(i) {
            if !done[j] && other.count == item.count && other.step == item.step {
                raw_line(out, "    ", plan, other);
                lines += 1;
                done[j] = true;
            }
        }
    }
    let top = chunk.dims.y - 1;
    if folded.iter().all(|item| item.top() < top) {
        let _ = writeln!(
            out,
            "  raw_block mat={} at=0,{top},0 origin=imported",
            BlockState::AIR_ID
        );
        lines += 1;
    }
    lines
}

/// The `raw_block` or `raw_fill` writing `item`'s first copy.
fn raw_line(out: &mut String, indent: &str, plan: &UnitPlan<'_>, item: &Folded) {
    let cuboid = item.cuboid;
    let mat = spelled(plan.state(cuboid.slot));
    let (x, y, z) = cuboid.low;
    if cuboid.size == (1, 1, 1) {
        let _ = writeln!(
            out,
            "{indent}raw_block mat={mat} at={x},{y},{z} origin=imported"
        );
    } else {
        let (hx, hy, hz) = cuboid.high();
        let _ = writeln!(
            out,
            "{indent}raw_fill mat={mat} from={x},{y},{z} to={hx},{hy},{hz} origin=imported"
        );
    }
}

/// The `site` placing a chunked unit's chunks edge to edge: the first column from its last chunk
/// northward, then each row eastward from it.
fn site(out: &mut String, plan: &UnitPlan<'_>) {
    let columns = plan
        .chunks
        .iter()
        .map(|chunk| chunk.origin.0)
        .max()
        .map_or(1, |x| x / CHUNK_FOOTPRINT + 1);
    let rows = plan.rows();
    let name = |i: usize, j: usize| plan.chunks[i * rows + j].name.as_str();
    let _ = writeln!(out, "site {}", plan.unit.name);
    let place = |out: &mut String, id: &str, origin: String| {
        let _ = writeln!(out, "  place id={id} use={id} theme={CHUNK_THEME} {origin}");
    };
    place(out, name(0, rows - 1), "at=origin".to_owned());
    for j in (0..rows - 1).rev() {
        place(
            out,
            name(0, j),
            format!("north_of={} gap=0", name(0, j + 1)),
        );
    }
    for j in 0..rows {
        for i in 1..columns as usize {
            place(out, name(i, j), format!("east_of={} gap=0", name(i - 1, j)));
        }
    }
}

/// Where the compile places a chunk of a site [`site`] writes with `rows` chunks along `z`: its
/// origin in the unit's grid, moved north by the first column's last chunk.
fn placed_origin(rows: usize, origin: Cell) -> (i32, i32, i32) {
    let rows = u32::try_from(rows).unwrap_or(u32::MAX);
    let shift = (rows - 1) * CHUNK_FOOTPRINT;
    let to_i32 = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
    (to_i32(origin.0), 0, to_i32(origin.2) - to_i32(shift))
}
