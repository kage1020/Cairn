//! The `connections` component of a registry pack: what each block is, per version, to the rules
//! that join panes, fences, walls, chests and redstone dust to their neighbours (`spec/blockstate`
//! "Connections").

use std::collections::HashMap;

use cairn_lang_core::block_array::{Face, Faces, JoinKind, Joins};
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
    pub exception: bool,
    /// Whether a wall under them raises its post whatever its sides.
    #[serde(default)]
    pub raises_post: bool,
    /// The default state's value of each property the rules read.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
    /// The faces their models cover whole.
    #[serde(default)]
    pub sturdy: Option<FacesRow>,
    /// The faces redstone dust beside them joins.
    #[serde(default)]
    pub dust: Option<FacesRow>,
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
    /// A chest that pairs with the chest beside it.
    Chest,
    /// Redstone dust.
    RedstoneDust,
}

/// Some faces of a [`ConnectionRow`]'s blocks, the same in every state or looked up per state.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FacesRow {
    /// Looked up from the values of `reads`, comma-joined.
    Lookup {
        /// The properties read, in order.
        reads: Vec<String>,
        /// Their values, comma-joined, to the faces whole there.
        map: IndexMap<String, Vec<String>>,
    },
    /// The same faces in every state.
    Every(Vec<String>),
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
    kind: Option<ConnectionKind>,
    exception: bool,
    raises_post: bool,
    defaults: IndexMap<String, String>,
    sturdy: FaceSet,
    dust: FaceSet,
}

#[derive(Debug, Clone)]
enum FaceSet {
    Every(Faces),
    Lookup {
        reads: Vec<String>,
        map: HashMap<String, Faces>,
    },
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
    /// [`ConnectionsError`] when the schema is too new, a row is malformed, or two rows describe
    /// one block at one version.
    pub fn from_catalog(catalog: ConnectionsCatalog) -> Result<Self, ConnectionsError> {
        if catalog.schema_version > SUPPORTED_CONNECTIONS_SCHEMA {
            return Err(ConnectionsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_CONNECTIONS_SCHEMA,
            });
        }
        let mut index = Self::default();
        for row in catalog.rows {
            let first = row.ids.first().cloned().unwrap_or_default();
            let at = index.rows.len();
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
        let faces = |set: &FaceSet| match set {
            FaceSet::Every(faces) => *faces,
            FaceSet::Lookup { reads, map } => {
                let values: Option<Vec<&str>> = reads.iter().map(|key| value(key)).collect();
                values
                    .and_then(|values| map.get(&values.join(",")).copied())
                    .unwrap_or(Faces::NONE)
            }
        };
        let kind = row.kind.and_then(|kind| {
            Some(match kind {
                ConnectionKind::Pane => JoinKind::Pane,
                ConnectionKind::Wall => JoinKind::Wall,
                ConnectionKind::Fence => JoinKind::Fence { wooden: false },
                ConnectionKind::WoodenFence => JoinKind::Fence { wooden: true },
                ConnectionKind::FenceGate => JoinKind::FenceGate {
                    facing: value("facing")
                        .and_then(Face::from_name)
                        .filter(|face| Face::SIDES.contains(face))?,
                    open: value("open")? == "true",
                },
                ConnectionKind::Chest => JoinKind::Chest {
                    facing: value("facing")
                        .and_then(Face::from_name)
                        .filter(|face| Face::SIDES.contains(face))?,
                },
                ConnectionKind::RedstoneDust => JoinKind::Dust,
            })
        });
        Some(Joins {
            kind,
            sturdy: faces(&row.sturdy),
            exception: row.exception,
            raises_post: row.raises_post,
            dust: faces(&row.dust),
        })
    }
}

/// `faces` as a set, or the first name that is no face.
fn faces_of(faces: &[String]) -> Result<Faces, String> {
    faces
        .iter()
        .map(|name| Face::from_name(name).ok_or_else(|| format!("`{name}` is no face")))
        .collect()
}

/// `row` as a set of faces per state, or why its lookup is malformed; `field` names it.
fn face_set(row: Option<&FacesRow>, field: &str) -> Result<FaceSet, String> {
    Ok(match row {
        None => FaceSet::Every(Faces::NONE),
        Some(FacesRow::Every(faces)) => FaceSet::Every(faces_of(faces)?),
        Some(FacesRow::Lookup { reads, map }) => {
            let mut parsed = HashMap::with_capacity(map.len());
            for (key, faces) in map {
                if key.split(',').count() != reads.len() {
                    return Err(format!(
                        "the `{field}` key `{key}` holds no value for each of {}",
                        reads.join(", ")
                    ));
                }
                parsed.insert(key.clone(), faces_of(faces)?);
            }
            FaceSet::Lookup {
                reads: reads.clone(),
                map: parsed,
            }
        }
    })
}

fn parse_row(row: &ConnectionRow) -> Result<Row, String> {
    let sturdy = face_set(row.sturdy.as_ref(), "sturdy")?;
    let dust = face_set(row.dust.as_ref(), "dust")?;
    let mut needs: Vec<&str> = Vec::new();
    for set in [&sturdy, &dust] {
        if let FaceSet::Lookup { reads, .. } = set {
            needs.extend(reads.iter().map(String::as_str));
        }
    }
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
    Ok(Row {
        kind: row.kind,
        exception: row.exception,
        raises_post: row.raises_post,
        defaults: row.defaults.clone(),
        sturdy,
        dust,
    })
}

#[cfg(test)]
mod tests;
