//! The `states` and `sensitivity` components of a Bedrock registry pack: each version's map from
//! the canonical vocabulary's block states, which are Java's, to the edition's own, and what a
//! Java state no Bedrock state carries costs when it is dropped (`spec/versioning-editions`
//! "Folding the `(edition, version)` matrix").

use std::collections::{HashMap, HashSet};

use cairn_lang_core::block_array::BlockState;
use indexmap::IndexMap;
use serde::Deserialize;

use super::blocks::{BlockStates, BlocksIndex, StateValue};
use super::namespaced;

/// The highest `states.schema_version` this build reads.
pub const SUPPORTED_STATES_SCHEMA: u32 = 1;
/// The highest `sensitivity.schema_version` this build reads.
pub const SUPPORTED_SENSITIVITY_SCHEMA: u32 = 1;

/// The Java state that says a block holds water, which the `.mcstructure` writer carries as the
/// water layer rather than as a state.
pub const WATER_LAYER_STATE: &str = "waterlogged";

/// The body of `states.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatesCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace of every id the catalog names.
    pub namespace: String,
    /// Per version, the pairs whose Bedrock states that version's palette does not declare.
    #[serde(default)]
    pub refused: IndexMap<String, Vec<String>>,
    /// The Java blocks that always stand in water, though they have no `waterlogged` state.
    #[serde(default)]
    pub in_water: Vec<String>,
    /// The families, each with the versions and pairs it maps.
    pub families: Vec<FamilyRow>,
}

/// One row of [`StatesCatalog::families`]: the Java states of one block that become one Bedrock
/// block, and how their states map.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyRow {
    /// The versions the row maps.
    pub mc_versions: Vec<String>,
    /// The pairs it maps: a Bedrock id, `=` the Java id when the two are spelled apart.
    pub ids: Vec<String>,
    /// Every property of the Java block, with the values the family holds, in Java's order.
    pub java: IndexMap<String, Vec<String>>,
    /// The Java block's default state.
    pub defaults: IndexMap<String, String>,
    /// Each Bedrock state the family writes, and the rule that sets it.
    pub states: IndexMap<String, RuleRow>,
    /// The properties held at more than one value that no Bedrock state reads, but the water
    /// layer's.
    #[serde(default)]
    pub dropped: Vec<String>,
}

/// How a [`FamilyRow`] sets one Bedrock state.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RuleRow {
    /// Looked up from the Java values a [`LookupRow`] reads.
    Lookup(LookupRow),
    /// The same value for every state of the family.
    Constant(ConstantRow),
}

/// A [`RuleRow::Lookup`]: the Bedrock value for each combination of the Java values it reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupRow {
    /// The Java properties read, in order.
    pub reads: Vec<String>,
    /// Their values, comma-joined, to the Bedrock value.
    pub map: IndexMap<String, StateValue>,
}

/// A [`RuleRow::Constant`]: one Bedrock value for every state of the family.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstantRow {
    /// The value.
    pub value: StateValue,
}

/// The body of `sensitivity.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace of every Java id the catalog names.
    pub namespace: String,
    /// Per Java property some family drops, what dropping it costs.
    pub dropped_states: IndexMap<String, Vec<SensitivityRow>>,
}

/// One classification of a dropped Java property, for the blocks it names or for every other.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityRow {
    /// The Java blocks the row covers; without it, every block no other row of the key names.
    #[serde(default)]
    pub java: Option<Vec<String>>,
    /// What dropping it costs.
    pub kind: SensitivityKind,
    /// Why, as `W_INTENT_DEGRADED` prints it.
    pub reason: String,
}

/// What dropping a Java property costs on Bedrock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityKind {
    /// Bedrock does not carry it, so a value other than the Java default is lost.
    MissingState,
    /// Bedrock works it out itself, so nothing is lost.
    Derived,
}

/// The validated state maps of a Bedrock pack.
#[derive(Debug, Clone, Default)]
pub struct StateMapIndex {
    families: Vec<Family>,
    by_version: IndexMap<String, VersionMap>,
    sensitivity: HashMap<String, KeyRows>,
    in_water: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Family {
    java: IndexMap<String, Vec<String>>,
    defaults: IndexMap<String, String>,
    states: Vec<(String, Rule)>,
    dropped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    Lookup {
        reads: Vec<String>,
        map: HashMap<String, StateValue>,
    },
    Constant(StateValue),
}

/// One version's pairs, both ways, with the ids of the pairs its palette refused.
#[derive(Debug, Clone, Default)]
struct VersionMap {
    /// Bedrock id to each Java block mapped onto it, with the family.
    onto: HashMap<String, Vec<(String, usize)>>,
    /// Java id to each Bedrock block it maps to, with the family.
    from: HashMap<String, Vec<(String, usize)>>,
    refused_onto: HashSet<String>,
    refused_from: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct KeyRows {
    named: HashMap<String, (SensitivityKind, String)>,
    other: Option<(SensitivityKind, String)>,
}

impl KeyRows {
    fn row(&self, java_id: &str) -> Option<&(SensitivityKind, String)> {
        self.named.get(java_id).or(self.other.as_ref())
    }
}

/// Why a state map component does not validate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatesError {
    /// A component's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack {component} schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// `"states"` or `"sensitivity"`.
        component: &'static str,
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The pack has one of the two components without the other.
    #[error("registry pack names `{present}` without `{missing}`; a state map needs both")]
    Unpaired {
        /// The component named.
        present: &'static str,
        /// The component missing.
        missing: &'static str,
    },
    /// A row or `refused` names a version the pack's block tables do not describe.
    #[error("registry pack `states` maps `{mc_version}`, which `blocks` has no table for")]
    UnknownVersion {
        /// The version.
        mc_version: String,
    },
    /// A pair's Bedrock id is not in its version's block table.
    #[error(
        "registry pack `states` maps onto `{id}`, which `blocks` does not declare at `{mc_version}`"
    )]
    UnknownBlock {
        /// The version.
        mc_version: String,
        /// The Bedrock id.
        id: String,
    },
    /// Two rows map one pair at one version.
    #[error("registry pack `states` maps `{pair}` twice at `{mc_version}`")]
    DuplicatePair {
        /// The version.
        mc_version: String,
        /// The pair, as `ids` names it.
        pair: String,
    },
    /// A pair is both mapped and refused at one version.
    #[error("registry pack `states` both maps and refuses `{pair}` at `{mc_version}`")]
    RefusedAndMapped {
        /// The version.
        mc_version: String,
        /// The pair, as `refused` names it.
        pair: String,
    },
    /// A family is malformed: it maps no pair or version, a property holds no value, an int is
    /// past `TAG_Int`, or a rule, its defaults or what it drops disagree with its properties.
    #[error("registry pack `states` family of `{pair}`: {problem}")]
    MalformedFamily {
        /// The family's first pair, or its place in `families` when it names none.
        pair: String,
        /// What is wrong.
        problem: String,
    },
    /// A family leaves out a state its Bedrock block declares.
    #[error(
        "registry pack `states` maps onto `{id}` without writing its `{state}`, which `blocks` declares at `{mc_version}`"
    )]
    StateUnwritten {
        /// The version.
        mc_version: String,
        /// The Bedrock id.
        id: String,
        /// The state.
        state: String,
    },
    /// A family writes a Bedrock state or value its block does not declare.
    #[error(
        "registry pack `states` writes `{state}={value}` on `{id}`, which `blocks` does not declare at `{mc_version}`"
    )]
    OutsidePalette {
        /// The version.
        mc_version: String,
        /// The Bedrock id.
        id: String,
        /// The state.
        state: String,
        /// The value.
        value: String,
    },
    /// Two families of one Java block hold one of its states at one version.
    #[error(
        "registry pack `states` maps a state of `{java}` both onto `{first}` and onto `{second}` at `{mc_version}`"
    )]
    Overlap {
        /// The version.
        mc_version: String,
        /// The Java id.
        java: String,
        /// One Bedrock id.
        first: String,
        /// The other.
        second: String,
    },
    /// Two families of one Java block at one version give it different properties or defaults.
    #[error(
        "registry pack `states` maps `{java}` onto `{first}` and onto `{second}` at `{mc_version}` with different properties or defaults"
    )]
    Disagree {
        /// The version.
        mc_version: String,
        /// The Java id.
        java: String,
        /// One Bedrock id.
        first: String,
        /// The other.
        second: String,
    },
    /// An `in_water` block no family maps.
    #[error("registry pack `states` puts `{java}` in water, but no family maps it")]
    InWaterUnmapped {
        /// The Java id.
        java: String,
    },
    /// A dropped property `sensitivity` gives no classification for.
    #[error(
        "registry pack `sensitivity` does not classify `{key}` on `{java}`, which `states` drops"
    )]
    Unclassified {
        /// The property.
        key: String,
        /// The Java id.
        java: String,
    },
    /// A property `sensitivity` classifies twice for one block, or with two unnamed rows.
    #[error("registry pack `sensitivity` classifies `{key}` twice for one block")]
    ClassifiedTwice {
        /// The property.
        key: String,
    },
}

/// A pair name of `ids` or `refused`, split into its Bedrock and Java ids.
fn split_pair(namespace: &str, pair: &str) -> (String, String) {
    let (bedrock, java) = pair.split_once('=').unwrap_or((pair, pair));
    (namespaced(namespace, bedrock), namespaced(namespace, java))
}

fn malformed(pair: &str, problem: String) -> StatesError {
    StatesError::MalformedFamily {
        pair: pair.to_owned(),
        problem,
    }
}

impl StateMapIndex {
    /// Validates `states` and `sensitivity` against the pack's block tables into an index.
    ///
    /// # Errors
    ///
    /// [`StatesError`] for the first schema, pair, family, block or dropped property that fails.
    pub fn from_catalogs(
        states: StatesCatalog,
        sensitivity: SensitivityCatalog,
        blocks: &BlocksIndex,
    ) -> Result<Self, StatesError> {
        if states.schema_version > SUPPORTED_STATES_SCHEMA {
            return Err(StatesError::UnsupportedSchemaVersion {
                component: "states",
                got: states.schema_version,
                supported: SUPPORTED_STATES_SCHEMA,
            });
        }
        if sensitivity.schema_version > SUPPORTED_SENSITIVITY_SCHEMA {
            return Err(StatesError::UnsupportedSchemaVersion {
                component: "sensitivity",
                got: sensitivity.schema_version,
                supported: SUPPORTED_SENSITIVITY_SCHEMA,
            });
        }
        let mut index = Self {
            sensitivity: key_rows(sensitivity)?,
            ..Self::default()
        };
        let namespace = states.namespace.as_str();
        let known = |version: &str| -> Result<(), StatesError> {
            match blocks.ids_for(version) {
                Some(_) => Ok(()),
                None => Err(StatesError::UnknownVersion {
                    mc_version: version.to_owned(),
                }),
            }
        };
        for (place, row) in states.families.into_iter().enumerate() {
            let first = row
                .ids
                .first()
                .cloned()
                .unwrap_or_else(|| format!("families[{place}]"));
            if row.ids.is_empty() || row.mc_versions.is_empty() {
                return Err(malformed(
                    &first,
                    "it maps no pair or no version".to_owned(),
                ));
            }
            let family = family_of(&first, row.java, row.defaults, row.states, row.dropped)?;
            let at = index.families.len();
            for version in &row.mc_versions {
                known(version)?;
                let map = index.by_version.entry(version.clone()).or_default();
                for pair in &row.ids {
                    let (bedrock, java) = split_pair(namespace, pair);
                    check_palette(blocks, version, &bedrock, &family)?;
                    let onto = map.onto.entry(bedrock.clone()).or_default();
                    if onto.iter().any(|(seen, _)| *seen == java) {
                        return Err(StatesError::DuplicatePair {
                            mc_version: version.clone(),
                            pair: pair.clone(),
                        });
                    }
                    onto.push((java.clone(), at));
                    map.from.entry(java).or_default().push((bedrock, at));
                }
            }
            index.families.push(family);
        }
        for (version, pairs) in states.refused {
            known(&version)?;
            let map = index.by_version.entry(version.clone()).or_default();
            for pair in &pairs {
                // Unchecked against `blocks`: a pair is often refused for a Bedrock id its
                // version's palette no longer declares.
                let (bedrock, java) = split_pair(namespace, pair);
                let mapped = map
                    .onto
                    .get(&bedrock)
                    .is_some_and(|forms| forms.iter().any(|(seen, _)| *seen == java));
                if mapped {
                    return Err(StatesError::RefusedAndMapped {
                        mc_version: version,
                        pair: pair.clone(),
                    });
                }
                map.refused_onto.insert(bedrock.clone());
                map.refused_from.insert(java.clone());
            }
        }
        for java in &states.in_water {
            let java = namespaced(namespace, java);
            if !index
                .by_version
                .values()
                .any(|map| map.from.contains_key(&java))
            {
                return Err(StatesError::InWaterUnmapped { java });
            }
            index.in_water.insert(java);
        }
        let order: HashMap<&str, usize> =
            blocks.versions().enumerate().map(|(i, v)| (v, i)).collect();
        index
            .by_version
            .sort_by(|a, _, b, _| order[a.as_str()].cmp(&order[b.as_str()]));
        index.check_partitions()?;
        index.check_classified()?;
        Ok(index)
    }

    /// The map of `mc_version`, or `None` when the pack carries none for it.
    #[must_use]
    pub fn at<'a>(&'a self, mc_version: &'a str) -> Option<VersionStates<'a>> {
        self.by_version.get(mc_version).map(|map| VersionStates {
            index: self,
            mc_version,
            map,
        })
    }

    /// Every version the index maps, oldest first as the block tables order them.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// The versions whose map writes `state` as a form of a block it maps, oldest first.
    pub fn versions_writing<'a>(&'a self, state: &'a BlockState) -> impl Iterator<Item = &'a str> {
        self.by_version.iter().filter_map(move |(version, map)| {
            let names = map.onto.contains_key(&state.id) || map.from.contains_key(&state.id);
            let states = VersionStates {
                index: self,
                mc_version: version,
                map,
            };
            (names && states.resolve(state).is_ok()).then_some(version.as_str())
        })
    }

    /// The versions, but `skip`, whose map holds `key` on the Java block `java`, at `value` when
    /// one is given, oldest first.
    fn versions_holding(
        &self,
        skip: &str,
        java: &str,
        key: &str,
        value: Option<&str>,
    ) -> Vec<&str> {
        self.by_version
            .iter()
            .filter(|(version, map)| {
                *version != skip
                    && map.from.get(java).is_some_and(|forms| {
                        forms.iter().any(|(_, at)| {
                            self.families[*at].java.get(key).is_some_and(|held| {
                                value.is_none_or(|value| held.iter().any(|h| h == value))
                            })
                        })
                    })
            })
            .map(|(version, _)| version.as_str())
            .collect()
    }

    /// Refuses two families of one Java block that disagree on its properties or defaults, or that
    /// hold one state, which would make the family an entry belongs to depend on reading order.
    fn check_partitions(&self) -> Result<(), StatesError> {
        for (version, map) in &self.by_version {
            for (java, forms) in &map.from {
                for (i, (first, a)) in forms.iter().enumerate() {
                    for (second, b) in &forms[i + 1..] {
                        let (a, b) = (&self.families[*a], &self.families[*b]);
                        let same_keys = a.java.len() == b.java.len()
                            && a.java.keys().all(|key| b.java.contains_key(key));
                        if !same_keys || a.defaults != b.defaults {
                            return Err(StatesError::Disagree {
                                mc_version: version.clone(),
                                java: java.clone(),
                                first: first.clone(),
                                second: second.clone(),
                            });
                        }
                        let apart = a.java.iter().any(|(key, held)| {
                            b.java
                                .get(key)
                                .is_some_and(|other| held.iter().all(|v| !other.contains(v)))
                        });
                        if !apart {
                            return Err(StatesError::Overlap {
                                mc_version: version.clone(),
                                java: java.clone(),
                                first: first.clone(),
                                second: second.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Refuses a dropped property no `sensitivity` row covers for its Java block.
    fn check_classified(&self) -> Result<(), StatesError> {
        for map in self.by_version.values() {
            for (java, forms) in &map.from {
                for (_, at) in forms {
                    for key in &self.families[*at].dropped {
                        if self
                            .sensitivity
                            .get(key)
                            .and_then(|rows| rows.row(java))
                            .is_none()
                        {
                            return Err(StatesError::Unclassified {
                                key: key.clone(),
                                java: java.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn key_rows(sensitivity: SensitivityCatalog) -> Result<HashMap<String, KeyRows>, StatesError> {
    let namespace = sensitivity.namespace;
    let mut by_key = HashMap::with_capacity(sensitivity.dropped_states.len());
    for (key, rows) in sensitivity.dropped_states {
        let mut classified = KeyRows::default();
        for row in rows {
            let cost = (row.kind, row.reason);
            let twice = match row.java {
                Some(java) => java.into_iter().any(|id| {
                    classified
                        .named
                        .insert(namespaced(&namespace, &id), cost.clone())
                        .is_some()
                }),
                None => classified.other.replace(cost).is_some(),
            };
            if twice {
                return Err(StatesError::ClassifiedTwice { key });
            }
        }
        by_key.insert(key, classified);
    }
    Ok(by_key)
}

fn family_of(
    pair: &str,
    java: IndexMap<String, Vec<String>>,
    defaults: IndexMap<String, String>,
    states: IndexMap<String, RuleRow>,
    dropped: Vec<String>,
) -> Result<Family, StatesError> {
    if java.values().any(Vec::is_empty) {
        return Err(malformed(pair, "a property holds no value".to_owned()));
    }
    if defaults.len() != java.len() || defaults.keys().any(|key| !java.contains_key(key)) {
        return Err(malformed(
            pair,
            "`defaults` does not name exactly the block's properties".to_owned(),
        ));
    }
    if let Some(key) = dropped.iter().find(|key| !java.contains_key(*key)) {
        return Err(malformed(
            pair,
            format!("it drops `{key}`, which it does not hold"),
        ));
    }
    let mut rules = Vec::with_capacity(states.len());
    for (state, rule) in states {
        let values: Vec<&StateValue> = match &rule {
            RuleRow::Constant(row) => vec![&row.value],
            RuleRow::Lookup(row) => row.map.values().collect(),
        };
        if values
            .iter()
            .any(|value| matches!(value, StateValue::Int(n) if i32::try_from(*n).is_err()))
        {
            return Err(malformed(
                pair,
                format!("`{state}` holds an int past `TAG_Int`"),
            ));
        }
        let rule = match rule {
            RuleRow::Constant(ConstantRow { value }) => Rule::Constant(value),
            RuleRow::Lookup(LookupRow { reads, map }) => {
                let combinations = combinations(&java, &reads).ok_or_else(|| {
                    malformed(pair, format!("`{state}` reads a property it does not hold"))
                })?;
                if map.len() != combinations.len()
                    || combinations.iter().any(|c| !map.contains_key(c))
                {
                    return Err(malformed(
                        pair,
                        format!("`{state}` does not map exactly the values its reads hold"),
                    ));
                }
                Rule::Lookup {
                    reads,
                    map: map.into_iter().collect(),
                }
            }
        };
        rules.push((state, rule));
    }
    let read = |key: &str| {
        rules.iter().any(|(_, rule)| match rule {
            Rule::Lookup { reads, .. } => reads.iter().any(|r| r == key),
            Rule::Constant(_) => false,
        })
    };
    if let Some(key) = dropped.iter().find(|key| read(key)) {
        return Err(malformed(
            pair,
            format!("it drops `{key}`, which a state reads"),
        ));
    }
    let lost = java.iter().find(|(key, held)| {
        held.len() > 1 && *key != WATER_LAYER_STATE && !read(key) && !dropped.contains(key)
    });
    if let Some((key, _)) = lost {
        return Err(malformed(
            pair,
            format!(
                "it holds `{key}` at several values, which no state reads and it does not drop"
            ),
        ));
    }
    Ok(Family {
        java,
        defaults,
        states: rules,
        dropped,
    })
}

/// Every comma-joined combination of the values `reads` hold in `java`, or `None` when it reads a
/// property `java` lacks.
fn combinations(java: &IndexMap<String, Vec<String>>, reads: &[String]) -> Option<Vec<String>> {
    let mut joined = vec![String::new()];
    for (i, key) in reads.iter().enumerate() {
        let held = java.get(key)?;
        joined = joined
            .iter()
            .flat_map(|prefix| {
                held.iter().map(move |value| {
                    if i == 0 {
                        value.clone()
                    } else {
                        format!("{prefix},{value}")
                    }
                })
            })
            .collect();
    }
    Some(joined)
}

/// Whether the block tables declare `bedrock` at `version`.
fn declares(blocks: &BlocksIndex, version: &str, bedrock: &str) -> bool {
    blocks
        .ids_for(version)
        .is_some_and(|ids| ids.binary_search_by(|id| id.as_str().cmp(bedrock)).is_ok())
}

/// Refuses a family that writes a state or value the Bedrock block does not declare at `version`,
/// or leaves out a state it declares.
fn check_palette(
    blocks: &BlocksIndex,
    version: &str,
    bedrock: &str,
    family: &Family,
) -> Result<(), StatesError> {
    if !declares(blocks, version, bedrock) {
        return Err(StatesError::UnknownBlock {
            mc_version: version.to_owned(),
            id: bedrock.to_owned(),
        });
    }
    let domains = blocks.states_of(version, bedrock);
    let unwritten = domains
        .into_iter()
        .flat_map(BlockStates::properties)
        .map(|(name, _)| name)
        .find(|name| !family.states.iter().any(|(state, _)| state == name));
    if let Some(state) = unwritten {
        return Err(StatesError::StateUnwritten {
            mc_version: version.to_owned(),
            id: bedrock.to_owned(),
            state: state.to_owned(),
        });
    }
    for (state, rule) in &family.states {
        let values: Vec<&StateValue> = match rule {
            Rule::Constant(value) => vec![value],
            Rule::Lookup { map, .. } => map.values().collect(),
        };
        let domain = domains.and_then(|domains| domains.domain(state));
        for value in values {
            if !domain.is_some_and(|domain| domain.contains(value)) {
                return Err(StatesError::OutsidePalette {
                    mc_version: version.to_owned(),
                    id: bedrock.to_owned(),
                    state: state.clone(),
                    value: value.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// One version's state map, as [`StateMapIndex::at`] returns it.
#[derive(Debug, Clone, Copy)]
pub struct VersionStates<'a> {
    index: &'a StateMapIndex,
    mc_version: &'a str,
    map: &'a VersionMap,
}

/// What a palette entry is written as on Bedrock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockForm {
    /// The Bedrock id, which a state can spell as another block (`oak_slab[type=double]` is
    /// `oak_double_slab`).
    pub id: String,
    /// Each Bedrock state the family writes, by name; empty for an entry written bare.
    pub states: Vec<(String, StateValue)>,
    /// Whether the cell also holds water, which the writer carries as the water layer.
    pub waterlogged: bool,
    /// Each state the entry asks for that Bedrock does not carry.
    pub dropped: Vec<DroppedState>,
}

/// One Java state value a [`BedrockForm`] does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedState {
    /// The property.
    pub key: String,
    /// The value asked for.
    pub value: String,
    /// Why it is lost, from the pack's `sensitivity`.
    pub reason: String,
}

/// Why a palette entry has no [`BedrockForm`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateRefusal {
    /// The version's map has no form for the entry's states; `reason` says why.
    Unmapped {
        /// Why, as a clause.
        reason: String,
    },
    /// The entry carries a property its Java block does not have.
    UnknownProperty {
        /// The property.
        key: String,
        /// The block's properties.
        valid: Vec<String>,
    },
    /// The entry carries a value outside its Java property's values.
    UnknownValue {
        /// The property.
        key: String,
        /// The value.
        value: String,
        /// The values the property takes.
        valid: Vec<String>,
    },
}

impl<'a> VersionStates<'a> {
    /// The canonical blocks `id` with the states `given` names here: its own Java block, or each
    /// Java block mapped onto it that takes `given`, as [`Self::resolve`] narrows them.
    #[must_use]
    pub fn java_ids<'s>(&self, id: &'s str, given: &IndexMap<String, String>) -> Vec<&'s str>
    where
        'a: 's,
    {
        if self.reads_as_own(id, given) {
            return vec![id];
        }
        let mut ids: Vec<&'s str> = Vec::new();
        for (java, at) in self.map.onto.get(id).map_or(&[][..], Vec::as_slice) {
            if self.index.families[*at].takes(given) && !ids.contains(&java.as_str()) {
                ids.push(java);
            }
        }
        ids
    }

    /// The Bedrock id that writes `state` here when no family of its own id holds its states but
    /// one of the Java block its properties name does, as `redstone_torch[facing=north,lit=false]`
    /// is the unlit wall torch, `unlit_redstone_torch`; `None` otherwise.
    #[must_use]
    pub fn spelling(&self, state: &BlockState) -> Option<&'a str> {
        if self.resolve(state).is_ok() {
            return None;
        }
        let given = &state.properties;
        let map: &'a VersionMap = self.map;
        let mut named = map
            .onto
            .get(state.id.as_str())?
            .iter()
            .filter(|(_, at)| {
                given
                    .keys()
                    .all(|key| self.family(*at).java.contains_key(key))
            })
            .map(|(java, _)| java.as_str());
        let java = named.next()?;
        if named.any(|other| other != java) {
            return None;
        }
        let forms = map.from.get(java)?;
        let &(_, first) = forms.first()?;
        let mut full = self.family(first).defaults.clone();
        full.extend(given.iter().map(|(k, v)| (k.clone(), v.clone())));
        forms
            .iter()
            .find(|(_, at)| self.family(*at).holds(&full))
            .map(|(bedrock, _)| bedrock.as_str())
    }
}

impl VersionStates<'_> {
    /// The version this map is for.
    #[must_use]
    pub fn mc_version(&self) -> &str {
        self.mc_version
    }

    /// The Bedrock form of `state`, read as the Java block of its id or else as the one Java block
    /// mapped onto the id that holds its properties. A property left out takes that block's
    /// default, and a bare Bedrock spelling several blocks answer is written bare.
    ///
    /// # Errors
    ///
    /// [`StateRefusal`] when the map has no form for the states, or they are not the block's.
    pub fn resolve(&self, state: &BlockState) -> Result<BedrockForm, StateRefusal> {
        let id = state.id.as_str();
        if self.reads_as_own(id, &state.properties) {
            self.resolve_same(id, &state.properties)
        } else {
            self.resolve_onto(id, &state.properties)
        }
    }

    /// Every canonical property `id` takes here with the values it holds, those of each block
    /// [`Self::resolve`] may read the id as, or `None` when no Java block maps onto it or the
    /// palette refuses some of its states, so the map cannot tell which states it takes.
    #[must_use]
    pub fn canonical_domain(&self, id: &str) -> Option<IndexMap<String, Vec<String>>> {
        let own = self.reads_as_java(id);
        let (forms, refused) = if own {
            (self.map.from.get(id), self.map.refused_from.contains(id))
        } else {
            (self.map.onto.get(id), self.map.refused_onto.contains(id))
        };
        let forms = forms.filter(|forms| !forms.is_empty() && !refused)?;
        let others = self
            .map
            .onto
            .get(id)
            .filter(|_| own && !self.map.refused_onto.contains(id))
            .into_iter()
            .flatten()
            .filter(|(java, _)| java != id);
        let mut domain: IndexMap<String, Vec<String>> = IndexMap::new();
        for (_, at) in forms.iter().chain(others) {
            for (key, held) in &self.family(*at).java {
                let values = domain.entry(key.clone()).or_default();
                for value in held {
                    if !values.contains(value) {
                        values.push(value.clone());
                    }
                }
            }
        }
        Some(domain)
    }

    /// Whether `id` names its own Java block here rather than a Bedrock spelling others map onto.
    fn reads_as_java(&self, id: &str) -> bool {
        self.map.refused_from.contains(id)
            || self
                .map
                .onto
                .get(id)
                .is_some_and(|forms| forms.iter().any(|(java, _)| java == id))
    }

    /// Whether an entry of `id` with the states `given` reads as `id`'s own Java block: `id` names
    /// one here, and that block has every property `given` carries or no other block mapped onto
    /// `id` has them.
    fn reads_as_own(&self, id: &str, given: &IndexMap<String, String>) -> bool {
        if !self.reads_as_java(id) {
            return false;
        }
        let own_takes = self
            .map
            .from
            .get(id)
            .and_then(|forms| forms.first())
            .is_none_or(|(_, at)| {
                let defaults = &self.family(*at).defaults;
                given.keys().all(|key| defaults.contains_key(key))
            });
        own_takes
            || !self.map.onto.get(id).is_some_and(|forms| {
                forms.iter().any(|(java, at)| {
                    java != id
                        && given
                            .keys()
                            .all(|key| self.family(*at).java.contains_key(key))
                })
            })
    }

    fn family(&self, at: usize) -> &Family {
        &self.index.families[at]
    }

    fn resolve_same(
        &self,
        id: &str,
        given: &IndexMap<String, String>,
    ) -> Result<BedrockForm, StateRefusal> {
        let forms = self.map.from.get(id).map_or(&[][..], Vec::as_slice);
        let Some(&(_, first)) = forms.first() else {
            return Err(StateRefusal::Unmapped {
                reason: refused(id),
            });
        };
        let defaults = &self.family(first).defaults;
        let unknown = given.keys().find(|key| !defaults.contains_key(*key));
        let mut full = defaults.clone();
        full.extend(given.iter().map(|(k, v)| (k.clone(), v.clone())));
        if unknown.is_none()
            && let Some((bedrock, at)) = forms.iter().find(|(_, at)| self.family(*at).holds(&full))
        {
            return Ok(self.form(bedrock, id, self.family(*at), &full));
        }
        // The refused pairs may hold these states, so no family's domain can rule them out.
        if self.map.refused_from.contains(id) {
            return Err(StateRefusal::Unmapped {
                reason: refused(id),
            });
        }
        if let Some(key) = unknown {
            return Err(self.elsewhere(&[id], key, None).unwrap_or_else(|| {
                StateRefusal::UnknownProperty {
                    key: key.clone(),
                    valid: defaults.keys().cloned().collect(),
                }
            }));
        }
        let held = |key: &str| -> Vec<String> {
            let mut values: Vec<String> = Vec::new();
            for (_, at) in forms {
                for value in &self.family(*at).java[key] {
                    if !values.contains(value) {
                        values.push(value.clone());
                    }
                }
            }
            values
        };
        let outside = full.iter().find(|(key, value)| !held(key).contains(*value));
        Err(match outside {
            Some((key, value)) => self.elsewhere(&[id], key, Some(value)).unwrap_or_else(|| {
                StateRefusal::UnknownValue {
                    key: key.clone(),
                    value: value.clone(),
                    valid: held(key),
                }
            }),
            // Each value is some family's, but no family holds them together.
            None => StateRefusal::Unmapped {
                reason: format!(
                    "no family of `{id}` there holds these states together, so the pack carries \
                     no form for them"
                ),
            },
        })
    }

    /// The refusal for `key`, at `value` when one is given, which no family of the Java blocks
    /// `java` holds at this version but some other version's map does; `None` when none does.
    fn elsewhere(&self, java: &[&str], key: &str, value: Option<&str>) -> Option<StateRefusal> {
        java.iter().find_map(|java| {
            let versions = self
                .index
                .versions_holding(self.mc_version, java, key, value);
            let state = value.map_or_else(|| key.to_owned(), |value| format!("{key}={value}"));
            (!versions.is_empty()).then(|| StateRefusal::Unmapped {
                reason: format!(
                    "its state map there is read from a Java release whose `{java}` has no \
                     `{state}`, which the map of {} holds",
                    and_list(&versions)
                ),
            })
        })
    }

    fn resolve_onto(
        &self,
        id: &str,
        given: &IndexMap<String, String>,
    ) -> Result<BedrockForm, StateRefusal> {
        let forms = self.map.onto.get(id).map_or(&[][..], Vec::as_slice);
        if forms.is_empty() {
            if self.map.refused_onto.contains(id) {
                return Err(StateRefusal::Unmapped {
                    reason: refused(id),
                });
            }
            return bare_or(
                id,
                given,
                format!(
                    "no Java block maps onto `{id}` there, so the canonical vocabulary names none \
                     of its states"
                ),
            );
        }
        let fitting: Vec<&(String, usize)> = forms
            .iter()
            .filter(|(_, at)| self.family(*at).takes(given))
            .collect();
        let Some(&&(ref java, at)) = fitting.first() else {
            if self.map.refused_onto.contains(id) {
                return Err(StateRefusal::Unmapped {
                    reason: refused(id),
                });
            }
            return Err(self.not_taken(id, given, forms));
        };
        let family = self.family(at);
        if fitting
            .iter()
            .any(|(_, other)| self.family(*other) != family)
        {
            return bare_or(id, given, several(id, &fitting));
        }
        let mut full = IndexMap::with_capacity(family.java.len());
        for (key, held) in &family.java {
            let value = match (given.get(key), &family.defaults[key]) {
                (Some(value), _) => value,
                (None, default) if held.contains(default) => default,
                (None, _) if held.len() == 1 => &held[0],
                (None, _) => return bare_or(id, given, several(id, &fitting)),
            };
            full.insert(key.clone(), value.clone());
        }
        Ok(self.form(id, java, family, &full))
    }

    /// Why no Java block mapped onto `id` takes `given`: a property or value none has, else that
    /// several do and these states do not say which.
    fn not_taken(
        &self,
        id: &str,
        given: &IndexMap<String, String>,
        forms: &[(String, usize)],
    ) -> StateRefusal {
        let families = || forms.iter().map(|(_, at)| self.family(*at));
        let mut java: Vec<&str> = Vec::with_capacity(forms.len());
        for (name, _) in forms {
            if !java.contains(&name.as_str()) {
                java.push(name);
            }
        }
        for (key, value) in given {
            let mut valid: Vec<String> = Vec::new();
            for family in families() {
                for held in family.java.get(key).into_iter().flatten() {
                    if !valid.contains(held) {
                        valid.push(held.clone());
                    }
                }
            }
            if valid.is_empty() {
                let mut keys: Vec<String> = Vec::new();
                for family in families() {
                    for key in family.java.keys() {
                        if !keys.contains(key) {
                            keys.push(key.clone());
                        }
                    }
                }
                return self.elsewhere(&java, key, None).unwrap_or_else(|| {
                    StateRefusal::UnknownProperty {
                        key: key.clone(),
                        valid: keys,
                    }
                });
            }
            if !valid.contains(value) {
                return self.elsewhere(&java, key, Some(value)).unwrap_or_else(|| {
                    StateRefusal::UnknownValue {
                        key: key.clone(),
                        value: value.clone(),
                        valid,
                    }
                });
            }
        }
        let all: Vec<&(String, usize)> = forms.iter().collect();
        StateRefusal::Unmapped {
            reason: several(id, &all),
        }
    }

    fn form(
        &self,
        bedrock: &str,
        java: &str,
        family: &Family,
        full: &IndexMap<String, String>,
    ) -> BedrockForm {
        let states = family
            .states
            .iter()
            .map(|(name, rule)| {
                let value = match rule {
                    Rule::Constant(value) => value.clone(),
                    Rule::Lookup { reads, map } => {
                        let joined: Vec<&str> = reads.iter().map(|k| full[k].as_str()).collect();
                        // INVARIANT(rules-cover-holds): the loader refuses a lookup that does not map
                        // every combination its reads hold, and `full` is one of them.
                        map[&joined.join(",")].clone()
                    }
                };
                (name.clone(), value)
            })
            .collect();
        let dropped = family
            .dropped
            .iter()
            .filter(|key| full[*key] != family.defaults[*key])
            .filter_map(|key| {
                // INVARIANT(dropped-classified): the loader refuses a dropped key without a row.
                let (kind, reason) = self.index.sensitivity.get(key)?.row(java)?;
                (*kind == SensitivityKind::MissingState).then(|| DroppedState {
                    key: key.clone(),
                    value: full[key].clone(),
                    reason: reason.clone(),
                })
            })
            .collect();
        BedrockForm {
            id: bedrock.to_owned(),
            states,
            waterlogged: full.get(WATER_LAYER_STATE).is_some_and(|v| v == "true")
                || self.index.in_water.contains(java),
            dropped,
        }
    }
}

/// Why `id` has no form at a version whose palette refuses the states it maps to.
fn refused(id: &str) -> String {
    format!(
        "its palette does not declare the states GeyserMC maps `{id}` to, so the pack carries no \
         form for them"
    )
}

/// Why `id`, the Bedrock spelling of each Java block in `forms`, has no form for given states.
fn several(id: &str, forms: &[&(String, usize)]) -> String {
    let mut java: Vec<&str> = Vec::with_capacity(forms.len());
    for (name, _) in forms {
        if !java.contains(&name.as_str()) {
            java.push(name);
        }
    }
    format!(
        "`{id}` is {} there, and these states do not say which",
        and_or_list(&java)
    )
}

impl Family {
    /// Whether every property of `full`, a whole state of the Java block, holds a value it holds.
    fn holds(&self, full: &IndexMap<String, String>) -> bool {
        self.java
            .iter()
            .all(|(key, held)| full.get(key).is_some_and(|value| held.contains(value)))
    }

    /// Whether each property of `given`, part of a state, is one it has at a value it holds.
    fn takes(&self, given: &IndexMap<String, String>) -> bool {
        given
            .iter()
            .all(|(key, value)| self.java.get(key).is_some_and(|held| held.contains(value)))
    }
}

/// `id`, which no Java block's states answer for, written bare when the entry names no state,
/// else the refusal `reason` gives.
fn bare_or(
    id: &str,
    given: &IndexMap<String, String>,
    reason: String,
) -> Result<BedrockForm, StateRefusal> {
    if given.is_empty() {
        Ok(BedrockForm {
            id: id.to_owned(),
            states: Vec::new(),
            waterlogged: false,
            dropped: Vec::new(),
        })
    } else {
        Err(StateRefusal::Unmapped { reason })
    }
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `a`, `a or b`, `a, b or c`, each backticked; past four names, the first three and a count.
fn and_or_list(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] if quoted.len() <= 4 => format!("{} or {last}", init.join(", ")),
        _ => format!(
            "{} or {} other Java blocks",
            quoted[..3].join(", "),
            quoted.len() - 3
        ),
    }
}

#[cfg(test)]
mod tests;
