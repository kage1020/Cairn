//! The constraint catalog: what the game requires of the cells around a block to keep it where a
//! build puts it, and the boundary versions across which a block means something else
//! (`spec/lint` "Constraint catalog"). It is edition-neutral, written in canonical ids, and
//! hashed apart from the registry packs.

use std::collections::HashMap;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{Constraint, Face, Faces, Support};
use cairn_lang_core::lock::HashHex;
use cairn_lang_core::resolve::{FloorPlacement, VersionOrder};
use indexmap::IndexMap;
use serde::Deserialize;

use super::blocks::BlocksIndex;
use super::namespaced;

/// The highest `constraints.schema_version` this build reads.
pub const SUPPORTED_CONSTRAINTS_SCHEMA: u32 = 1;

/// The body of `constraints.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The catalog's name.
    pub name: String,
    /// What the catalog holds and where it comes from.
    pub description: String,
    /// The namespace of every id the catalog names without one.
    pub namespace: String,
    /// The blocks whose meaning changes across a boundary version.
    #[serde(default)]
    pub semantic_sensitivity: Vec<SensitivityRow>,
    /// The blocks that hang on a face of the block beside them.
    #[serde(default)]
    pub attachment: Vec<AttachmentRow>,
    /// The blocks that stand on the block below them or hang from the block above.
    #[serde(default)]
    pub support: Vec<SupportRow>,
    /// The blocks that fall through air or a fluid below them.
    #[serde(default)]
    pub gravity: Vec<GravityRow>,
    /// The blocks beyond whole faces and posts whose faces give a centre.
    #[serde(default)]
    pub center_support: Vec<CenterRow>,
    /// Each entity's box.
    #[serde(default)]
    pub entity_aabb: Vec<EntityAabbRow>,
}

/// One row of [`ConstraintsCatalog::semantic_sensitivity`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityRow {
    /// The edition whose versions [`Self::boundary`] is one of.
    pub edition: Edition,
    /// The first version that reads the blocks the new way.
    pub boundary: String,
    /// What changes there, as a member's finding words it.
    pub reason: String,
    /// The blocks it changes for.
    pub ids: Vec<String>,
}

/// One row of [`ConstraintsCatalog::attachment`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentRow {
    /// The blocks it describes.
    pub ids: Vec<String>,
    /// Which states say where they hang.
    pub hangs_by: HangsBy,
    /// The default state's value of each property the row reads.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
}

/// The states an [`AttachmentRow`]'s blocks hang by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HangsBy {
    /// `face`: on the block below at `floor`, above at `ceiling`, and behind at `wall`, where
    /// `facing` is the way out from it.
    Face,
    /// `facing`, the way out from the block behind.
    Facing,
}

/// One row of [`ConstraintsCatalog::support`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportRow {
    /// The blocks it describes.
    pub ids: Vec<String>,
    /// The states it holds in; every state when empty.
    #[serde(default)]
    pub when: IndexMap<String, String>,
    /// The default state's value of each property `when` reads.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
    /// Which block holds them.
    pub on: Side,
    /// What that block's face toward them must give.
    pub needs: Needs,
    /// Blocks that hold them whatever their faces.
    #[serde(default)]
    pub also_on: Vec<String>,
}

/// The block a [`SupportRow`]'s blocks rest on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The block below, which they stand on.
    Below,
    /// The block above, which they hang from.
    Above,
}

/// What a [`SupportRow`]'s blocks need of the face that holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// The whole face.
    Sturdy,
    /// The centre of the face.
    Center,
}

/// One row of [`ConstraintsCatalog::gravity`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GravityRow {
    /// The blocks it describes.
    pub ids: Vec<String>,
}

/// One row of [`ConstraintsCatalog::center_support`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CenterRow {
    /// The blocks it describes.
    pub ids: Vec<String>,
    /// The states it holds in; every state when empty.
    #[serde(default)]
    pub when: IndexMap<String, String>,
    /// The default state's value of each property `when` reads.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
    /// The faces that give a centre.
    pub faces: Vec<String>,
}

/// One row of [`ConstraintsCatalog::entity_aabb`], in blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAabbRow {
    /// The entities it describes.
    pub entities: Vec<String>,
    /// Across the entity, or along the face it hangs on.
    pub width: f64,
    /// Up the entity, or up the face it hangs on.
    pub height: f64,
    /// Out of the face, for an entity that hangs on one; absent for one that stands.
    #[serde(default)]
    pub depth: Option<f64>,
}

/// An entity's box, in blocks, as [`ConstraintIndex::entity_box`] gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityBox {
    /// Across the entity, or along the face it hangs on.
    pub width: f64,
    /// Up the entity, or up the face it hangs on.
    pub height: f64,
    /// Out of the face, for an entity that hangs on one; `None` for one that stands.
    pub depth: Option<f64>,
}

/// A boundary version across which a block means something else, as
/// [`ConstraintIndex::boundaries_of`] gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary<'a> {
    /// The first version that reads the block the new way.
    pub version: &'a str,
    /// What changes there.
    pub reason: &'a str,
}

/// Why a constraint catalog does not load.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstraintsError {
    /// The text is not a catalog's JSON.
    #[error("the constraint catalog does not parse: {message}")]
    Parse {
        /// The parser's message.
        message: String,
    },
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported constraint catalog schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// A row names no block or entity, and so describes nothing.
    #[error("constraint catalog `{table}` row {index} names no {missing}")]
    EmptyRow {
        /// The table.
        table: &'static str,
        /// The row's place in the table, from 0.
        index: usize,
        /// `"block"` or `"entity"`.
        missing: &'static str,
    },
    /// A row names a block no version of the Java registry pack declares.
    #[error(
        "constraint catalog `{table}` names `{id}`, which no version of the Java registry pack \
         declares"
    )]
    UnknownId {
        /// The table.
        table: &'static str,
        /// The id, namespaced.
        id: String,
    },
    /// A row is malformed: a state its block never takes, a default it lacks or does not read,
    /// a face, a reason or a box dimension.
    #[error("constraint catalog `{table}` row of `{id}`: {problem}")]
    MalformedRow {
        /// The table.
        table: &'static str,
        /// The row's first id.
        id: String,
        /// What is wrong.
        problem: String,
    },
    /// Two rows give one state of a block two rules.
    #[error(
        "constraint catalog gives a state of `{id}` a rule in `{first}` and another in `{second}`"
    )]
    OverlappingRules {
        /// The id, namespaced.
        id: String,
        /// The table of the first rule.
        first: &'static str,
        /// The table of the second.
        second: &'static str,
    },
    /// A boundary is no version of its edition's registry pack.
    #[error(
        "constraint catalog `semantic_sensitivity` boundary `{boundary}` is no version of the {} \
         registry pack",
        edition.as_str()
    )]
    UnknownBoundary {
        /// The edition.
        edition: Edition,
        /// The boundary, as written.
        boundary: String,
    },
}

/// A validated constraint catalog, ready for lookup.
#[derive(Debug, Clone)]
pub struct ConstraintIndex {
    hash: HashHex,
    sensitivity: Vec<Sensitivity>,
    /// Per id, its rules, which no one state meets two of.
    rules: HashMap<String, Vec<Rule>>,
    /// Per id, the faces its states give a centre by.
    centres: HashMap<String, Vec<Centre>>,
    entities: IndexMap<String, EntityBox>,
}

#[derive(Debug, Clone)]
struct Sensitivity {
    edition: Edition,
    boundary: String,
    reason: String,
    ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct Rule {
    table: &'static str,
    when: IndexMap<String, String>,
    defaults: IndexMap<String, String>,
    kind: RuleKind,
}

#[derive(Debug, Clone)]
enum RuleKind {
    Hangs(HangsBy),
    Rests {
        host: Face,
        needs: Support,
        also_on: Vec<String>,
    },
    Falls,
}

#[derive(Debug, Clone)]
struct Centre {
    when: IndexMap<String, String>,
    defaults: IndexMap<String, String>,
    faces: Faces,
}

/// Whether a state with `properties` meets `when`, reading `defaults` for a property it lacks.
fn meets(
    when: &IndexMap<String, String>,
    defaults: &IndexMap<String, String>,
    properties: &IndexMap<String, String>,
) -> bool {
    when.iter()
        .all(|(key, value)| properties.get(key).or_else(|| defaults.get(key)) == Some(value))
}

impl Rule {
    /// The rule's constraint on a state with `properties`, or `None` when a state it reads is no
    /// face.
    fn constraint(&self, properties: &IndexMap<String, String>) -> Option<Constraint> {
        let read = |key: &str| properties.get(key).or_else(|| self.defaults.get(key));
        Some(match &self.kind {
            RuleKind::Hangs(by) => {
                let facing = Face::from_name(read("facing")?)?;
                let host = match (by, read("face").map(String::as_str)) {
                    (HangsBy::Face, Some("floor")) => Face::Down,
                    (HangsBy::Face, Some("ceiling")) => Face::Up,
                    (HangsBy::Face, Some("wall")) | (HangsBy::Facing, _) => facing.opposite(),
                    (HangsBy::Face, _) => return None,
                };
                Constraint::Hangs { host }
            }
            RuleKind::Rests { host, needs, .. } => Constraint::Rests {
                host: *host,
                needs: *needs,
            },
            RuleKind::Falls => Constraint::Falls,
        })
    }
}

impl ConstraintIndex {
    /// Validate `catalog`, whose text hashes to `hash`, against the Java block tables and the Java
    /// and Bedrock version tables.
    ///
    /// # Errors
    ///
    /// [`ConstraintsError`] for the first fault in the catalog.
    pub fn from_catalog(
        catalog: ConstraintsCatalog,
        hash: HashHex,
        java: &BlocksIndex,
        versions: [&VersionOrder; 2],
    ) -> Result<Self, ConstraintsError> {
        if catalog.schema_version > SUPPORTED_CONSTRAINTS_SCHEMA {
            return Err(ConstraintsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_CONSTRAINTS_SCHEMA,
            });
        }
        let check = Checker {
            namespace: &catalog.namespace,
            java,
        };
        let mut index = Self {
            hash,
            sensitivity: Vec::new(),
            rules: HashMap::new(),
            centres: HashMap::new(),
            entities: IndexMap::new(),
        };
        index.add_sensitivity(&check, catalog.semantic_sensitivity, versions)?;
        index.add_attachment(&check, catalog.attachment)?;
        index.add_support(&check, catalog.support)?;
        index.add_gravity(&check, catalog.gravity)?;
        index.add_centres(&check, catalog.center_support)?;
        index.add_entities(&catalog.namespace, catalog.entity_aabb)?;
        Ok(index)
    }

    fn add_sensitivity(
        &mut self,
        check: &Checker<'_>,
        rows: Vec<SensitivityRow>,
        [java, bedrock]: [&VersionOrder; 2],
    ) -> Result<(), ConstraintsError> {
        let table = "semantic_sensitivity";
        for (at, row) in rows.into_iter().enumerate() {
            let ids = check.ids(table, at, &row.ids)?;
            let order = match row.edition {
                Edition::Java => java,
                Edition::Bedrock => bedrock,
            };
            if !matches!(order.place(&row.boundary), FloorPlacement::At(_)) {
                return Err(ConstraintsError::UnknownBoundary {
                    edition: row.edition,
                    boundary: row.boundary,
                });
            }
            let reason = row.reason.trim();
            if reason.is_empty() {
                return Err(malformed(table, &ids, "it gives no reason"));
            }
            self.sensitivity.push(Sensitivity {
                edition: row.edition,
                boundary: row.boundary,
                reason: reason.to_owned(),
                ids,
            });
        }
        Ok(())
    }

    fn add_attachment(
        &mut self,
        check: &Checker<'_>,
        rows: Vec<AttachmentRow>,
    ) -> Result<(), ConstraintsError> {
        let table = "attachment";
        for (at, row) in rows.into_iter().enumerate() {
            let ids = check.ids(table, at, &row.ids)?;
            let reads: &[&str] = match row.hangs_by {
                HangsBy::Face => &["face", "facing"],
                HangsBy::Facing => &["facing"],
            };
            let when = IndexMap::new();
            check.states(table, &ids, &when, &row.defaults, reads)?;
            self.add_rules(
                &ids,
                &Rule {
                    table,
                    when,
                    defaults: row.defaults,
                    kind: RuleKind::Hangs(row.hangs_by),
                },
            )?;
        }
        Ok(())
    }

    fn add_support(
        &mut self,
        check: &Checker<'_>,
        rows: Vec<SupportRow>,
    ) -> Result<(), ConstraintsError> {
        let table = "support";
        for (at, row) in rows.into_iter().enumerate() {
            let ids = check.ids(table, at, &row.ids)?;
            check.states(table, &ids, &row.when, &row.defaults, &[])?;
            let also_on = if row.also_on.is_empty() {
                Vec::new()
            } else {
                check.ids(table, at, &row.also_on)?
            };
            let host = match row.on {
                Side::Below => Face::Down,
                Side::Above => Face::Up,
            };
            let needs = match row.needs {
                Needs::Sturdy => Support::Sturdy,
                Needs::Center => Support::Center,
            };
            self.add_rules(
                &ids,
                &Rule {
                    table,
                    when: row.when,
                    defaults: row.defaults,
                    kind: RuleKind::Rests {
                        host,
                        needs,
                        also_on,
                    },
                },
            )?;
        }
        Ok(())
    }

    fn add_gravity(
        &mut self,
        check: &Checker<'_>,
        rows: Vec<GravityRow>,
    ) -> Result<(), ConstraintsError> {
        let table = "gravity";
        for (at, row) in rows.into_iter().enumerate() {
            let ids = check.ids(table, at, &row.ids)?;
            self.add_rules(
                &ids,
                &Rule {
                    table,
                    when: IndexMap::new(),
                    defaults: IndexMap::new(),
                    kind: RuleKind::Falls,
                },
            )?;
        }
        Ok(())
    }

    fn add_centres(
        &mut self,
        check: &Checker<'_>,
        rows: Vec<CenterRow>,
    ) -> Result<(), ConstraintsError> {
        let table = "center_support";
        for (at, row) in rows.into_iter().enumerate() {
            let ids = check.ids(table, at, &row.ids)?;
            check.states(table, &ids, &row.when, &row.defaults, &[])?;
            let faces = row
                .faces
                .iter()
                .map(|name| {
                    Face::from_name(name)
                        .ok_or_else(|| malformed(table, &ids, &format!("`{name}` is no face")))
                })
                .collect::<Result<Faces, _>>()?;
            if faces == Faces::NONE {
                return Err(malformed(table, &ids, "it names no face"));
            }
            for id in &ids {
                self.centres.entry(id.clone()).or_default().push(Centre {
                    when: row.when.clone(),
                    defaults: row.defaults.clone(),
                    faces,
                });
            }
        }
        Ok(())
    }

    fn add_entities(
        &mut self,
        namespace: &str,
        rows: Vec<EntityAabbRow>,
    ) -> Result<(), ConstraintsError> {
        let table = "entity_aabb";
        for (at, row) in rows.into_iter().enumerate() {
            let entities: Vec<String> = row
                .entities
                .iter()
                .map(|entity| namespaced(namespace, entity))
                .collect();
            if entities.is_empty() {
                return Err(ConstraintsError::EmptyRow {
                    table,
                    index: at,
                    missing: "entity",
                });
            }
            let sizes = [
                ("width", Some(row.width)),
                ("height", Some(row.height)),
                ("depth", row.depth),
            ];
            for (name, size) in sizes {
                if size.is_some_and(|size| !size.is_finite() || size <= 0.0) {
                    return Err(malformed(
                        table,
                        &entities,
                        &format!("`{name}` is not a positive number of blocks"),
                    ));
                }
            }
            let entity_box = EntityBox {
                width: row.width,
                height: row.height,
                depth: row.depth,
            };
            for entity in entities {
                if self.entities.insert(entity.clone(), entity_box).is_some() {
                    return Err(malformed(
                        table,
                        &[entity],
                        "another row gives the entity a box",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Add `rule` to each of `ids`, refusing one a state of the block already meets another of.
    fn add_rules(&mut self, ids: &[String], rule: &Rule) -> Result<(), ConstraintsError> {
        for id in ids {
            let rules = self.rules.entry(id.clone()).or_default();
            if let Some(other) = rules.iter().find(|other| !exclusive(other, rule)) {
                return Err(ConstraintsError::OverlappingRules {
                    id: id.clone(),
                    first: other.table,
                    second: rule.table,
                });
            }
            rules.push(rule.clone());
        }
        Ok(())
    }

    /// The sha256 of the catalog's text, as the lockfile records it.
    #[must_use]
    pub fn hash(&self) -> &HashHex {
        &self.hash
    }

    /// What the canonical block `id` with `properties` needs of the cells beside it, or `None`
    /// when the catalog gives it no rule.
    #[must_use]
    pub fn constraint(
        &self,
        id: &str,
        properties: &IndexMap<String, String>,
    ) -> Option<Constraint> {
        self.rule(id, properties)?.constraint(properties)
    }

    /// Whether the catalog lets the canonical block `id` with `properties` rest on `host`, a
    /// canonical id, whatever the faces of `host`.
    #[must_use]
    pub fn rests_on_any_face(
        &self,
        id: &str,
        properties: &IndexMap<String, String>,
        host: &str,
    ) -> bool {
        self.rule(id, properties)
            .is_some_and(|rule| match &rule.kind {
                RuleKind::Rests { also_on, .. } => also_on.iter().any(|held| held == host),
                RuleKind::Hangs(_) | RuleKind::Falls => false,
            })
    }

    /// Whether the catalog says the canonical block `id` with `properties` gives a centre by
    /// `face`, beyond a whole face or a post.
    #[must_use]
    pub fn gives_centre(
        &self,
        id: &str,
        properties: &IndexMap<String, String>,
        face: Face,
    ) -> bool {
        self.centres.get(id).is_some_and(|centres| {
            centres.iter().any(|centre| {
                centre.faces.contains(face) && meets(&centre.when, &centre.defaults, properties)
            })
        })
    }

    /// Each boundary version of `edition` across which the canonical block `id` means something
    /// else, in catalog order.
    pub fn boundaries_of<'a>(
        &'a self,
        edition: Edition,
        id: &'a str,
    ) -> impl Iterator<Item = Boundary<'a>> + 'a {
        self.sensitivity
            .iter()
            .filter(move |row| row.edition == edition && row.ids.iter().any(|held| held == id))
            .map(|row| Boundary {
                version: &row.boundary,
                reason: &row.reason,
            })
    }

    /// The box of the entity `id`, namespaced, or `None` when the catalog gives it none.
    #[must_use]
    pub fn entity_box(&self, id: &str) -> Option<EntityBox> {
        self.entities.get(id).copied()
    }

    fn rule(&self, id: &str, properties: &IndexMap<String, String>) -> Option<&Rule> {
        self.rules
            .get(id)?
            .iter()
            .find(|rule| meets(&rule.when, &rule.defaults, properties))
    }
}

/// Whether no state meets both `a` and `b`: some property both read takes a different value in
/// each.
fn exclusive(a: &Rule, b: &Rule) -> bool {
    a.when
        .iter()
        .any(|(key, value)| b.when.get(key).is_some_and(|other| other != value))
}

/// What every row of one catalog is held to.
struct Checker<'a> {
    namespace: &'a str,
    java: &'a BlocksIndex,
}

impl Checker<'_> {
    /// `ids` namespaced, refusing an empty list and an id no Java version declares.
    fn ids(
        &self,
        table: &'static str,
        index: usize,
        ids: &[String],
    ) -> Result<Vec<String>, ConstraintsError> {
        if ids.is_empty() {
            return Err(ConstraintsError::EmptyRow {
                table,
                index,
                missing: "block",
            });
        }
        ids.iter()
            .map(|id| {
                let id = namespaced(self.namespace, id);
                if self.java.declared_by_some_version(&id).unwrap_or(true) {
                    Ok(id)
                } else {
                    Err(ConstraintsError::UnknownId { table, id })
                }
            })
            .collect()
    }

    /// Refuse a `when` or `defaults` pair no Java version gives the blocks, a key read without a
    /// default, and a default nothing reads; `reads` are the keys the row reads beside `when`.
    fn states(
        &self,
        table: &'static str,
        ids: &[String],
        when: &IndexMap<String, String>,
        defaults: &IndexMap<String, String>,
        reads: &[&str],
    ) -> Result<(), ConstraintsError> {
        let read: Vec<&str> = when
            .keys()
            .map(String::as_str)
            .chain(reads.iter().copied())
            .collect();
        if let Some(missing) = read.iter().find(|key| !defaults.contains_key(**key)) {
            return Err(malformed(
                table,
                ids,
                &format!("the row reads `{missing}`, which `defaults` does not give"),
            ));
        }
        if let Some(unread) = defaults.keys().find(|key| !read.contains(&key.as_str())) {
            return Err(malformed(
                table,
                ids,
                &format!("`defaults` gives `{unread}`, which the row does not read"),
            ));
        }
        let blocks = self.java;
        for id in ids {
            for (key, value) in when.iter().chain(defaults) {
                let taken = blocks.versions().any(|version| {
                    blocks
                        .states_of(version, id)
                        .and_then(|states| states.domain(key))
                        .is_some_and(|values| values.iter().any(|held| held.to_string() == *value))
                });
                if !taken {
                    return Err(ConstraintsError::MalformedRow {
                        table,
                        id: id.clone(),
                        problem: format!("no version gives the block `{key}={value}`"),
                    });
                }
            }
        }
        Ok(())
    }
}

/// A [`ConstraintsError::MalformedRow`] naming the row by the first of `ids`.
fn malformed(table: &'static str, ids: &[String], problem: &str) -> ConstraintsError {
    ConstraintsError::MalformedRow {
        table,
        id: ids.first().cloned().unwrap_or_default(),
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests;
