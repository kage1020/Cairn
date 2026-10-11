//! The grid and the palette one body is painted into.

use indexmap::IndexMap;

use crate::block_array::{BlockState, Dims, Palette, PaletteFull, PaletteIndex};

use super::members::Phase;

/// Drop the palette slots a later phase covered, and remap the voxels
/// onto the survivors.
///
/// A member may paint a cell a later phase then covers — a `window` set
/// into a `walls` ring takes those cells for good — and when a member's
/// last cell goes, its material is left in the palette describing a block
/// the structure does not contain. That entry is not free: the Java writer
/// emits it into the `.nbt`, `cairn info` reports a portability row for
/// it, and `resolved_ir_hash` covers it, so two sources that differ only
/// in which member lost would hash apart.
///
/// Slot 0 stays whatever happens: [`Palette::new_with_air`] puts air there
/// before any member runs, and a fully paved volume names it nowhere.
///
/// A slot that was **never painted at all** is a generator bug, not a
/// covered cell, and is left in place so `tests/palette_is_referenced`
/// can catch it; such slots are returned for the caller to assert on.
pub(super) fn prune_unreferenced(
    palette: &mut Palette,
    canvas: &Canvas,
) -> (Vec<PaletteIndex>, Vec<usize>) {
    let mut referenced = vec![false; palette.entries.len()];
    for v in &canvas.voxels {
        referenced[usize::from(v.0)] = true;
    }
    let mut remap: Vec<PaletteIndex> = vec![PaletteIndex::AIR; palette.entries.len()];
    let mut kept = Vec::with_capacity(palette.entries.len());
    let mut never_painted = Vec::new();
    for (slot, state) in palette.entries.drain(..).enumerate() {
        let painted = canvas.painted.get(slot).copied().unwrap_or(false);
        if slot != 0 && !referenced[slot] {
            if painted {
                continue;
            }
            never_painted.push(slot);
        }
        remap[slot] = PaletteIndex(
            u16::try_from(kept.len()).expect("the kept palette is no longer than the original"),
        );
        kept.push(state);
    }
    palette.entries = kept;
    let voxels = canvas
        .voxels
        .iter()
        .map(|v| remap[usize::from(v.0)])
        .collect();
    (voxels, never_painted)
}

/// The palette one body is painted against, refusing rather than
/// panicking once it holds its capacity: [`PALETTE_CAPACITY`] entries
/// from [`lower_to_block_array`].
///
/// Every paint interns its state, including one a later member then
/// covers, so what fills the palette is the states *written*, not the
/// states the finished body keeps, and the overflow has to be caught at
/// paint time. A paint past the capacity gets air in place of its state
/// and records the refusal; [`Self::into_palette`] then answers with it,
/// and the body is refused with `W_PALETTE_TOO_LARGE` before its grid is
/// read. Holding the refusal here, rather than threading a `Result` out
/// of every generator, is what lets the generators keep their infallible
/// `intern`, and the type is what keeps them off the panicking
/// [`Palette::intern`].
///
/// [`PALETTE_CAPACITY`]: crate::block_array::PALETTE_CAPACITY
/// [`lower_to_block_array`]: super::lower_to_block_array
pub(super) struct ScopePalette {
    palette: Palette,
    capacity: usize,
    /// The refusal that overflowed the palette, once one has.
    overflow: Option<PaletteFull>,
}

impl ScopePalette {
    /// Air at index `0`, and room for `capacity` entries in all, air's
    /// among them; [`Palette::try_intern_within`] clamps a larger one to
    /// [`PALETTE_CAPACITY`].
    ///
    /// [`PALETTE_CAPACITY`]: crate::block_array::PALETTE_CAPACITY
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            palette: Palette::new_with_air(),
            capacity,
            overflow: None,
        }
    }

    pub(super) fn intern(&mut self, state: BlockState) -> PaletteIndex {
        // Once a paint has overflowed, `into_palette` discards the palette
        // and the body is refused, so which states the palette holds from
        // here on, and which index a later paint gets, is never read. Air
        // without the scan, which would otherwise walk every entry on each
        // remaining paint of an already refused body.
        if self.overflow.is_some() {
            return PaletteIndex::AIR;
        }
        self.palette
            .try_intern_within(state, self.capacity)
            .unwrap_or_else(|full| {
                self.overflow = Some(full);
                PaletteIndex::AIR
            })
    }

    /// The palette, or the refusal that overflowed it.
    pub(super) fn into_palette(self) -> Result<Palette, PaletteFull> {
        match self.overflow {
            None => Ok(self.palette),
            Some(full) => Err(full),
        }
    }
}

/// The voxel grid under construction, plus the two things the phase model
/// needs to know about the writes that built it.
///
/// **Who wrote each cell.** `spec/compilation` "Phase evaluation" promises
/// that a `window` written after a `roof` still lands as an opening, and only
/// says last-wins for "local overrides within the same phase". So a
/// cross-phase overwrite is the model working and a within-phase one is the
/// author's two members contesting a cell with nothing but line order to
/// separate them. The canvas can tell the two apart because it remembers the
/// writer, and reports the second kind rather than resolving it silently.
///
/// **Which palette slots a write has named.** An entry no voxel references
/// reaches the `.nbt`, the `resolved_ir_hash`, and `cairn info`'s per-entry
/// rows, so it is a block the tooling counts for a build that does not
/// contain it. There are two ways to get one and they are not the same
/// mistake: a generator claiming a material for geometry it never emits is
/// a bug in the generator, while a member painting a cell a later phase
/// then covers is the layering doing its job. [`prune_unreferenced`] drops
/// the second and leaves the first where the artifact still shows it,
/// which is why the flag is kept at all rather than deriving everything
/// from the finished grid.
///
/// Nothing paints through a `Canvas` directly: [`Self::for_member`] hands
/// out a [`MemberCanvas`], and that is the only type with a write on it.
pub(super) struct Canvas {
    dims: Dims,
    voxels: Vec<PaletteIndex>,
    /// Per voxel: `0` while nothing has written it, and `n + 1` once the
    /// member at index `n` of the flattened paint list has.
    ///
    /// One `u32` rather than the `(phase, index)` pair this began as. The
    /// pair is 16 bytes at align 8, which over a volume at the
    /// [`MAX_STRUCTURE_VOLUME`] cap is 256 MB of bookkeeping against
    /// 32 MB of voxels — a cap chosen against the grid alone would no
    /// longer mean what it was set to mean. The phase comes from
    /// [`Self::phases`] instead, which is one entry per member rather
    /// than one per cell.
    ///
    /// [`MAX_STRUCTURE_VOLUME`]: crate::block_array::MAX_STRUCTURE_VOLUME
    owners: Vec<u32>,
    /// Phase of each member of the flattened paint list, by that member's
    /// index. `None` for the members no phase evaluates — they never
    /// paint, so the entry exists only to keep the indices aligned.
    phases: Vec<Option<Phase>>,
    /// Indexed by palette slot: `true` once a write has named that slot.
    painted: Vec<bool>,
    /// Indexed like [`Self::phases`]: `true` once that member has written
    /// a cell, whatever a later write did to it. How the pass knows which
    /// openings were cut, for the `connect` rows that anchor on them.
    pub(super) wrote: Vec<bool>,
    /// `(overridden, overriding)` member pairs to the number of voxels the
    /// second took from the first. Insertion-ordered so the diagnostics
    /// come out in the order the phases discovered them, then sorted by
    /// span with the rest of the pass's findings.
    pub(super) conflicts: IndexMap<(u32, u32), u32>,
}

impl Canvas {
    pub(super) fn new(dims: Dims, phases: Vec<Option<Phase>>) -> Self {
        Self {
            dims,
            voxels: vec![PaletteIndex::AIR; dims.volume()],
            owners: vec![0; dims.volume()],
            wrote: vec![false; phases.len()],
            phases,
            painted: vec![false; 1],
            conflicts: IndexMap::new(),
        }
    }

    /// Borrow the canvas as the member at `index` of the flattened paint
    /// list sees it.
    ///
    /// The only way to reach a write. A generator cannot paint without a
    /// member to file the write under, and cannot file it under the wrong
    /// one by forgetting to update a field: [`run_phase`] opens the scope
    /// once per member and the borrow closes it.
    ///
    /// [`run_phase`]: super::body::run_phase
    pub(super) fn for_member(&mut self, index: u32) -> MemberCanvas<'_> {
        debug_assert!(
            (index as usize) < self.phases.len(),
            "member {index} is not in the flattened paint list this canvas was built for",
        );
        MemberCanvas {
            canvas: self,
            member: index,
        }
    }

    /// Whether two members of the flattened paint list are evaluated in
    /// one phase.
    ///
    /// A member with no phase never paints, so a `None` on either side
    /// describes a write that could not have happened; it answers `false`
    /// rather than letting two of them match each other.
    fn same_phase(&self, a: u32, b: u32) -> bool {
        matches!(
            (
                self.phases.get(a as usize).copied().flatten(),
                self.phases.get(b as usize).copied().flatten(),
            ),
            (Some(x), Some(y)) if x == y
        )
    }
}

/// The canvas as one member sees it: the same grid, with every write
/// already attributed.
pub(super) struct MemberCanvas<'a> {
    canvas: &'a mut Canvas,
    member: u32,
}

impl MemberCanvas<'_> {
    /// Paint one voxel, claiming its palette slot only once the cell is
    /// known to exist.
    ///
    /// Every generator derives its coordinates from the same [`Dims`] the
    /// volume was allocated against, so a coordinate outside it means the
    /// dim math and the generator disagree. That disagreement is what a
    /// `roof` under `level y=0` used to be: a full set of stair entries
    /// reached the palette and not one stair reached the structure, with
    /// nothing said. The write is still clipped rather than panicking a
    /// release build, and the `debug_assert!` turns the disagreement into
    /// a test failure instead of a silent drop.
    pub(super) fn paint(&mut self, pos: (u32, u32, u32), claim: impl FnOnce() -> PaletteIndex) {
        if !self.try_paint(pos, claim) {
            debug_assert!(
                false,
                "voxel {pos:?} lies outside {:?}; the dim math and the generator disagree",
                self.canvas.dims,
            );
        }
    }

    /// [`Self::paint`] for the one generator whose coordinate is the
    /// author's to get wrong: a `pressure_plate` anchors at `at=`, so an
    /// out-of-range cell there earns a diagnostic rather than an assertion.
    /// Answers whether the cell existed; `claim` runs only when it did,
    /// which is what keeps the palette free of entries no voxel names.
    pub(super) fn try_paint(
        &mut self,
        pos: (u32, u32, u32),
        claim: impl FnOnce() -> PaletteIndex,
    ) -> bool {
        let member = self.member;
        let canvas = &mut *self.canvas;
        let Some(i) = canvas.dims.index(pos.0, pos.1, pos.2) else {
            return false;
        };
        let after = claim();
        let slot = usize::from(after.0);
        if canvas.painted.len() <= slot {
            canvas.painted.resize(slot + 1, false);
        }
        canvas.painted[slot] = true;
        if let Some(wrote) = canvas.wrote.get_mut(member as usize) {
            *wrote = true;
        }
        let before = canvas.voxels[i];
        canvas.voxels[i] = after;
        let previous = std::mem::replace(&mut canvas.owners[i], member + 1);
        // A write that changes nothing is not a contest: two `walls`
        // members of one material meeting at a corner leave the same block
        // whichever order they run in, and a `window` whose `repeat=` /
        // `step=` stamps overlap covers its own cells with its own
        // material.
        if before == after || previous == 0 {
            return true;
        }
        // The member check is the other half of that and is unreachable
        // from today's generators — none writes one cell to two different
        // blocks, so a same-member pair never gets past the line above.
        // It stays because the finding is a statement about two members:
        // reported against itself it would read "`roof` overwrites 3
        // voxels that `roof` painted", which is a generator emitting two
        // faces onto one cell and not something an author can move.
        let overridden = previous - 1;
        if overridden != member && canvas.same_phase(overridden, member) {
            *canvas.conflicts.entry((overridden, member)).or_default() += 1;
        }
        true
    }
}
