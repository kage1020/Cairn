//! A box of voxels as a structure file stores it, which the readers return.

use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_nbt::tag::{Compound, Tag};
use indexmap::IndexMap;
use thiserror::Error;

/// The most memory, about, that the NBT a reader holds whole of one file may take: its entities,
/// its block entities' data, its palettes and its metadata (`spec/ecosystem-interop` "Import
/// stamping and pitfalls").
pub const MAX_HELD_NBT: usize = 64 << 20;

/// Voxels read from a structure file, with the palette and block entities the file stores, each
/// voxel a slot of the palette and each block entity on a voxel of the box.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockVolume {
    dims: Dims,
    palette: Vec<BlockState>,
    voxels: Vec<PaletteIndex>,
    block_entities: Vec<StoredBlockEntity>,
}

/// Why parts do not make a [`BlockVolume`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum VolumeError {
    /// A grid whose length is not the box's voxel count.
    #[error("the box holds {volume} voxels, and {voxels} were given")]
    VoxelCount {
        /// The box's voxel count, saturated at `usize::MAX`.
        volume: usize,
        /// The grid's length.
        voxels: usize,
    },
    /// A voxel naming a slot past the end of the palette.
    #[error("voxel {voxel} names slot {index} of a {len}-entry palette")]
    IndexOutsidePalette {
        /// The voxel, in `(y, z, x)` order.
        voxel: usize,
        /// The slot.
        index: u16,
        /// The palette's length.
        len: usize,
    },
    /// A block entity on no voxel of the box.
    #[error("block entity {index} sits at {pos:?}, outside the {dims:?} box")]
    BlockEntityOutside {
        /// Its position in the list.
        index: usize,
        /// Where it sits.
        pos: (u32, u32, u32),
        /// The box.
        dims: Dims,
    },
}

impl BlockVolume {
    /// The volume of these parts.
    ///
    /// # Errors
    ///
    /// A [`VolumeError`] for a grid that does not fill the box, a voxel past the palette, or a
    /// block entity outside the box.
    pub fn new(
        dims: Dims,
        palette: Vec<BlockState>,
        voxels: Vec<PaletteIndex>,
        block_entities: Vec<StoredBlockEntity>,
    ) -> Result<Self, VolumeError> {
        let volume = dims.volume();
        if voxels.len() != volume {
            return Err(VolumeError::VoxelCount {
                volume,
                voxels: voxels.len(),
            });
        }
        if let Some((voxel, index)) = voxels
            .iter()
            .enumerate()
            .find(|(_, index)| usize::from(index.0) >= palette.len())
        {
            return Err(VolumeError::IndexOutsidePalette {
                voxel,
                index: index.0,
                len: palette.len(),
            });
        }
        if let Some((index, entity)) = block_entities.iter().enumerate().find(|(_, entity)| {
            dims.index(entity.pos.0, entity.pos.1, entity.pos.2)
                .is_none()
        }) {
            return Err(VolumeError::BlockEntityOutside {
                index,
                pos: entity.pos,
                dims,
            });
        }
        Ok(Self {
            dims,
            palette,
            voxels,
            block_entities,
        })
    }

    /// The box's extents.
    #[must_use]
    pub fn dims(&self) -> Dims {
        self.dims
    }

    /// The block states the voxels index into, in the file's order.
    #[must_use]
    pub fn palette(&self) -> &[BlockState] {
        &self.palette
    }

    /// Palette indices in `(y, z, x)` order, as [`Dims::index`] lays them out.
    #[must_use]
    pub fn voxels(&self) -> &[PaletteIndex] {
        &self.voxels
    }

    /// The block entities, in the file's order.
    #[must_use]
    pub fn block_entities(&self) -> &[StoredBlockEntity] {
        &self.block_entities
    }
}

/// A block entity as a structure file stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredBlockEntity {
    /// Voxel coordinate, from the box's minimum corner.
    pub pos: (u32, u32, u32),
    /// Block entity id, e.g. `minecraft:comparator`.
    pub id: String,
    /// Its payload, without the id and position the file stores beside it.
    pub data: Compound,
}

/// The field of a palette entry that keeps it from being a block state, and what belongs there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotABlockState {
    /// The field, as `Name` or `Properties.facing`.
    pub(crate) field: String,
    /// What a palette entry holds there.
    pub(crate) expected: &'static str,
}

/// A palette entry's block state, under either key pair the game has used.
pub(crate) fn block_state_from_nbt(entry: &Compound) -> Result<BlockState, NotABlockState> {
    let (id_key, properties_key) = if entry.entries.contains_key("id") {
        ("id", "properties")
    } else {
        ("Name", "Properties")
    };
    let refuse = |field: String, expected| NotABlockState { field, expected };
    let Some(Tag::String(id)) = entry.entries.get(id_key) else {
        return Err(refuse(id_key.to_owned(), "a block id string"));
    };
    let mut properties = IndexMap::new();
    match entry.entries.get(properties_key) {
        None => {}
        Some(Tag::Compound(stored)) => {
            for (key, value) in &stored.entries {
                let Tag::String(value) = value else {
                    return Err(refuse(format!("{properties_key}.{key}"), "a string"));
                };
                properties.insert(key.clone(), value.clone());
            }
        }
        Some(_) => return Err(refuse(properties_key.to_owned(), "a compound of strings")),
    }
    Ok(BlockState {
        id: id.clone(),
        properties,
    })
}
