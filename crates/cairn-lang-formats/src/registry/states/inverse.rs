//! The canonical state a Bedrock `block_palette` entry reads as at one version: a state of a Java
//! block mapped onto its id whose form there is the entry, checked by resolving it.

use cairn_lang_core::block_array::BlockState;
use indexmap::IndexMap;

use super::{Family, Rule, VersionStates, WATER_LAYER_STATE};
use crate::registry::blocks::StateValue;

/// The most combinations of the values its states read that a family is searched over.
const MOST_COMBINATIONS: usize = 1 << 16;

/// The most properties a line leaves out whose every set is tried as the needed ones; past it, the
/// first `1 << MOST_SEARCHED` sets are, then all of them (`spec/ecosystem-interop` "What an import
/// writes").
const MOST_SEARCHED: usize = 12;

/// How a source spells a Bedrock block at one version, as [`VersionStates::canonical_spelling`]
/// finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalSpelling {
    /// The state, with every property of its Java block.
    pub state: BlockState,
    /// The value the map takes for a property a line leaves out, which a needed one left out does
    /// not get.
    pub defaults: IndexMap<String, String>,
    /// The properties no Bedrock state reads, which the entry is written the same at any value.
    pub unread: Vec<String>,
    /// The properties a line writes even at their default, since a line that leaves one out is not
    /// built as this block, as bare `redstone_torch` is a standing torch: the fewest that build it,
    /// the first in the block's order among as few, or past the sets searched every one left out.
    pub needed: Vec<String>,
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
        // With the water layer it is written with, when `line` is written as the entry.
        let written_as = |line: &BlockState| {
            let form = self.resolve(line).ok()?;
            (form.id == id && same(&form.states, states)).then_some(form.waterlogged)
        };
        let line = |spelled: &str, properties: IndexMap<String, String>| BlockState {
            id: spelled.to_owned(),
            properties,
        };
        for (_, java, family, full) in found {
            let ids: Vec<&str> = [java, id]
                .into_iter()
                .take(if java == id { 1 } else { 2 })
                .filter(|spelled| declares(spelled))
                .collect();
            if ids.is_empty() {
                continue;
            }
            let unread = unread(family);
            let left_out: Vec<&String> = full
                .iter()
                .filter(|(key, value)| {
                    unread.contains(*key) || family.defaults.get(*key) == Some(*value)
                })
                .map(|(key, _)| key)
                .collect();
            let n = left_out.len();
            let restorations = subsets(n)
                .take(1 << MOST_SEARCHED)
                .chain((n > MOST_SEARCHED).then(|| (0..n).collect()));
            for restored in restorations {
                let needed: Vec<String> = restored.iter().map(|&i| left_out[i].clone()).collect();
                let properties: IndexMap<String, String> = full
                    .iter()
                    .filter(|(key, _)| !left_out.contains(key) || needed.contains(*key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                for spelled in &ids {
                    let Some(waterlogged) = written_as(&line(spelled, properties.clone())) else {
                        continue;
                    };
                    return Some(CanonicalSpelling {
                        state: line(spelled, full),
                        defaults: family.defaults.clone(),
                        unread,
                        needed,
                        waterlogged,
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
            needed: Vec::new(),
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
    read.iter()
        .try_fold(1_usize, |n, key| n.checked_mul(family.java[*key].len()))
        .filter(|&n| n <= MOST_COMBINATIONS)?;
    let mut checks: Vec<Vec<Check<'_>>> = vec![Vec::new(); read.len()];
    for (name, rule) in &family.states {
        let wanted = states
            .iter()
            .find(|(state, _)| state == name)
            .map(|(_, v)| v)?;
        let first = match rule {
            Rule::Lookup { reads, .. } => {
                read.iter().position(|key| reads.iter().any(|r| r == key))
            }
            Rule::Constant(_) => None,
        };
        match first {
            Some(first) => checks[first].push(Check { rule, wanted }),
            None if rule.value(&base) == Some(wanted) => {}
            None => return None,
        }
    }
    if family.states.len() != states.len() {
        return None;
    }
    let mut search = Search {
        family,
        read: &read,
        checks: &checks,
        full: base,
        best: None,
    };
    search.set(read.len(), 0);
    search.best
}

/// A search for a family's whole state nearest its defaults whose Bedrock states are given; among
/// as near, the first in a count whose first read property moves fastest.
struct Search<'a> {
    family: &'a Family,
    read: &'a [&'a str],
    /// Per read property, the lookups it is the first read of, checked once it is set.
    checks: &'a [Vec<Check<'a>>],
    full: IndexMap<String, String>,
    best: Option<(usize, IndexMap<String, String>)>,
}

/// A lookup of a family's Bedrock state, with the value it must give.
#[derive(Clone, Copy)]
struct Check<'a> {
    rule: &'a Rule,
    wanted: &'a StateValue,
}

impl Search<'_> {
    /// Set each read property below `left`, `off` of those above it off their default, keeping a
    /// whole state with fewer off than the best so far.
    fn set(&mut self, left: usize, off: usize) {
        if self.best.as_ref().is_some_and(|(least, _)| off >= *least) {
            return;
        }
        let Some(at) = left.checked_sub(1) else {
            self.best = Some((off, self.full.clone()));
            return;
        };
        let key = self.read[at];
        for value in &self.family.java[key] {
            self.full.insert(key.to_owned(), value.clone());
            let holds = self.checks[at]
                .iter()
                .all(|check| check.rule.value(&self.full) == Some(check.wanted));
            if holds {
                let moved = usize::from(self.family.defaults.get(key) != Some(value));
                self.set(at, off + moved);
            }
        }
    }
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

/// Every set of the indices below `n`, fewest first, and in lexicographic order within a size.
fn subsets(n: usize) -> impl Iterator<Item = Vec<usize>> {
    (0..=n).flat_map(move |size| {
        let mut next = Some((0..size).collect::<Vec<usize>>());
        std::iter::from_fn(move || {
            let current = next.take()?;
            let mut following = current.clone();
            if let Some(i) = (0..size).rev().find(|&i| following[i] < n - size + i) {
                following[i] += 1;
                for j in i + 1..size {
                    following[j] = following[j - 1] + 1;
                }
                next = Some(following);
            }
            Some(current)
        })
    })
}

/// Whether `a` and `b` hold the same states, in any order.
fn same(a: &[(String, StateValue)], b: &[(String, StateValue)]) -> bool {
    a.len() == b.len() && a.iter().all(|pair| b.contains(pair))
}

#[cfg(test)]
mod tests;
