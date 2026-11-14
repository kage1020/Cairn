//! The grid and the palette one body is painted into.

use indexmap::IndexMap;

use crate::block_array::geometry::Declared;
use crate::block_array::material::is_stand_in;
use crate::block_array::{
    BlockEntity, BlockEntityData, BlockState, Dims, PALETTE_CAPACITY, Palette, PaletteFull,
    PaletteIndex,
};

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
    overflow: Option<Overflow>,
    /// The one slot every stand-in shares, once one is interned.
    stand_in: Option<PaletteIndex>,
}

/// Why a [`ScopePalette`] refused a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Overflow {
    /// The states that are no stand-in passed the capacity, so every target refuses the body.
    Full(PaletteFull),
    /// The stand-in's slot took the last index, and only the pack can say whether it was needed.
    StandIn,
}

impl ScopePalette {
    /// Air at index `0`, and room for `capacity` entries in all, air's among them.
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            palette: Palette::new_with_air(),
            capacity,
            overflow: None,
            stand_in: None,
        }
    }

    pub(super) fn intern(&mut self, state: BlockState) -> PaletteIndex {
        // After an overflow the palette is never read, so skip the scan.
        if self.overflow.is_some() {
            return PaletteIndex::AIR;
        }
        let entries = &mut self.palette.entries;
        if is_stand_in(&state) {
            // How many blocks the stand-ins come to, and which the body already paints, is the
            // pack's to say, so they share one slot; past the last index they share the newest.
            return *self.stand_in.get_or_insert_with(|| {
                let next = u16::try_from(entries.len()).unwrap_or(u16::MAX);
                if usize::from(next) == entries.len() {
                    entries.push(state);
                }
                PaletteIndex(next)
            });
        }
        if let Some(i) = entries.iter().position(|s| s == &state) {
            return PaletteIndex(
                u16::try_from(i).expect("palette index fits in u16 by construction"),
            );
        }
        let uncounted = self
            .stand_in
            .is_some_and(|slot| entries.get(usize::from(slot.0)).is_some_and(is_stand_in));
        let capacity = self.capacity.min(PALETTE_CAPACITY);
        if entries.len() - usize::from(uncounted) >= capacity {
            self.overflow = Some(Overflow::Full(PaletteFull { capacity }));
            return PaletteIndex::AIR;
        }
        let Ok(next) = u16::try_from(entries.len()) else {
            self.overflow = Some(Overflow::StandIn);
            return PaletteIndex::AIR;
        };
        entries.push(state);
        PaletteIndex(next)
    }

    /// How many states the palette holds, air included.
    pub(super) fn len(&self) -> usize {
        self.palette.entries.len()
    }

    /// The state at `slot`, which must be below [`Self::len`].
    pub(super) fn state(&self, slot: usize) -> &BlockState {
        &self.palette.entries[slot]
    }

    /// The slot every stand-in shares, once one is interned.
    pub(super) fn stand_in(&self) -> Option<PaletteIndex> {
        self.stand_in
    }

    /// The palette, or the refusal that overflowed it.
    pub(super) fn into_palette(self) -> Result<Palette, Overflow> {
        match self.overflow {
            None => Ok(self.palette),
            Some(overflow) => Err(overflow),
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
    /// Indexed like [`Self::phases`]: what that member's line declares, once its painter read it.
    declared: Vec<Option<Declared>>,
    /// `(overridden, overriding)` member pairs, in the order they are judged, to how many voxels
    /// the second took from each `(before, after)` pair of palette slots.
    pub(super) conflicts: IndexMap<(u32, u32), IndexMap<(PaletteIndex, PaletteIndex), u32>>,
    /// Indexed like [`Self::phases`]: `true` while that member's contests wait on
    /// [`Self::settle_contests`].
    held: Vec<bool>,
    /// The contests a held member took part in, in the order they happened.
    deferred: Vec<Contest>,
    /// Each block entity a member set beside a block it painted, with that member.
    block_entities: Vec<(u32, BlockEntity)>,
}

/// One cell a member wrote over another member's in the same phase.
#[derive(Debug, Clone, Copy)]
struct Contest {
    overridden: u32,
    overriding: u32,
    before: PaletteIndex,
    after: PaletteIndex,
}

impl Canvas {
    pub(super) fn new(dims: Dims, phases: Vec<Option<Phase>>) -> Self {
        Self {
            dims,
            voxels: vec![PaletteIndex::AIR; dims.volume()],
            owners: vec![0; dims.volume()],
            wrote: vec![false; phases.len()],
            declared: vec![None; phases.len()],
            held: vec![false; phases.len()],
            phases,
            painted: vec![false; 1],
            conflicts: IndexMap::new(),
            deferred: Vec::new(),
            block_entities: Vec::new(),
        }
    }

    /// Each block entity whose member still holds its cell, in the order they were set.
    pub(super) fn block_entities(&self) -> Vec<BlockEntity> {
        self.block_entities
            .iter()
            .filter(|(member, entity)| self.owner_at(entity.pos) == Some(*member))
            .map(|(_, entity)| entity.clone())
            .collect()
    }

    /// Record that a write named `slot`, for a state a member settled on in a cell it no longer
    /// holds.
    pub(super) fn name(&mut self, slot: PaletteIndex) {
        let slot = usize::from(slot.0);
        if self.painted.len() <= slot {
            self.painted.resize(slot + 1, false);
        }
        self.painted[slot] = true;
    }

    /// Judge every held contest with each held member's slots mapped by `settled`, the
    /// `(member, painted)` → slot it settled on, and stop holding them.
    pub(super) fn settle_contests(&mut self, settled: impl Fn(u32, PaletteIndex) -> PaletteIndex) {
        for contest in std::mem::take(&mut self.deferred) {
            let before = settled(contest.overridden, contest.before);
            let after = settled(contest.overriding, contest.after);
            if before != after {
                self.count(contest.overridden, contest.overriding, before, after);
            }
        }
        self.held.fill(false);
    }

    /// Count one voxel `overriding` took from `overridden`, from slot `before` to `after`.
    fn count(
        &mut self,
        overridden: u32,
        overriding: u32,
        before: PaletteIndex,
        after: PaletteIndex,
    ) {
        *self
            .conflicts
            .entry((overridden, overriding))
            .or_default()
            .entry((before, after))
            .or_default() += 1;
    }

    /// Whether the member's contests wait on [`Self::settle_contests`].
    fn holds(&self, member: u32) -> bool {
        self.held.get(member as usize).copied().unwrap_or(false)
    }

    /// Per voxel: `0` while unwritten, `n + 1` once member `n` of the flattened paint list did.
    pub(super) fn owners(&self) -> &[u32] {
        &self.owners
    }

    /// The palette slot `pos` holds, or `None` outside the grid.
    pub(super) fn slot_at(&self, pos: (u32, u32, u32)) -> Option<PaletteIndex> {
        Some(self.voxels[self.dims.index(pos.0, pos.1, pos.2)?])
    }

    /// Every cell a member wrote, with the slot it holds.
    pub(super) fn cells(&self) -> impl Iterator<Item = ((u32, u32, u32), PaletteIndex)> + '_ {
        self.voxels
            .iter()
            .zip(&self.owners)
            .enumerate()
            .filter(|(_, (_, owner))| **owner != 0)
            .filter_map(|(i, (slot, _))| Some((self.dims.position(i)?, *slot)))
    }

    /// What member `index` of the flattened paint list declared, or `None` if it declared nothing.
    pub(super) fn declared(&self, index: u32) -> Option<Declared> {
        self.declared.get(index as usize).copied().flatten()
    }

    /// The member of the flattened paint list that wrote `pos` last, or `None` for a cell no member
    /// wrote or one outside the grid.
    pub(super) fn owner_at(&self, pos: (u32, u32, u32)) -> Option<u32> {
        let i = self.dims.index(pos.0, pos.1, pos.2)?;
        self.owners[i].checked_sub(1)
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

    /// The phase that paints member `index` of the flattened paint list, if any.
    pub(super) fn phase_of(&self, index: u32) -> Option<Phase> {
        self.phases.get(index as usize).copied().flatten()
    }

    /// Whether two members of the flattened paint list run in one phase; never for `None`.
    fn same_phase(&self, a: u32, b: u32) -> bool {
        matches!(
            (
                self.phase_of(a),
                self.phase_of(b),
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
    /// The index in the flattened paint list of the member this canvas files writes under.
    pub(super) fn member(&self) -> u32 {
        self.member
    }

    /// Record what this member's line declares, before it paints the cells it names.
    pub(super) fn declare(&mut self, declared: Declared) {
        if let Some(slot) = self.canvas.declared.get_mut(self.member as usize) {
            *slot = Some(declared);
        }
    }

    /// Hold every contest this member's writes take part in until
    /// [`Canvas::settle_contests`], for a member whose states are settled after it paints.
    pub(super) fn hold_contests(&mut self) {
        if let Some(held) = self.canvas.held.get_mut(self.member as usize) {
            *held = true;
        }
    }

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

    /// Set `data` as the block entity of the block this member painted at `pos`, kept only while
    /// the member holds that cell.
    pub(super) fn set_block_entity(&mut self, pos: (u32, u32, u32), data: BlockEntityData) {
        self.canvas
            .block_entities
            .push((self.member, BlockEntity { pos, data }));
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
        canvas.name(after);
        if let Some(wrote) = canvas.wrote.get_mut(member as usize) {
            *wrote = true;
        }
        let before = canvas.voxels[i];
        canvas.voxels[i] = after;
        let previous = std::mem::replace(&mut canvas.owners[i], member + 1);
        // A first write is not a contest, and a same-member overwrite is a generator fault no
        // author can move, so never a finding.
        let Some(overridden) = previous.checked_sub(1) else {
            return true;
        };
        if overridden == member || !canvas.same_phase(overridden, member) {
            return true;
        }
        // A held slot may settle on another state, so even an unchanged one is judged later.
        if canvas.holds(overridden) || canvas.holds(member) {
            canvas.deferred.push(Contest {
                overridden,
                overriding: member,
                before,
                after,
            });
        } else if before != after {
            canvas.count(overridden, member, before, after);
        }
        true
    }
}
