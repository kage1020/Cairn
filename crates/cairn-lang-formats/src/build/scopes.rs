//! Which of the scopes a source asks for a lowering built, and which it lost.

use std::collections::HashSet;

use cairn_lang_core::Resolution;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState};

/// Scopes the source asked for that hold no block after lowering, all-air arrays included: the
/// resolver's scopes, then one `site::SITE::FROM ↔ TO` per `connect` pair that laid none.
#[must_use]
pub fn dropped_scopes(resolution: &Resolution, block_ir: &BlockArrayIr) -> Vec<String> {
    let built = |key: &str| block_ir.structures.get(key).is_some_and(holds_a_block);
    let mut dropped: Vec<String> = resolution
        .scopes
        .keys()
        .filter(|key| !key.starts_with("def::"))
        .filter(|key| !built(key))
        .cloned()
        .collect();
    let pair = |site: &str, from: String, to: String| {
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        (site.to_owned(), a, b)
    };
    let mut accounted: HashSet<(String, String, String)> = block_ir
        .walkways
        .iter()
        .filter(|(key, _)| built(key.as_str()))
        .map(|(_, w)| pair(w.site.as_str(), w.from.to_string(), w.to.to_string()))
        .collect();
    for connect in &resolution.connects {
        let (from, to) = (connect.from.to_string(), connect.to.to_string());
        let key = format!("site::{}::{from} ↔ {to}", connect.site);
        if accounted.insert(pair(connect.site.as_str(), from, to)) {
            dropped.push(key);
        }
    }
    dropped
}

/// How many structures hold at least one block.
#[must_use]
pub fn built_scopes(block_ir: &BlockArrayIr) -> usize {
    block_ir
        .structures
        .values()
        .filter(|array| holds_a_block(array))
        .count()
}

/// Whether any voxel of `array` is something other than air, wherever its palette puts air.
fn holds_a_block(array: &BlockArray) -> bool {
    array.voxels.iter().any(|index| {
        array
            .palette
            .entries
            .get(usize::from(index.0))
            .is_some_and(|state| state.id != BlockState::AIR_ID)
    })
}
