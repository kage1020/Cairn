//! A read volume as the block grid transliteration writes, with what it leaves behind.

use std::collections::HashMap;

use cairn_lang_core::block_array::{BlockState, PaletteIndex};
use cairn_lang_core::transliterate::{BlockGrid, GridEntry};
use cairn_lang_nbt::tag::{Compound, Tag};
use indexmap::IndexMap;

use crate::bedrock_structure::BedrockVolume;
use crate::registry::{StateValue, VersionStates};
use crate::volume::{BlockVolume, StoredBlockEntity};

/// The Bedrock ids the second layer holds where a block stands in water.
const WATER_IDS: [&str; 2] = ["minecraft:water", "minecraft:flowing_water"];

/// A Java volume as a grid: every state as the file stores it, which Java builds as written.
pub(super) fn java_grid(volume: &BlockVolume) -> BlockGrid {
    BlockGrid {
        dims: volume.dims,
        palette: volume
            .palette
            .iter()
            .map(|state| GridEntry::exact(state.clone()))
            .collect(),
        voxels: volume.voxels.clone(),
    }
}

/// What a Bedrock volume reads as at one version's state map.
pub(super) struct BedrockGrid {
    pub(super) grid: BlockGrid,
    /// Each `block_palette` entry no canonical state writes, as `name[states]`, with its cells.
    pub(super) unspelled: Vec<(String, usize)>,
    /// Cells whose second layer holds water the written block does not stand in.
    pub(super) dried: usize,
    /// Cells whose written block stands in water the file's second layer does not hold.
    pub(super) flooded: usize,
    /// Each block the second layer holds that is not water, with its cells.
    pub(super) layered: Vec<(String, usize)>,
    /// Cells whose first layer holds a structure void.
    pub(super) void_cells: usize,
}

/// A Bedrock volume as a grid at `states`' version, each block spelled with an id `declares`
/// takes; a `block_palette` entry no canonical state writes is written by its id alone.
pub(super) fn bedrock_grid(
    volume: &BedrockVolume,
    states: &VersionStates<'_>,
    declares: &dyn Fn(&str) -> bool,
) -> BedrockGrid {
    let mut palette = vec![GridEntry::exact(BlockState::air())];
    // Per first-layer entry and whether water stands with it: its slot, and the water the
    // written block stands in, `None` for an entry no canonical state writes.
    let mut slots: HashMap<(usize, bool), (PaletteIndex, Option<bool>)> = HashMap::new();
    let mut unspelled: IndexMap<String, usize> = IndexMap::new();
    let mut layered: IndexMap<String, usize> = IndexMap::new();
    let (mut dried, mut flooded, mut void_cells) = (0, 0, 0);
    let mut voxels = Vec::with_capacity(volume.blocks.len());
    for (block, liquid) in volume.blocks.iter().zip(&volume.liquids) {
        let liquid = liquid.map(|slot| &volume.palette[usize::from(slot.0)]);
        let water = liquid.is_some_and(|liquid| WATER_IDS.contains(&liquid.name.as_str()));
        if let Some(liquid) = liquid.filter(|_| !water) {
            *layered.entry(liquid.name.clone()).or_default() += 1;
        }
        let Some(block) = block else {
            void_cells += 1;
            voxels.push(PaletteIndex::AIR);
            continue;
        };
        let at = usize::from(block.0);
        let entry = &volume.palette[at];
        let (slot, wet) = *slots.entry((at, water)).or_insert_with(|| {
            let spelling = typed_states(&entry.states)
                .and_then(|typed| states.canonical_spelling(&entry.name, &typed, water, declares));
            let (grid_entry, wet) = match spelling {
                Some(spelling) => (
                    GridEntry {
                        state: spelling.state,
                        defaults: spelling.defaults,
                        unread: spelling.unread,
                    },
                    Some(spelling.waterlogged),
                ),
                None => (GridEntry::exact(BlockState::bare(entry.name.clone())), None),
            };
            let slot = PaletteIndex(
                u16::try_from(palette.len()).expect("one entry per palette entry and water"),
            );
            palette.push(grid_entry);
            (slot, wet)
        });
        match wet {
            None => {
                *unspelled
                    .entry(spelled(&entry.name, &entry.states))
                    .or_default() += 1;
            }
            Some(false) if water => dried += 1,
            Some(true) if !water => flooded += 1,
            Some(_) => {}
        }
        voxels.push(slot);
    }
    BedrockGrid {
        grid: BlockGrid {
            dims: volume.dims,
            palette,
            voxels,
        },
        unspelled: unspelled.into_iter().collect(),
        dried,
        flooded,
        layered: layered.into_iter().collect(),
        void_cells,
    }
}

/// `states` as the state map types them: a byte of 0 or 1 a bool, an int, a string; `None` for
/// any other tag.
fn typed_states(states: &Compound) -> Option<Vec<(String, StateValue)>> {
    states
        .entries
        .iter()
        .map(|(name, tag)| {
            let value = match tag {
                Tag::Byte(0) => StateValue::Bool(false),
                Tag::Byte(1) => StateValue::Bool(true),
                Tag::Byte(n) => StateValue::Int(i64::from(*n)),
                Tag::Int(n) => StateValue::Int(i64::from(*n)),
                Tag::String(s) => StateValue::Str(s.clone()),
                _ => return None,
            };
            Some((name.clone(), value))
        })
        .collect()
}

/// A Bedrock block as `name[state=value,...]`.
fn spelled(name: &str, states: &Compound) -> String {
    if states.entries.is_empty() {
        return name.to_owned();
    }
    let pairs: Vec<String> = states
        .entries
        .iter()
        .map(|(key, tag)| match tag {
            Tag::String(value) => format!("{key}={value}"),
            Tag::Byte(value) => format!("{key}={value}"),
            Tag::Int(value) => format!("{key}={value}"),
            other => format!("{key}={other:?}"),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

/// The ids of `entities`, each named by its `id`, `Id` or `identifier`, or `nbt.id`, counted.
pub(super) fn entity_ids(entities: &[Compound]) -> Vec<(String, usize)> {
    let mut counted: IndexMap<String, usize> = IndexMap::new();
    for entity in entities {
        let named = |compound: &Compound| {
            ["id", "Id", "identifier"]
                .iter()
                .find_map(|key| match compound.entries.get(*key) {
                    Some(Tag::String(id)) => Some(id.clone()),
                    _ => None,
                })
        };
        let id = named(entity)
            .or_else(|| match entity.entries.get("nbt") {
                Some(Tag::Compound(nbt)) => named(nbt),
                _ => None,
            })
            .unwrap_or_else(|| "an entity with no id".to_owned());
        *counted.entry(id).or_default() += 1;
    }
    counted.into_iter().collect()
}

/// The ids of `block_entities`, counted.
pub(super) fn block_entity_ids(block_entities: &[StoredBlockEntity]) -> Vec<(String, usize)> {
    let mut counted: IndexMap<String, usize> = IndexMap::new();
    for entity in block_entities {
        *counted.entry(entity.id.clone()).or_default() += 1;
    }
    counted.into_iter().collect()
}
