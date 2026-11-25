//! The canonical spelling of a Bedrock entry: which states a line needs, in hand-built packs, and
//! every entry the built-in Bedrock pack writes for a family's whole state, at every version it
//! maps, read back by its spelling.

use std::sync::OnceLock;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use indexmap::IndexMap;
use serde_json::json;

use super::{CanonicalSpelling, MOST_SEARCHED, subsets};
use crate::registry::blocks::{BlocksCatalog, BlocksIndex, StateValue};
use crate::registry::builtin_pack;
use crate::registry::states::{
    BedrockForm, ConstantRow, FamilyRow, RuleRow, SensitivityCatalog, StateMapIndex, StateRefusal,
    StatesCatalog,
};

/// One entry a version's map writes for some family's whole state.
struct Entry {
    version: String,
    form: BedrockForm,
    spelling: Option<CanonicalSpelling>,
    /// Whether it has no spelling though the line of a whole state the map writes as it, spelled
    /// whole with an id the version declares, its Java block's or the entry's, writes it.
    unspelled_but_written: bool,
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
                let unspelled_but_written = spelling.is_none()
                    && lines
                        .iter()
                        .flatten()
                        .any(|line| ids.contains(&line.id) && writes(&states.resolve(line), &form));
                out.push(Entry {
                    version: version.clone(),
                    form,
                    spelling,
                    unspelled_but_written,
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
#[ignore = "slow: every entry of the built-in Bedrock pack at every version"]
fn every_entry_a_whole_line_writes_has_a_canonical_spelling_whose_line_writes_it() {
    let pack = builtin_pack(Edition::Bedrock);
    let mut failures = Vec::new();
    let mut spelled = 0;
    for entry in entries() {
        let label = format!("{} {}", entry.version, key(&entry.form));
        if entry.unspelled_but_written {
            failures.push(format!(
                "{label}: no canonical spelling, though a whole line writes it"
            ));
        }
        let Some(spelling) = &entry.spelling else {
            continue;
        };
        spelled += 1;
        let states = pack.states_at(&entry.version).expect("a version it maps");
        let line = spelled_with(spelling, &written_keys(spelling));
        let resolved = states.resolve(&line);
        if !writes(&resolved, &entry.form) {
            failures.push(format!("{label}: {line:?} writes {resolved:?}"));
        }
    }
    assert!(spelled > 0, "no entry has a canonical spelling");
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
#[ignore = "slow: every entry of the built-in Bedrock pack at every version"]
fn the_needed_states_are_the_first_of_the_fewest_whose_line_writes_the_entry() {
    let pack = builtin_pack(Edition::Bedrock);
    let mut failures = Vec::new();
    let mut needing = 0;
    for entry in entries() {
        let Some(spelling) = entry.spelling.as_ref().filter(|s| !s.needed.is_empty()) else {
            continue;
        };
        needing += 1;
        let states = pack.states_at(&entry.version).expect("a version it maps");
        let kept: Vec<&String> = written_keys(spelling)
            .into_iter()
            .filter(|key| !spelling.needed.contains(key))
            .collect();
        let left_out: Vec<&String> = spelling
            .state
            .properties
            .keys()
            .filter(|key| !kept.contains(key))
            .collect();
        let needed: Vec<usize> = (0..left_out.len())
            .filter(|&i| spelling.needed.contains(left_out[i]))
            .collect();
        let earlier = subsets(left_out.len())
            .take(1 << MOST_SEARCHED)
            .take_while(|set| *set != needed);
        for set in earlier {
            let chosen: Vec<&String> = set.iter().map(|&i| left_out[i]).collect();
            let line = spelled_with(spelling, &[kept.as_slice(), &chosen].concat());
            if writes(&states.resolve(&line), &entry.form) {
                failures.push(format!(
                    "{} {}: {line:?} writes it before the needed {:?}",
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

/// A pack mapping each Java block of `blocks` onto Bedrock's `post` at 1.0, given as its id, the
/// `kind` it is written at, and its properties in order, each holding only `a`.
fn posts(blocks: &[(&str, i64, Vec<&str>)]) -> StateMapIndex {
    let families = blocks
        .iter()
        .map(|(java, kind, properties)| FamilyRow {
            mc_versions: vec!["1.0".to_owned()],
            ids: vec![if *java == "post" {
                "post".to_owned()
            } else {
                format!("post={java}")
            }],
            java: properties
                .iter()
                .map(|p| ((*p).to_owned(), vec!["a".to_owned()]))
                .collect(),
            defaults: properties
                .iter()
                .map(|p| ((*p).to_owned(), "a".to_owned()))
                .collect(),
            states: IndexMap::from([(
                "kind".to_owned(),
                RuleRow::Constant(ConstantRow {
                    value: StateValue::Int(*kind),
                }),
            )]),
            dropped: Vec::new(),
        })
        .collect();
    let states = StatesCatalog {
        schema_version: 1,
        namespace: "minecraft".to_owned(),
        refused: IndexMap::new(),
        in_water: Vec::new(),
        families,
    };
    let sensitivity = SensitivityCatalog {
        schema_version: 1,
        namespace: "minecraft".to_owned(),
        dropped_states: IndexMap::new(),
    };
    let blocks: BlocksCatalog = serde_json::from_value(json!({
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": { "post": { "kind": [0, 1] } } }
    }))
    .expect("the fixture's blocks parse");
    let blocks =
        BlocksIndex::from_catalog(blocks, Edition::Bedrock).expect("the fixture's blocks fold");
    StateMapIndex::from_catalogs(states, sensitivity, &blocks).expect("the fixture loads")
}

/// The canonical spelling of `post` at `kind` 1 in `index`, which declares no other id.
fn post_spelling(index: &StateMapIndex) -> Option<CanonicalSpelling> {
    index
        .at("1.0")
        .expect("the fixture maps 1.0")
        .canonical_spelling(
            "minecraft:post",
            &[("kind".to_owned(), StateValue::Int(1))],
            false,
            |id| id == "minecraft:post",
        )
}

/// `minecraft:post` with each of `keys` at `a`, and those defaults.
fn post_at_a(keys: &[&str]) -> (BlockState, IndexMap<String, String>) {
    let properties: IndexMap<String, String> = keys
        .iter()
        .map(|key| ((*key).to_owned(), "a".to_owned()))
        .collect();
    let state = BlockState {
        id: "minecraft:post".to_owned(),
        properties: properties.clone(),
    };
    (state, properties)
}

fn names(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

#[test]
fn of_states_that_each_pick_the_block_alone_the_first_in_its_order_is_needed() {
    let index = posts(&[("post", 0, vec![]), ("tall_post", 1, vec!["q", "p"])]);
    let (state, defaults) = post_at_a(&["q", "p"]);
    assert_eq!(
        post_spelling(&index),
        Some(CanonicalSpelling {
            state,
            defaults,
            unread: names(&["q", "p"]),
            needed: names(&["q"]),
            waterlogged: false,
        })
    );
}

#[test]
fn past_the_sets_searched_a_line_needs_every_state_it_leaves_out() {
    let keys: Vec<String> = (0..13).map(|i| format!("p{i}")).collect();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let shorts: Vec<String> = (0..7).map(|k| format!("short_post_{k}")).collect();
    // Each short post lacks one of `p0` to `p6`, so only a line naming all seven is the tall post,
    // and the 4096 sets searched of thirteen properties are those of at most six.
    let mut blocks = vec![("tall_post", 1, keys.clone())];
    for (k, short) in shorts.iter().enumerate() {
        let mut lacking = keys.clone();
        lacking.remove(k);
        blocks.push((short, 0, lacking));
    }
    let (state, defaults) = post_at_a(&keys);
    assert_eq!(
        post_spelling(&posts(&blocks)),
        Some(CanonicalSpelling {
            state,
            defaults,
            unread: names(&keys),
            needed: names(&keys),
            waterlogged: false,
        })
    );
}
