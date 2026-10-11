//! The version axes `cairn info` reports (`spec/versioning-editions`): the declared registry range,
//! per-edition portability, semantic-sensitive members, and the buildable targets.

use std::collections::HashSet;

use serde::Serialize;

use crate::ast::{Arg, Header, Item, ItemKind, Module, RawRequirement, Statement};
use crate::diagnostic::RenderedDiagnostic;
use crate::edition::Edition;
use crate::error::{Position, Span};
use crate::version::{FloorVerdict, VersionOrder, compare_versions, parse_min_version};

use super::theme_variant::{bound_theme_name, pick_variant, single_logical_theme};

/// The three-axis answer to "which version is this `.crn` for?".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VersionAxes {
    /// Axis 1: the version range the file declares.
    pub registry_compat: RegistryRange,
    /// Axis 2: per-edition portability counts, in the caller's order (the CLI's `--editions`).
    pub edition_portability: Vec<EditionPortability>,
    /// Axis 3: members whose meaning shifts at a boundary version. Always empty: no catalog exists.
    pub semantic_sensitive: Vec<SemanticSensitiveFinding>,
    /// Per edition, in `edition_portability` order, the supported versions that build the source.
    pub buildable_targets: Vec<BuildableTargets>,
    /// The versions `@intended_targets` names, verbatim and in source order.
    pub intended_targets: Vec<String>,
}

/// Everything one requested edition contributes to [`VersionAxes`].
#[derive(Debug, Clone, PartialEq)]
pub struct EditionReport {
    /// Target edition this report describes.
    pub edition: Edition,
    /// Palette entries that compile straight through.
    pub portable: u32,
    /// Palette entries with no representable form on this edition.
    pub unsupported_entries: Vec<UnsupportedEntry>,
    /// Palette entries that compile but lose detail, and what each lost.
    pub degraded_entries: Vec<DegradedEntry>,
    /// Versions from [`Self::considered`] a build would accept.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares.
    pub considered: Vec<String>,
    /// Why [`Self::buildable`] is empty, when it is.
    pub refusal: Option<BuildableRefusal>,
}

/// Which supported versions of one edition can build the source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildableTargets {
    /// Target edition this row describes.
    pub edition: Edition,
    /// The versions in [`Self::considered`] that pass `cairn compile --target`'s source gates.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares, in ascending release order.
    pub considered: Vec<String>,
    /// Why [`Self::buildable`] is empty, when it is. Omitted from the wire when a version builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<BuildableRefusal>,
}

/// Why no supported version of one edition can build the source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildableRefusal {
    /// Floors this edition's version table cannot place, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unplaceable_floors: Vec<DeclaredFloor>,
    /// Scopes and walkways that produced no voxels, as keys opaque to this crate.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped_scopes: Vec<String>,
    /// Versions refused for a reason of their own, a subsequence of `considered`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<RefusedTarget>,
}

/// One supported version that cannot build the source, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RefusedTarget {
    /// The version, as the registry pack spells it.
    pub version: String,
    /// What refused it.
    #[serde(flatten)]
    pub refusal: TargetRefusal,
}

/// Why one supported version cannot build the source.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum TargetRefusal {
    /// The version is below a floor the source declares.
    BelowFloor {
        /// The floors that refuse this version, in source order.
        floors: Vec<DeclaredFloor>,
    },
    /// The lowering pinned to this version raised errors.
    LoweringRefused {
        /// Its findings, shaped as `spec/lint` "Machine-readable payload" says.
        findings: Vec<RenderedDiagnostic>,
    },
}

/// A version floor as a reader acts on it: what it says, where, and which part declared it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeclaredFloor {
    /// The floor as the author wrote it ([`VersionFloor::rendered`]).
    pub declared: String,
    /// Where the `@requires` directive or `requires` line is written.
    #[serde(flatten)]
    pub position: Position,
    /// The part that declared it; absent for a floor on the file itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<FloorDeclarer>,
}

/// The part a floor was inherited from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FloorDeclarer {
    /// The kind of part, by its surface keyword.
    pub keyword: FloorPart,
    /// The part's name.
    pub name: String,
}

/// The kinds of part a floor can be inherited from, spelled as [`ItemKind::keyword`] spells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloorPart {
    /// `def NAME`, inherited by every `place use=NAME`.
    Def,
    /// `theme NAME`, inherited by every scope that binds the theme.
    Theme,
}

/// The version range the file declares, per `spec/versioning-editions` "The `registry
/// compatibility` row".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryRange {
    /// Lower bound (inclusive).
    pub min: String,
    /// Upper bound (inclusive), always `"latest"`.
    pub max: String,
}

/// Per-edition portability counts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditionPortability {
    /// Target edition this row's counts describe.
    pub edition: Edition,
    /// Palette entries that compile straight through.
    pub portable: u32,
    /// Palette entries that compile but lose detail (e.g. stair `shape` on Bedrock).
    pub degraded: u32,
    /// Palette entries with no representable form on this edition.
    pub unsupported: u32,
    /// The entries [`Self::unsupported`] counts, each with why it has no form here.
    pub unsupported_entries: Vec<UnsupportedEntry>,
    /// The entries [`Self::degraded`] counts, each with what it lost.
    pub degraded_entries: Vec<DegradedEntry>,
}

/// One palette entry that compiles into an edition and loses detail doing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DegradedEntry {
    /// The palette entry's block id, verbatim as the lowering interned it.
    pub id: String,
    /// The entry's `key=value` pairs, comma-joined; an id appears once per state combination.
    pub states: String,
    /// What the edition had no form for, one per intent dropped.
    pub dropped: Vec<DroppedIntent>,
}

/// One piece of intent an edition cannot represent, as pieces rather than a sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "key", rename_all = "snake_case")]
pub enum DroppedIntent {
    /// A stair `shape` the edition has no state for.
    Shape {
        /// The value asked for and not written (`outer_left`).
        value: String,
    },
}

/// One palette entry an edition has no form for, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsupportedEntry {
    /// The palette entry's block id, verbatim as the lowering interned it.
    pub id: String,
    /// Which of the two ways this entry has no form on the edition.
    #[serde(flatten)]
    pub reason: UnsupportedReason,
}

/// Why one palette entry counts as unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// No supported version of the edition declares the block, so there are no states to ask about.
    AbsentFromEdition {
        /// The nearest id the edition declares, when close enough to be a plausible typo.
        suggestion: Option<String>,
        /// The ids this edition declares for the same block, from the registry pack's alias table.
        aliases: Vec<String>,
    },
    /// The edition has the block, but this compiler's backend maps none of these states.
    StatesUnmapped {
        /// The entry's `key=value` pairs, comma-joined.
        states: String,
        /// The families the backend does map, as the message lists them.
        mapped: String,
    },
}

/// One semantic-sensitivity finding; never produced, since no catalog exists.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticSensitiveFinding {
    /// Identifier or keyword the finding refers to (`yard_water`, `wall`, ...).
    pub member: String,
    /// Human-readable reason (`"cauldron split at 1.17"`).
    pub reason: String,
    /// Version at which the meaning shifts.
    pub boundary_version: String,
}

/// The version axes for `cairn info`, from `module` and the caller's per-edition reports.
#[must_use]
pub fn compute_axes(module: &Module, editions: Vec<EditionReport>) -> VersionAxes {
    let mut edition_portability = Vec::with_capacity(editions.len());
    let mut buildable_targets = Vec::with_capacity(editions.len());
    for report in editions {
        edition_portability.push(EditionPortability {
            edition: report.edition,
            portable: report.portable,
            degraded: named_count(report.degraded_entries.len()),
            unsupported: named_count(report.unsupported_entries.len()),
            unsupported_entries: report.unsupported_entries,
            degraded_entries: report.degraded_entries,
        });
        buildable_targets.push(BuildableTargets {
            edition: report.edition,
            buildable: report.buildable,
            considered: report.considered,
            reason: report.refusal,
        });
    }
    VersionAxes {
        registry_compat: RegistryRange {
            min: derive_min_version(module),
            max: "latest".to_owned(),
        },
        edition_portability,
        semantic_sensitive: Vec::new(),
        buildable_targets,
        intended_targets: declared_intended_targets(module),
    }
}

/// A list's length as its figure; `PALETTE_CAPACITY` keeps the clamp unreachable.
fn named_count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The versions every `@intended_targets` header names, in source order.
fn declared_intended_targets(module: &Module) -> Vec<String> {
    module
        .headers
        .iter()
        .filter_map(|header| match header {
            Header::IntendedTargets { targets, .. } => Some(targets.iter().cloned()),
            Header::Cairn { .. } | Header::Requires { .. } => None,
        })
        .flatten()
        .collect()
}

/// The `registry compatibility` row's lower edge: the strictest floor that names no edition.
///
/// Picked by label comparison, which is sound only because this row decides no build.
fn derive_min_version(module: &Module) -> String {
    unscoped_version_floors(module)
        .into_iter()
        .reduce(|best, next| {
            if compare_versions(&next.version, &best.version).is_gt() {
                next
            } else {
                best
            }
        })
        .map_or_else(|| "0.0".to_owned(), |floor| floor.version)
}

/// Every version floor a build of `edition` is held to, unfolded and in source order.
///
/// Covers the headers and every part the build instantiates, per `spec/versioning-editions`
/// "Fail-loud and minimum-version inference". Requirements the grammar refuses are skipped.
#[must_use]
pub fn declared_version_floors(module: &Module, edition: Edition) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Pinned(edition))
}

/// The floors that constrain the file without naming an edition.
///
/// A `theme` contributes only when both editions bind the same one.
#[must_use]
pub fn unscoped_version_floors(module: &Module) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Neutral)
}

/// Which build is asking for a module's floors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloorScope {
    /// A build of one edition: its own floors and the unscoped ones.
    Pinned(Edition),
    /// The edition-neutral row: unscoped floors, from parts both editions inherit alike.
    Neutral,
}

impl FloorScope {
    /// Whether a floor scoped to `declared` constrains this build; another edition's is inert.
    fn keeps(self, declared: Option<Edition>) -> bool {
        match self {
            Self::Pinned(edition) => declared.is_none_or(|declared| declared == edition),
            Self::Neutral => declared.is_none(),
        }
    }
}

/// The floors `scope` accepts, headers first, in source order.
fn collect_floors(module: &Module, scope: FloorScope) -> Vec<VersionFloor> {
    let instantiated = InstantiatedParts::of(module, scope);
    let mut floors = Vec::new();
    let mut push = |requirement: &RawRequirement, span: &Span, origin: &FloorOrigin| {
        if let Some(parsed) = parse_min_version(requirement.as_str())
            && scope.keeps(parsed.edition)
        {
            floors.push(VersionFloor {
                version: parsed.version.to_owned(),
                edition: parsed.edition,
                origin: origin.clone(),
                span: span.clone(),
            });
        }
    };
    for header in &module.headers {
        if let Header::Requires { requirement, span } = header {
            push(requirement, span, &FloorOrigin::Module);
        }
    }
    for item in &module.items {
        // A part declared twice is `E_DUPLICATE_ITEM`, so walking both copies decides nothing.
        let origin = match item {
            Item::Def { name, .. } if instantiated.defs.contains(name.as_str()) => {
                FloorOrigin::Def(name.clone())
            }
            Item::Theme { name, .. } if instantiated.themes.contains(name.as_str()) => {
                FloorOrigin::Theme(name.clone())
            }
            _ => continue,
        };
        for line in item.requires() {
            push(&line.requirement, &line.span, &origin);
        }
    }
    floors
}

/// The parts of a module a build actually instantiates, by declared name.
struct InstantiatedParts<'a> {
    /// Defs some `place use=NAME` in a `site` names.
    defs: HashSet<&'a str>,
    /// Themes some scope binds.
    themes: HashSet<&'a str>,
}

impl<'a> InstantiatedParts<'a> {
    /// The parts `module`'s placements and its module-level auto-pick bind.
    ///
    /// A theme's floor applies once it is bound, whether or not a slot is read from it.
    /// The auto-pick is read only for a `struct`, since an unplaced `def` builds nothing.
    fn of(module: &'a Module, scope: FloorScope) -> Self {
        let declared_defs: HashSet<&str> = module
            .items
            .iter()
            .filter(|item| item.kind() == ItemKind::Def)
            .map(Item::name)
            .collect();
        let declared_themes: Vec<&str> = module
            .items
            .iter()
            .filter(|item| item.kind() == ItemKind::Theme)
            .map(Item::name)
            .collect();

        let mut parts = Self {
            defs: HashSet::new(),
            themes: HashSet::new(),
        };
        let declares_a_struct = module
            .items
            .iter()
            .any(|item| item.kind() == ItemKind::Struct);
        if declares_a_struct
            && let Some(logical) = single_logical_theme(declared_themes.iter().copied())
            && let Some(name) = bound_alike(
                &declared_themes,
                logical,
                scope,
                |names, logical, edition| pick_variant(names.iter().copied(), logical, edition),
            )
        {
            parts.themes.insert(name);
        }
        for item in &module.items {
            let Item::Site { body, .. } = item else {
                continue;
            };
            parts.walk(body, &declared_defs, &declared_themes, scope);
        }
        parts
    }

    /// Collects the `place` lines of one body and the bodies under it.
    fn walk(
        &mut self,
        body: &'a [Statement],
        declared_defs: &HashSet<&'a str>,
        declared_themes: &[&'a str],
        scope: FloorScope,
    ) {
        for statement in body {
            let Statement::Generic {
                keyword,
                args,
                children,
                ..
            } = statement
            else {
                continue;
            };
            self.walk(children, declared_defs, declared_themes, scope);
            if keyword != "place" {
                continue;
            }
            if let Some(name) = label_arg(args, "use")
                && let Some(declared) = declared_defs.get(name)
            {
                self.defs.insert(declared);
            }
            if let Some(written) = label_arg(args, "theme")
                && let Some(bound) = bound_alike(
                    declared_themes,
                    written,
                    scope,
                    |names, written, edition| {
                        bound_theme_name(names.iter().copied(), written, edition)
                    },
                )
            {
                self.themes.insert(bound);
            }
        }
    }
}

/// The theme `bind` selects under `scope`; a neutral scope keeps it only when both editions agree.
fn bound_alike<'a>(
    names: &[&'a str],
    written: &str,
    scope: FloorScope,
    bind: impl Fn(&[&'a str], &str, Option<Edition>) -> Option<&'a str>,
) -> Option<&'a str> {
    match scope {
        FloorScope::Pinned(edition) => bind(names, written, Some(edition)),
        FloorScope::Neutral => {
            let java = bind(names, written, Some(Edition::Java))?;
            let bedrock = bind(names, written, Some(Edition::Bedrock))?;
            (java == bedrock).then_some(java)
        }
    }
}

/// The label value of `key` among `args`; `None` also for a non-label value (`use=3`).
fn label_arg<'a>(args: &'a [Arg], key: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.key == key)
        .and_then(|arg| arg.value.as_label_str())
}

/// A version floor a module declares, with the directive it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionFloor {
    /// The version label as written, already known to be version-shaped.
    pub version: String,
    /// The edition the directive scoped the floor to, when it named one.
    pub edition: Option<Edition>,
    /// Which part of the module declared it.
    pub origin: FloorOrigin,
    /// Byte range of the `@requires` directive or `requires` line.
    pub span: Span,
}

impl VersionFloor {
    /// The floor as the author wrote it, scope and all.
    #[must_use]
    pub fn rendered(&self) -> String {
        match self.edition {
            Some(edition) => format!("{edition} version>={}", self.version),
            None => format!("version>={}", self.version),
        }
    }

    /// Who declared the floor, as a sentence's subject: "this file" or the part.
    #[must_use]
    pub fn declarer(&self) -> String {
        self.origin.part().map_or_else(
            || "this file".to_owned(),
            |(keyword, name)| format!("`{keyword} {name}`"),
        )
    }
}

/// The versions in `considered` that satisfy every floor (`spec/lint` "Diagnostic codes").
///
/// A floor this edition's table cannot place refuses nothing here: it is `E_REQUIRES_UNORDERABLE`.
#[must_use]
pub fn versions_satisfying<'a>(
    order: &VersionOrder,
    floors: &[VersionFloor],
    considered: &'a [String],
) -> Vec<&'a str> {
    considered
        .iter()
        .filter(|label| {
            order.key_of(label).is_some_and(|key| {
                floors
                    .iter()
                    .all(|floor| order.verdict(&floor.version, key) == FloorVerdict::Satisfied)
            })
        })
        .map(String::as_str)
        .collect()
}

/// Which part of a module a [`VersionFloor`] came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorOrigin {
    /// An `@requires` header: a floor on the file itself.
    Module,
    /// A `requires` line in `def NAME`, inherited by every `place use=NAME`.
    Def(String),
    /// A `requires` line in `theme NAME`, inherited by every scope that binds the theme.
    Theme(String),
}

impl FloorOrigin {
    /// The part as `(keyword, name)`, or `None` for a module-level floor.
    #[must_use]
    pub fn part(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Module => None,
            Self::Def(name) => Some((ItemKind::Def.keyword(), name)),
            Self::Theme(name) => Some((ItemKind::Theme.keyword(), name)),
        }
    }
}

#[cfg(test)]
mod tests;
