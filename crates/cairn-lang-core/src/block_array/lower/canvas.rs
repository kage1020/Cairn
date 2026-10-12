//! The grid and the palette one body is painted into.

use indexmap::IndexMap;

use crate::block_array::{BlockState, Dims, Palette, PaletteFull, PaletteIndex};

use super::phase::Phase;

/// Drop the palette slots a later phase covered, and remap the voxels onto the survivors.
///
/// A slot never painted at all is a generator bug, kept and returned for the caller to check.
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

/// The palette one body is painted against, answering air past its capacity and recording the
/// refusal for [`Self::into_palette`].
pub(super) struct ScopePalette {
    palette: Palette,
    capacity: usize,
    /// The refusal that overflowed the palette, once one has.
    overflow: Option<PaletteFull>,
}

impl ScopePalette {
    /// Air at index `0`, and room for `capacity` entries in all, air's among them.
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            palette: Palette::new_with_air(),
            capacity,
            overflow: None,
        }
    }

    pub(super) fn intern(&mut self, state: BlockState) -> PaletteIndex {
        // After an overflow the palette is never read, so skip the scan.
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

/// The voxel grid under construction, with who wrote each cell and which palette slots a write
/// named, so within-phase contests and covered materials can be told apart.
pub(super) struct Canvas {
    dims: Dims,
    voxels: Vec<PaletteIndex>,
    /// Per voxel: `0` while unwritten, `n + 1` once member `n` of the flattened paint list did.
    owners: Vec<u32>,
    /// Phase of each member of the flattened paint list, `None` for one no phase evaluates.
    phases: Vec<Option<Phase>>,
    /// Indexed by palette slot: `true` once a write has named that slot.
    painted: Vec<bool>,
    /// Indexed like [`Self::phases`]: `true` once that member has written a cell.
    pub(super) wrote: Vec<bool>,
    /// `(overridden, overriding)` member pairs to the voxels the second took, in discovery order.
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

    /// Borrow the canvas as the member at `index` of the flattened paint list sees it.
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

    /// Whether two members of the flattened paint list run in one phase; never for `None`.
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

/// The canvas as one member sees it, every write attributed to that member.
pub(super) struct MemberCanvas<'a> {
    canvas: &'a mut Canvas,
    member: u32,
}

impl MemberCanvas<'_> {
    /// Paint one voxel, claiming its palette slot only once the cell is known to exist.
    pub(super) fn paint(&mut self, pos: (u32, u32, u32), claim: impl FnOnce() -> PaletteIndex) {
        if !self.try_paint(pos, claim) {
            debug_assert!(
                false,
                "voxel {pos:?} lies outside {:?}; the dim math and the generator disagree",
                self.canvas.dims,
            );
        }
    }

    /// [`Self::paint`] without the assertion, for a cell the author placed; whether it existed.
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
        // A write that changes nothing, or a first write, is not a contest.
        if before == after || previous == 0 {
            return true;
        }
        // A same-member overwrite is a generator fault no author can move, so never a finding.
        let overridden = previous - 1;
        if overridden != member && canvas.same_phase(overridden, member) {
            *canvas.conflicts.entry((overridden, member)).or_default() += 1;
        }
        true
    }
}
