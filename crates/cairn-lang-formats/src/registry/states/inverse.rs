//! The canonical state a Bedrock `block_palette` entry reads as at one version: a state of a Java
//! block mapped onto its id whose form there is the entry, checked by resolving it.

use cairn_lang_core::block_array::BlockState;
use indexmap::IndexMap;

use super::{Family, Rule, VersionStates, WATER_LAYER_STATE};
use crate::registry::blocks::StateValue;

/// The most combinations of the values its states read that a family is searched over.
const MOST_COMBINATIONS: usize = 1 << 16;

/// How a source spells a Bedrock block at one version, as [`VersionStates::canonical_spelling`]
/// finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalSpelling {
    /// The state, with every property of its Java block.
    pub state: BlockState,
    /// The value the map takes for a property a line leaves out.
    pub defaults: IndexMap<String, String>,
    /// The properties no Bedrock state reads, which the entry is written the same at any value.
    pub unread: Vec<String>,
    /// Whether the state is written with the water layer.
    pub waterlogged: bool,
}

impl VersionStates<'_> {
    /// The canonical state that writes the Bedrock block `id` with `states` here, its
    /// `waterlogged` set by `water` where its block has one, spelled with an id `declares` takes:
    /// of the states the map writes so, the one with the fewest properties off their default.
    /// `None` when no state is written as the entry.
    #[must_use]
    pub fn canonical_spelling(
        &self,
        id: &str,
        states: &[(String, StateValue)],
        water: bool,
        declares: impl Fn(&str) -> bool,
    ) -> Option<CanonicalSpelling> {
        let forms = self.map.onto.get(id).map_or(&[][..], Vec::as_slice);
        let mut found: Vec<(usize, &str, &Family, IndexMap<String, String>)> = forms
            .iter()
            .filter_map(|(java, at)| {
                let family = self.family(*at);
                let (off, full) = closest(family, states, water)?;
                Some((off, java.as_str(), family, full))
            })
            .collect();
        found.sort_by_key(|(off, ..)| *off);
        for (_, java, family, full) in found {
            let unread = unread(family);
            let written: IndexMap<String, String> = full
                .iter()
                .filter(|(key, value)| {
                    !unread.contains(*key) && family.defaults.get(*key) != Some(*value)
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let spellings = [java, id];
            for spelled in spellings.iter().take(if java == id { 1 } else { 2 }) {
                if !declares(spelled) {
                    continue;
                }
                let line = BlockState {
                    id: (*spelled).to_owned(),
                    properties: written.clone(),
                };
                let Ok(form) = self.resolve(&line) else {
                    continue;
                };
                if form.id == id && same(&form.states, states) {
                    return Some(CanonicalSpelling {
                        state: BlockState {
                            id: (*spelled).to_owned(),
                            properties: full,
                        },
                        defaults: family.defaults.clone(),
                        unread,
                        waterlogged: form.waterlogged,
                    });
                }
            }
        }
        if !forms.is_empty() || !declares(id) {
            return None;
        }
        let bare = BlockState::bare(id);
        let form = self.resolve(&bare).ok()?;
        (form.id == id && same(&form.states, states)).then(|| CanonicalSpelling {
            state: bare,
            defaults: IndexMap::new(),
            unread: Vec::new(),
            waterlogged: form.waterlogged,
        })
    }
}

/// Of `family`'s whole states whose Bedrock states are `states`, the one with the fewest
/// properties off their default, with that count.
fn closest(
    family: &Family,
    states: &[(String, StateValue)],
    water: bool,
) -> Option<(usize, IndexMap<String, String>)> {
    // A property the family holds only off its block's default, as an unlit torch's `lit`, is
    // one of the values it holds.
    let mut base: IndexMap<String, String> = family
        .java
        .iter()
        .map(|(key, held)| {
            let default = &family.defaults[key];
            let value = if held.contains(default) {
                default
            } else {
                &held[0]
            };
            (key.clone(), value.clone())
        })
        .collect();
    if let Some(held) = family.java.get(WATER_LAYER_STATE) {
        let wanted = if water { "true" } else { "false" };
        if held.iter().any(|value| value == wanted) {
            base.insert(WATER_LAYER_STATE.to_owned(), wanted.to_owned());
        }
    }
    let read = read_keys(family);
    let count = read
        .iter()
        .try_fold(1_usize, |n, key| n.checked_mul(family.java[*key].len()))
        .filter(|&n| n <= MOST_COMBINATIONS)?;
    let mut best: Option<(usize, IndexMap<String, String>)> = None;
    for combination in 0..count {
        let mut full = base.clone();
        let mut rest = combination;
        for key in &read {
            let held = &family.java[*key];
            full.insert((*key).to_owned(), held[rest % held.len()].clone());
            rest /= held.len();
        }
        if !same(&family.bedrock_states(&full), states) {
            continue;
        }
        let off = read
            .iter()
            .filter(|key| family.defaults.get(**key) != full.get(**key))
            .count();
        if best.as_ref().is_none_or(|(least, _)| off < *least) {
            best = Some((off, full));
        }
    }
    best
}

/// The properties some state of `family` reads, in its block's order.
fn read_keys(family: &Family) -> Vec<&str> {
    family
        .java
        .keys()
        .filter(|key| {
            family.states.iter().any(|(_, rule)| match rule {
                Rule::Lookup { reads, .. } => reads.contains(key),
                Rule::Constant(_) => false,
            })
        })
        .map(String::as_str)
        .collect()
}

/// The properties of `family` no state reads and that take their default in it, but the water
/// layer's: a line leaving one out is written as `family` still.
fn unread(family: &Family) -> Vec<String> {
    let read = read_keys(family);
    family
        .java
        .iter()
        .filter(|(key, held)| {
            *key != WATER_LAYER_STATE
                && !read.contains(&key.as_str())
                && held.contains(&family.defaults[*key])
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// Whether `a` and `b` hold the same states, in any order.
fn same(a: &[(String, StateValue)], b: &[(String, StateValue)]) -> bool {
    a.len() == b.len() && a.iter().all(|pair| b.contains(pair))
}
