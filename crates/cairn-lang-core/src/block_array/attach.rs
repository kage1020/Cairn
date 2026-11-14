//! A circuit laid under a built structure: new layers under its base plane, which rises over
//! them (`spec/redstone` "Place-and-route").

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::error::Span;

use super::{
    Bbox, BlockArrayIr, BlockEntity, BlockState, Dims, MAX_STRUCTURE_VOLUME, MemberGeometry,
    PaletteFull, PaletteIndex, ResolvedBlock, ResolvedMember,
};

/// One block of a circuit, at `(x, y, z)` in the layers under the base plane, `y = 0` the lowest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidBlock {
    /// Where it stands.
    pub at: (u32, u32, u32),
    /// What it is.
    pub state: BlockState,
    /// Its block entity, if it has one.
    pub entity: Option<LaidEntity>,
}

/// The block entity of a [`LaidBlock`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidEntity {
    /// Its id, e.g. `minecraft:comparator`.
    pub id: String,
    /// Its integer fields, such as a comparator's `OutputSignal`, none named `id`, `x`, `y` or `z`.
    pub fields: IndexMap<String, i32>,
}

/// The fields a writer fills from a block entity's own id and position.
const POSITIONED_FIELDS: [&str; 4] = ["id", "x", "y", "z"];

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
    /// A block lies outside the layers under the base plane.
    #[error("a circuit block at {at:?} lies outside the layers under the floor")]
    OutOfBounds {
        /// Where it lies.
        at: (u32, u32, u32),
    },
    /// Two blocks stand at one position.
    #[error("two circuit blocks stand at {at:?}")]
    Overlap {
        /// Where they stand.
        at: (u32, u32, u32),
    },
    /// A block entity names a field its id or position fills.
    #[error(
        "the circuit block entity at {at:?} names the field `{field}`, which its id or position fills"
    )]
    PositionedField {
        /// Where the block stands.
        at: (u32, u32, u32),
        /// The field.
        field: String,
    },
}

impl BlockArrayIr {
    /// Lays `blocks` in `void` new layers under the structure `key`, which rises over them while a
    /// placement's origin moves down, filing the blocks under the `circuit` line `span`, or leaves
    /// the structure as it was.
    ///
    /// # Errors
    ///
    /// [`AttachError`] when no structure is built under `key` or the circuit does not fit it.
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
        let reserved = Bbox::spanning(
            (0, 0, 0),
            (
                dims.x.saturating_sub(1),
                void.saturating_sub(1),
                dims.z.saturating_sub(1),
            ),
        );
        // A circuit that lays no block still writes air over every layer it reserves.
        let actual = Bbox::around(blocks.iter().map(|block| block.at)).unwrap_or(reserved);
        let mut member = ResolvedMember {
            role: "circuit".to_owned(),
            id: None,
            resolved_state: Vec::new(),
            geometry: MemberGeometry {
                anchor: reserved.min,
                declared_bbox: reserved,
                actual_bbox: actual,
                host_face: None,
            },
            span,
        };
        let sorted = indexed(blocks, dims, void)?;
        for &(index, block) in &sorted {
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
        // No member writes a circuit's blocks, so a slot only the circuit holds derived every key.
        let held = array.palette.entries.len();
        let mut derived = array.derived.clone();
        derived.resize(held, Vec::new());
        derived.extend(
            palette.entries[held..]
                .iter()
                .map(|state| state.properties.keys().cloned().collect()),
        );
        voxels.append(&mut array.voxels);
        array.voxels = voxels;
        array.palette = palette;
        array.derived = derived;
        array.dims = dims;
        for entity in &mut array.block_entities {
            entity.pos.1 += void;
        }
        for entity in &mut array.entities {
            entity.pos.1 += f64::from(void);
        }
        for (_, block) in &sorted {
            if let Some(entity) = &block.entity {
                array.block_entities.push(BlockEntity {
                    pos: block.at,
                    id: entity.id.clone(),
                    fields: entity.fields.clone(),
                });
            }
        }
        if let Some(members) = self.members.get_mut(key) {
            for resolved in members.iter_mut() {
                resolved.geometry = resolved.geometry.raised(void);
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

/// Each of `blocks` with its index into `dims`, in the order a structure lists its voxels.
fn indexed(
    blocks: &[LaidBlock],
    dims: Dims,
    void: u32,
) -> Result<Vec<(usize, &LaidBlock)>, AttachError> {
    let mut taken = HashSet::new();
    let mut sorted = Vec::with_capacity(blocks.len());
    for block in blocks {
        let (x, y, z) = block.at;
        let Some(index) = dims.index(x, y, z).filter(|_| y < void) else {
            return Err(AttachError::OutOfBounds { at: block.at });
        };
        if !taken.insert(block.at) {
            return Err(AttachError::Overlap { at: block.at });
        }
        if let Some(field) = block
            .entity
            .iter()
            .flat_map(|entity| entity.fields.keys())
            .find(|field| POSITIONED_FIELDS.contains(&field.as_str()))
        {
            return Err(AttachError::PositionedField {
                at: block.at,
                field: field.clone(),
            });
        }
        sorted.push((index, block));
    }
    sorted.sort_by_key(|(_, b)| (b.at.1, b.at.2, b.at.0));
    Ok(sorted)
}

#[cfg(test)]
mod tests;
