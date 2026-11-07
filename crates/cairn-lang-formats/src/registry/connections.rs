//! The `connections` component of a registry pack: what each block is, per version, to the rules
//! that join panes, fences, walls, chests and redstone dust to their neighbours (`spec/blockstate`
//! "Connections").

use std::collections::HashMap;

use cairn_lang_core::block_array::{Covers, Face, Faces, JoinKind, Joins};
use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `connections.schema_version` this build reads.
pub const SUPPORTED_CONNECTIONS_SCHEMA: u32 = 1;

/// The states the rules read of a neighbouring fence gate.
const GATE_READS: [&str; 2] = ["facing", "open"];

/// The states the rules read of a chest.
const CHEST_READS: [&str; 1] = ["facing"];

/// The body of `connections.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace of every id the catalog names.
    pub namespace: String,
    /// The rows, each with the versions and blocks it describes.
    pub rows: Vec<ConnectionRow>,
}

/// One row of [`ConnectionsCatalog::rows`]: blocks the rules read alike at some versions.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRow {
    /// The versions the row describes.
    pub mc_versions: Vec<String>,
    /// The canonical ids of the blocks it describes.
    pub ids: Vec<String>,
    /// The kind of connecting block they are, or none for blocks that only offer their faces.
    #[serde(default)]
    pub kind: Option<ConnectionKind>,
    /// Whether a pane, fence or wall never joins them by a whole face.
    #[serde(default)]
    pub never_joined_by_face: bool,
    /// Whether the game's `wall_post_override` tag holds them.
    #[serde(default)]
    pub raises_post: bool,
    /// The default state's value of each property the rules read.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
    /// The faces their models cover whole.
    #[serde(default)]
    pub sturdy: Option<ByState>,
    /// What their bottoms cover of a wall below: `post`, and the directions of its sides.
    #[serde(default)]
    pub covers: Option<ByState>,
    /// The faces redstone dust beside them joins.
    #[serde(default)]
    pub dust: Option<ByState>,
}

/// The kind of connecting block a [`ConnectionRow`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// A glass pane or a run of bars.
    Pane,
    /// A wall.
    Wall,
    /// A fence that is not wooden.
    Fence,
    /// A wooden fence.
    WoodenFence,
    /// A fence gate.
    FenceGate,
    /// A chest, which pairs with a chest of the same block and facing beside it.
    Chest,
    /// Redstone dust.
    RedstoneDust,
}

/// Names that hold of a [`ConnectionRow`]'s blocks, in every state alike or by state.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ByState {
    /// Looked up from the values of some of the blocks' properties.
    Lookup(StateLookup),
    /// The same names in every state.
    Every(Vec<String>),
}

/// A [`ByState`] looked up from the values of [`Self::reads`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateLookup {
    /// The properties read, in order.
    pub reads: Vec<String>,
    /// Their values, comma-joined, to the names that hold there.
    pub map: IndexMap<String, Vec<String>>,
}

/// Why a `connections` component does not validate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionsError {
    /// The component's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack connections schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The component has no rows; a pack without connection tables names no `connections` file.
    #[error(
        "registry pack `connections` has no rows; a pack without connection tables names no \
         `connections` file"
    )]
    NoRows,
    /// A row names no block, or no version, and so describes nothing.
    #[error("registry pack `connections` `rows[{index}]` names no {missing}")]
    EmptyRow {
        /// The row's place in `rows`, from 0.
        index: usize,
        /// `"block"` or `"version"`.
        missing: &'static str,
    },
    /// Two rows describe one block at one version.
    #[error("registry pack `connections` describes `{id}` twice at `{mc_version}`")]
    DuplicateId {
        /// The version.
        mc_version: String,
        /// The id.
        id: String,
    },
    /// A row is malformed: a face, a lookup or a default its rules need.
    #[error("registry pack `connections` row of `{id}`: {problem}")]
    MalformedRow {
        /// The row's first id.
        id: String,
        /// What is wrong.
        problem: String,
    },
}

/// The validated connection tables of a pack, empty for a pack without the component.
#[derive(Debug, Clone, Default)]
pub struct ConnectionIndex {
    rows: Vec<Row>,
    /// Per version, each id to its row.
    by_version: IndexMap<String, HashMap<String, usize>>,
}

#[derive(Debug, Clone)]
struct Row {
    kind: Option<RowKind>,
    never_joined_by_face: bool,
    raises_post: bool,
    defaults: IndexMap<String, String>,
    sturdy: Held<Faces>,
    covers: Held<Covers>,
    dust: Held<Faces>,
}

#[derive(Debug, Clone, Copy)]
enum RowKind {
    Joins(JoinKind),
    /// A fence gate, with the `facing` and `open` its default state holds.
    Gate {
        facing: Face,
        open: bool,
    },
    /// A chest, with the `facing` its default state holds.
    Chest {
        facing: Face,
    },
}

/// What holds of a row's blocks, parsed from a [`ByState`].
#[derive(Debug, Clone)]
enum Held<T> {
    Every(T),
    Lookup {
        reads: Vec<String>,
        map: HashMap<String, T>,
    },
}

impl<T: Copy + Default> Held<T> {
    /// What holds where `value` gives each property's value; nothing for values the map lacks.
    fn at<'v>(&self, value: impl Fn(&str) -> Option<&'v str>) -> T {
        match self {
            Self::Every(held) => *held,
            Self::Lookup { reads, map } => {
                let values: Option<Vec<&str>> = reads.iter().map(|key| value(key)).collect();
                values
                    .and_then(|values| map.get(&values.join(",")).copied())
                    .unwrap_or_default()
            }
        }
    }

    /// Each property the lookup reads, paired with every value one of its keys names.
    fn named(&self) -> impl Iterator<Item = (&str, &str)> {
        let (reads, map): (&[String], Option<&HashMap<String, T>>) = match self {
            Self::Every(_) => (&[], None),
            Self::Lookup { reads, map } => (reads, Some(map)),
        };
        map.into_iter()
            .flat_map(HashMap::keys)
            .flat_map(move |key| reads.iter().map(String::as_str).zip(key.split(',')))
    }
}

impl ConnectionIndex {
    /// An index with no table, for a pack without the component.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Validate `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`ConnectionsError`] when the schema is too new, the catalog has no rows, a row is empty or
    /// malformed, or two rows describe one block at one version.
    pub fn from_catalog(catalog: ConnectionsCatalog) -> Result<Self, ConnectionsError> {
        if catalog.schema_version > SUPPORTED_CONNECTIONS_SCHEMA {
            return Err(ConnectionsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_CONNECTIONS_SCHEMA,
            });
        }
        if catalog.rows.is_empty() {
            return Err(ConnectionsError::NoRows);
        }
        let mut index = Self::default();
        for (at, row) in catalog.rows.into_iter().enumerate() {
            let Some(first) = row.ids.first() else {
                return Err(ConnectionsError::EmptyRow {
                    index: at,
                    missing: "block",
                });
            };
            if row.mc_versions.is_empty() {
                return Err(ConnectionsError::EmptyRow {
                    index: at,
                    missing: "version",
                });
            }
            index.rows.push(
                parse_row(&row).map_err(|problem| ConnectionsError::MalformedRow {
                    id: first.clone(),
                    problem,
                })?,
            );
            for version in &row.mc_versions {
                let ids = index.by_version.entry(version.clone()).or_default();
                for id in &row.ids {
                    let id = namespaced(&catalog.namespace, id);
                    if ids.insert(id.clone(), at).is_some() {
                        return Err(ConnectionsError::DuplicateId {
                            mc_version: version.clone(),
                            id,
                        });
                    }
                }
            }
        }
        Ok(index)
    }

    /// Whether the pack carries no connection table.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_version.is_empty()
    }

    /// The versions the tables describe, in the order the component first names them.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// The ids `mc_version`'s table describes, or `None` when there is no table for it.
    pub fn ids_at(&self, mc_version: &str) -> Option<impl Iterator<Item = &str>> {
        Some(self.by_version.get(mc_version)?.keys().map(String::as_str))
    }

    /// Each state `mc_version`'s table names of a block, as `(id, property, value)`: every
    /// default its row gives and every value a lookup's key holds.
    pub fn named_states(&self, mc_version: &str) -> impl Iterator<Item = (&str, &str, &str)> {
        self.by_version
            .get(mc_version)
            .into_iter()
            .flatten()
            .flat_map(|(id, &at)| {
                let row = &self.rows[at];
                row.defaults
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .chain(row.sturdy.named())
                    .chain(row.covers.named())
                    .chain(row.dust.named())
                    .map(move |(key, value)| (id.as_str(), key, value))
            })
    }

    /// What the canonical block `id` with `properties` is to the connection rules at
    /// `mc_version`, or `None` when there is no table for the version.
    #[must_use]
    pub fn joins(
        &self,
        mc_version: &str,
        id: &str,
        properties: &IndexMap<String, String>,
    ) -> Option<Joins> {
        let table = self.by_version.get(mc_version)?;
        let Some(&at) = table.get(id) else {
            return Some(Joins::default());
        };
        let row = &self.rows[at];
        let value = |key: &str| {
            properties
                .get(key)
                .or_else(|| row.defaults.get(key))
                .map(String::as_str)
        };
        let kind = row.kind.and_then(|kind| match kind {
            RowKind::Joins(kind) => Some(kind),
            RowKind::Gate { facing, open } => Some(JoinKind::FenceGate {
                facing: match properties.get("facing") {
                    Some(written) => side_named(written)?,
                    None => facing,
                },
                open: match properties.get("open") {
                    Some(written) => written.parse().ok()?,
                    None => open,
                },
            }),
            RowKind::Chest { facing } => Some(JoinKind::Chest {
                facing: match properties.get("facing") {
                    Some(written) => side_named(written)?,
                    None => facing,
                },
            }),
        });
        Some(Joins {
            kind,
            sturdy: row.sturdy.at(value),
            covers: row.covers.at(value),
            never_joined_by_face: row.never_joined_by_face,
            raises_post: row.raises_post,
            dust: row.dust.at(value),
        })
    }
}

/// The sideways face `name` names, or `None` for any other name.
fn side_named(name: &str) -> Option<Face> {
    Face::from_name(name).filter(|face| Face::SIDES.contains(face))
}

/// `faces` as a set, or the first name that is no face.
fn faces_of(faces: &[String]) -> Result<Faces, String> {
    faces
        .iter()
        .map(|name| Face::from_name(name).ok_or_else(|| format!("`{name}` is no face")))
        .collect()
}

/// `parts` as what a bottom covers, or the first name that is no part of a wall.
fn covers_of(parts: &[String]) -> Result<Covers, String> {
    parts.iter().try_fold(Covers::default(), |covers, name| {
        covers
            .with(name)
            .ok_or_else(|| format!("`{name}` is no part of a wall: `post`, or a side's direction"))
    })
}

/// The `field` names a row writes, parsed by `parse`; nothing when the row leaves it out.
fn held<T: Default>(
    field: &str,
    written: Option<&ByState>,
    parse: impl Fn(&[String]) -> Result<T, String>,
) -> Result<Held<T>, String> {
    let lookup = match written {
        None => return Ok(Held::Every(T::default())),
        Some(ByState::Every(names)) => return Ok(Held::Every(parse(names)?)),
        Some(ByState::Lookup(lookup)) => lookup,
    };
    if lookup.reads.is_empty() || lookup.map.is_empty() {
        return Err(format!(
            "the `{field}` lookup reads no property or maps no value; a list says what holds in \
             every state"
        ));
    }
    let mut map = HashMap::with_capacity(lookup.map.len());
    for (key, names) in &lookup.map {
        if key.split(',').count() != lookup.reads.len() {
            return Err(format!(
                "the `{field}` key `{key}` holds no value for each of {}",
                lookup.reads.join(", ")
            ));
        }
        map.insert(key.clone(), parse(names)?);
    }
    Ok(Held::Lookup {
        reads: lookup.reads.clone(),
        map,
    })
}

fn parse_row(row: &ConnectionRow) -> Result<Row, String> {
    let sturdy = held("sturdy", row.sturdy.as_ref(), faces_of)?;
    let covers = held("covers", row.covers.as_ref(), covers_of)?;
    let dust = held("dust", row.dust.as_ref(), faces_of)?;
    let mut needs: Vec<&str> = [&row.sturdy, &row.covers, &row.dust]
        .into_iter()
        .flatten()
        .flat_map(|written| match written {
            ByState::Lookup(lookup) => lookup.reads.as_slice(),
            ByState::Every(_) => &[],
        })
        .map(String::as_str)
        .collect();
    match row.kind {
        Some(ConnectionKind::FenceGate) => needs.extend(GATE_READS),
        Some(ConnectionKind::Chest) => needs.extend(CHEST_READS),
        _ => {}
    }
    if let Some(missing) = needs.iter().find(|key| !row.defaults.contains_key(**key)) {
        return Err(format!(
            "the rules read `{missing}`, which `defaults` does not give"
        ));
    }
    if let Some(unread) = row
        .defaults
        .keys()
        .find(|key| !needs.contains(&key.as_str()))
    {
        return Err(format!(
            "`defaults` gives `{unread}`, which the rules do not read"
        ));
    }
    let kind = match row.kind {
        None => None,
        Some(ConnectionKind::Pane) => Some(RowKind::Joins(JoinKind::Pane)),
        Some(ConnectionKind::Wall) => Some(RowKind::Joins(JoinKind::Wall)),
        Some(ConnectionKind::Fence) => Some(RowKind::Joins(JoinKind::Fence { wooden: false })),
        Some(ConnectionKind::WoodenFence) => Some(RowKind::Joins(JoinKind::Fence { wooden: true })),
        Some(ConnectionKind::RedstoneDust) => Some(RowKind::Joins(JoinKind::Dust)),
        Some(ConnectionKind::FenceGate) => {
            let facing = &row.defaults["facing"];
            let open = &row.defaults["open"];
            Some(RowKind::Gate {
                facing: side_named(facing)
                    .ok_or_else(|| format!("the default `facing={facing}` is no sideways face"))?,
                open: open
                    .parse()
                    .map_err(|_| format!("the default `open={open}` is neither true nor false"))?,
            })
        }
        Some(ConnectionKind::Chest) => {
            let facing = &row.defaults["facing"];
            Some(RowKind::Chest {
                facing: side_named(facing)
                    .ok_or_else(|| format!("the default `facing={facing}` is no sideways face"))?,
            })
        }
    };
    Ok(Row {
        kind,
        never_joined_by_face: row.never_joined_by_face,
        raises_post: row.raises_post,
        defaults: row.defaults.clone(),
        sturdy,
        covers,
        dust,
    })
}

#[cfg(test)]
mod tests;
