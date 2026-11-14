//! A box of voxels as a structure file stores it, which the readers return.

use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_nbt::tag::{Compound, Tag};
use indexmap::IndexMap;

/// Voxels read from a structure file, with the palette and block entities the file stores.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockVolume {
    /// Voxel extents.
    pub dims: Dims,
    /// The block states the voxels index into, in the file's order.
    pub palette: Vec<BlockState>,
    /// Palette indices in `(y, z, x)` order, as [`Dims::index`] lays them out.
    pub voxels: Vec<PaletteIndex>,
    /// The block entities, each at a voxel of the box.
    pub block_entities: Vec<StoredBlockEntity>,
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

/// A palette entry's block state, under either key pair the game has used, or `None` for a
/// compound that is not one.
pub(crate) fn block_state_from_nbt(entry: &Compound) -> Option<BlockState> {
    let (id_key, properties_key) = if entry.entries.contains_key("id") {
        ("id", "properties")
    } else {
        ("Name", "Properties")
    };
    let Some(Tag::String(id)) = entry.entries.get(id_key) else {
        return None;
    };
    let mut properties = IndexMap::new();
    match entry.entries.get(properties_key) {
        None => {}
        Some(Tag::Compound(stored)) => {
            for (key, value) in &stored.entries {
                let Tag::String(value) = value else {
                    return None;
                };
                properties.insert(key.clone(), value.clone());
            }
        }
        Some(_) => return None,
    }
    Some(BlockState {
        id: id.clone(),
        properties,
    })
}
