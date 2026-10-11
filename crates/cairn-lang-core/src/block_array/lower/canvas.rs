//! The grid and the palette one body is painted into.

use indexmap::IndexMap;

use crate::block_array::{BlockState, Dims, Palette, PaletteFull, PaletteIndex};

use super::members::Phase;

/// Drop the palette slots a later phase covered, and remap the voxels
/// onto the survivors.
///
/// A member's cells may all be covered by a later phase (a `window` set
/// into a `walls` ring), leaving its material in the palette for a block
/// the structure does not contain. The Java writer would emit that entry,
/// `cairn info` would report a portability row for it, and
/// `resolved_ir_hash` would cover it, so sources differing only in which
/// member lost would hash apart.
///
/// Slot 0 stays whatever happens: [`Palette::new_with_air`] puts air there
/// before any member runs, and a fully paved volume names it nowhere.
///
/// A slot **never painted at all** is a generator bug, not a covered
/// cell: it is kept, so `tests/lower/palette_is_referenced.rs` can catch
/// it, and returned for the caller to assert on.
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
/// Every paint interns its state, covered later or not, so the palette
/// fills with the states *written* and overflow is caught at paint time.
/// A paint past the capacity gets air and records the refusal, which
/// [`Self::into_palette`] answers with, and the body is refused with
/// `W_PALETTE_TOO_LARGE` before its grid is read. Holding the refusal here
/// lets generators keep an infallible `intern` without a `Result` threaded
/// out of each, and the type keeps them off the panicking
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
        // After an overflow the body is refused and the palette never
        // read, so air without scanning every entry on each later paint.
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
/// **Which palette slots a write has named.** An entry no voxel
/// references comes two ways: a generator claiming a material for geometry
/// it never emits (a bug), or a member's cells covered by a later phase
/// (the layering working). [`prune_unreferenced`] drops the second and
/// keeps the first where the artifact shows it, which the finished grid
/// alone cannot tell apart.
///
/// Nothing paints through a `Canvas` directly: [`Self::for_member`] hands
/// out a [`MemberCanvas`], and that is the only type with a write on it.
pub(super) struct Canvas {
    dims: Dims,
    voxels: Vec<PaletteIndex>,
    /// Per voxel: `0` while nothing has written it, and `n + 1` once the
    /// member at index `n` of the flattened paint list has.
    ///
    /// One `u32` rather than a `(phase, index)` pair, which at 16 bytes
    /// (align 8) would be 256 MB of bookkeeping against 32 MB of voxels at
    /// the [`MAX_STRUCTURE_VOLUME`] cap, a cap chosen against the grid
    /// alone. The phase comes from [`Self::phases`], one entry per member.
    ///
    /// [`MAX_STRUCTURE_VOLUME`]: crate::block_array::MAX_STRUCTURE_VOLUME
    owners: Vec<u32>,
    /// Phase of each member of the flattened paint list, by index. `None`
    /// for a member no phase evaluates, which never paints; the entry
    /// keeps the indices aligned.
    phases: Vec<Option<Phase>>,
    /// Indexed by palette slot: `true` once a write has named that slot.
    painted: Vec<bool>,
    /// Indexed like [`Self::phases`]: `true` once that member has written
    /// a cell, whatever a later write did to it: which openings were cut,
    /// for the `connect` rows that anchor on them.
    pub(super) wrote: Vec<bool>,
    /// `(overridden, overriding)` member pairs to the number of voxels the
    /// second took from the first, in discovery order.
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
    /// The only way to reach a write, so a generator cannot paint without
    /// a member to file it under, nor file it under a stale one:
    /// [`run_phase`] opens the scope once per member and the borrow closes
    /// it.
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
    /// Every generator derives its coordinates from the [`Dims`] the
    /// volume was allocated against, so a coordinate outside it means the
    /// dim math and the generator disagree. The write is clipped rather
    /// than panicking a release build, and the `debug_assert!` makes the
    /// disagreement a test failure rather than a silent drop.
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
    /// so the palette gains no entry no voxel names.
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
        // A write that changes nothing is not a contest: two `walls` of one
        // material meeting at a corner, or a `window` whose `repeat=` /
        // `step=` stamps overlap.
        if before == after || previous == 0 {
            return true;
        }
        // No generator writes one cell to two different blocks, so a
        // same-member pair stops above. The check stays because the
        // finding is about two members: against itself it would read
        // "`roof` overwrites 3 voxels that `roof` painted", a generator
        // fault no author can move.
        let overridden = previous - 1;
        if overridden != member && canvas.same_phase(overridden, member) {
            *canvas.conflicts.entry((overridden, member)).or_default() += 1;
        }
        true
    }
}
