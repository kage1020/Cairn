//! Every whole state of every family the built-in Bedrock pack maps, at every version it maps,
//! read back by its canonical spelling.

use std::sync::OnceLock;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use indexmap::IndexMap;

use super::{CanonicalSpelling, subsets};
use crate::registry::builtin_pack;
use crate::registry::states::{BedrockForm, StateRefusal};

/// One entry a version's map writes for some family's whole state.
struct Entry {
    version: String,
    form: BedrockForm,
    /// Whether some line writes it: its canonical spelling's, or a whole state that writes it,
    /// spelled with its Java id or the entry's.
    spelled: bool,
    spelling: Option<CanonicalSpelling>,
}

/// Each entry the built-in Bedrock pack writes for a family's whole state, once per version, with
/// its canonical spelling there.
fn entries() -> &'static [Entry] {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let pack = builtin_pack(Edition::Bedrock);
        let index = pack
            .states()
            .expect("the built-in Bedrock pack maps states");
        let mut out = Vec::new();
        for (version, map) in &index.by_version {
            let states = index.at(version).expect("a version the index maps");
            let view = pack.view(Some(version));
            let ids = view.block_ids().expect("a pinned id table");
            // Per entry, each whole state that writes it, spelled with its Java id and the entry's.
            let mut written: IndexMap<String, (BedrockForm, Vec<[BlockState; 2]>)> =
                IndexMap::new();
            for (java, forms) in &map.from {
                for (bedrock, at) in forms {
                    let family = &index.families[*at];
                    for full in whole_states(&family.java) {
                        let form = states.form(bedrock, java, family, &full, &full);
                        let lines = [java, bedrock].map(|id| BlockState {
                            id: id.clone(),
                            properties: full.clone(),
                        });
                        written
                            .entry(key(&form))
                            .or_insert((form, Vec::new()))
                            .1
                            .push(lines);
                    }
                }
            }
            for (form, lines) in written.into_values() {
                let spelling =
                    states.canonical_spelling(&form.id, &form.states, form.waterlogged, |id| {
                        ids.contains(id)
                    });
                let spelled = spelling.is_some()
                    || lines
                        .iter()
                        .flatten()
                        .any(|line| ids.contains(&line.id) && writes(&states.resolve(line), &form));
                out.push(Entry {
                    version: version.clone(),
                    form,
                    spelled,
                    spelling,
                });
            }
        }
        out
    })
}

/// Every whole state of a block with the properties `java`, each holding the values given.
fn whole_states(java: &IndexMap<String, Vec<String>>) -> Vec<IndexMap<String, String>> {
    let mut out = vec![IndexMap::new()];
    for (key, held) in java {
        out = out
            .into_iter()
            .flat_map(|state| {
                held.iter().map(move |value| {
                    let mut next = state.clone();
                    next.insert(key.clone(), value.clone());
                    next
                })
            })
            .collect();
    }
    out
}

/// `form` as one entry is told from another: its id, its states sorted and its water layer.
fn key(form: &BedrockForm) -> String {
    let mut states = form.states.clone();
    states.sort_by(|(a, _), (b, _)| a.cmp(b));
    format!("{}{states:?} water={}", form.id, form.waterlogged)
}

/// Whether `resolved` is the entry `form`.
fn writes(resolved: &Result<BedrockForm, StateRefusal>, form: &BedrockForm) -> bool {
    resolved.as_ref().is_ok_and(|got| key(got) == key(form))
}

/// `spelling` with `properties` of its state alone.
fn spelled_with(spelling: &CanonicalSpelling, properties: &[&String]) -> BlockState {
    BlockState {
        id: spelling.state.id.clone(),
        properties: spelling
            .state
            .properties
            .iter()
            .filter(|(key, _)| properties.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    }
}

/// The properties of `spelling` a line writes: each needed one, and each other read and off its
/// default.
fn written_keys(spelling: &CanonicalSpelling) -> Vec<&String> {
    spelling
        .state
        .properties
        .iter()
        .filter(|(key, value)| {
            spelling.needed.contains(key)
                || (!spelling.unread.contains(key) && spelling.defaults.get(*key) != Some(value))
        })
        .map(|(key, _)| key)
        .collect()
}

#[test]
fn every_entry_a_line_writes_has_a_canonical_spelling_whose_line_writes_it_again() {
    let pack = builtin_pack(Edition::Bedrock);
    let mut failures = Vec::new();
    for entry in entries() {
        let states = pack.states_at(&entry.version).expect("a version it maps");
        let label = format!("{} {}", entry.version, key(&entry.form));
        let Some(spelling) = &entry.spelling else {
            if entry.spelled {
                failures.push(format!(
                    "{label}: no canonical spelling, though a line writes it"
                ));
            }
            continue;
        };
        let line = spelled_with(spelling, &written_keys(spelling));
        for written in [&line, &spelling.state] {
            let resolved = states.resolve(written);
            if !writes(&resolved, &entry.form) {
                failures.push(format!("{label}: {written:?} writes {resolved:?}"));
            }
        }
    }
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn a_line_that_leaves_out_a_needed_state_writes_another_block() {
    let pack = builtin_pack(Edition::Bedrock);
    let mut failures = Vec::new();
    let mut needing = 0;
    for entry in entries() {
        let Some(spelling) = entry.spelling.as_ref().filter(|s| !s.needed.is_empty()) else {
            continue;
        };
        needing += 1;
        let states = pack.states_at(&entry.version).expect("a version it maps");
        let written = written_keys(spelling);
        let kept: Vec<&String> = written
            .iter()
            .copied()
            .filter(|key| !spelling.needed.contains(key))
            .collect();
        let rest: Vec<&String> = spelling
            .state
            .properties
            .keys()
            .filter(|key| !kept.contains(key))
            .collect();
        for chosen in subsets(rest.len()) {
            let chosen: Vec<&String> = chosen.iter().map(|&i| rest[i]).collect();
            if spelling.needed.iter().all(|key| chosen.contains(&key)) {
                continue;
            }
            let line = spelled_with(spelling, &[kept.as_slice(), &chosen].concat());
            if writes(&states.resolve(&line), &entry.form) {
                failures.push(format!(
                    "{} {}: {line:?} writes it without all of {:?}",
                    entry.version,
                    key(&entry.form),
                    spelling.needed
                ));
            }
        }
    }
    assert!(needing > 0, "no entry needs a state at its default");
    assert_eq!(failures, Vec::<String>::new());
}
