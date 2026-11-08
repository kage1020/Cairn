//! A circuit laid under a built structure: new layers under its base plane, which rises over
//! them (`spec/redstone` "Place-and-route").

use indexmap::IndexMap;

use crate::error::Span;

use super::{
    BlockArrayIr, BlockEntity, BlockState, MAX_STRUCTURE_VOLUME, PaletteFull, PaletteIndex,
    ResolvedBlock, ResolvedMember,
};

/// One block of a circuit, at `(x, y, z)` in the layers under the base plane, `y = 0` the lowest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidBlock {
    /// Where it stands.
    pub at: (u32, u32, u32),
    /// What it is.
    pub state: BlockState,
    /// Its block entity's integer fields, if it has one, such as a comparator's `OutputSignal`.
    pub entity: Option<(String, IndexMap<String, i32>)>,
}

/// Why a circuit could not be laid under a structure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AttachError {
    /// No structure is built under that key.
    #[error("no structure is built under that key")]
    NoStructure,
    /// The layers under the base plane take the structure past [`MAX_STRUCTURE_VOLUME`].
    #[error("the layers under the floor take the structure past {MAX_STRUCTURE_VOLUME} blocks")]
    TooLarge,
    /// The structure's palette has no room for the circuit's states.
    #[error(transparent)]
    PaletteFull(PaletteFull),
}

impl BlockArrayIr {
    /// Lays `blocks` in `void` new layers under the structure `key`, which rises over them while a
    /// placement's origin moves down, filing the blocks under the `circuit` line `span`.
    ///
    /// # Errors
    ///
    /// When no structure is built under `key`, the layers make it too large, or its palette
    /// cannot hold the circuit's states; the structure is then left as it was.
    pub fn attach_circuit(
        &mut self,
        key: &str,
        void: u32,
        blocks: &[LaidBlock],
        span: Span,
    ) -> Result<(), AttachError> {
        let array = self
            .structures
            .get_mut(key)
            .ok_or(AttachError::NoStructure)?;
        let mut dims = array.dims;
        dims.y = dims.y.checked_add(void).ok_or(AttachError::TooLarge)?;
        if !dims.fits_volume_budget() {
            return Err(AttachError::TooLarge);
        }
        let mut palette = array.palette.clone();
        let layer = (array.dims.x as usize) * (array.dims.z as usize);
        let mut voxels = vec![PaletteIndex::AIR; layer * void as usize];
        let mut member = ResolvedMember {
            role: "circuit".to_owned(),
            id: None,
            resolved_state: Vec::new(),
            span,
        };
        let mut sorted: Vec<&LaidBlock> = blocks.iter().filter(|b| b.at.1 < void).collect();
        sorted.sort_by_key(|b| (b.at.1, b.at.2, b.at.0));
        for block in &sorted {
            let (x, y, z) = block.at;
            let Some(index) = dims.index(x, y, z) else {
                continue;
            };
            let slot = palette
                .try_intern(block.state.clone())
                .map_err(AttachError::PaletteFull)?;
            voxels[index] = slot;
            if let Some(resolved) = member
                .resolved_state
                .iter_mut()
                .find(|resolved| resolved.state == block.state)
            {
                resolved.push(block.at);
            } else {
                let mut resolved = ResolvedBlock {
                    state: block.state.clone(),
                    runs: Vec::new(),
                };
                resolved.push(block.at);
                member.resolved_state.push(resolved);
            }
        }
        voxels.append(&mut array.voxels);
        array.voxels = voxels;
        array.palette = palette;
        array.dims = dims;
        for entity in &mut array.block_entities {
            entity.pos.1 += void;
        }
        for entity in &mut array.entities {
            entity.pos.1 += f64::from(void);
        }
        for block in &sorted {
            if let Some((id, fields)) = &block.entity {
                array.block_entities.push(BlockEntity {
                    pos: block.at,
                    id: id.clone(),
                    fields: fields.clone(),
                });
            }
        }
        if let Some(members) = self.members.get_mut(key) {
            for resolved in members.iter_mut() {
                for state in &mut resolved.resolved_state {
                    for run in &mut state.runs {
                        run.from.1 += void;
                    }
                }
            }
            members.insert(0, member);
        }
        if let Some(placement) = self.placements.get_mut(key) {
            placement.origin.1 -= i32::try_from(void).unwrap_or(i32::MAX);
            placement.dims = dims;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
