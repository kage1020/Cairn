//! The three version axes `cairn info` reports, from `spec/versioning-editions`, the section on the
//! three answers to "which version is it for?":
//!
//! 1. **declared registry range** `[Vmin, Vmax]`: the strictest version floor the file declares
//!    without naming an edition, against an open upper edge. A reading of the file rather than a
//!    fact derived from the blocks it uses; `spec/versioning-editions` "The `registry
//!    compatibility` row" says why, and which row carries the derived answer.
//! 2. **edition portability**: per edition, how many palette entries compile portably, are
//!    degraded, or are unsupported. The caller supplies the figures, since they come from
//!    `cairn-lang-formats::portability` and `core` does not depend on `formats`.
//! 3. **semantic-sensitive members**: registry-valid ids whose meaning or behavior shifts at a
//!    known boundary version (the catalog half of `spec/versioning-editions` "Backend = data
//!    tables"). Always empty, since no semantic-sensitivity catalog exists.
//!
//! Beside them sits **buildable targets**, the answer axis (2) does not give. Portability asks of
//! the *edition*, so two entries declared by disjoint sets of versions each answer yes; this row
//! asks of each version in turn, so a source no supported version can build cannot report clean.
//!
//! Axis (1) reads the module's `@requires` headers and the `requires` lines of every part the build
//! instantiates (`spec/versioning-editions` "Fail-loud and minimum-version inference").

use std::collections::HashSet;

use serde::Serialize;

use crate::ast::{Arg, Header, Item, ItemKind, Module, RawRequirement, Statement};
use crate::diagnostic::RenderedDiagnostic;
use crate::edition::Edition;
use crate::error::{Position, Span};
use crate::version::{FloorVerdict, VersionOrder, compare_versions, parse_min_version};

use super::theme_variant::{bound_theme_name, pick_variant, single_logical_theme};

/// The three-axis answer to "which version is this `.crn` for?".
///
/// `#[non_exhaustive]` because this type is only *read* outside the crate: [`compute_axes`] builds
/// it and the CLI renders it, so a new axis should not be a breaking change. [`EditionReport`],
/// which the caller *builds*, stays exhaustive: a field added there is a value the caller has to
/// supply, and the compiler should say so.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VersionAxes {
    /// Axis 1: the version range the file declares.
    pub registry_compat: RegistryRange,
    /// Axis 2: per-edition portability counts, in the caller's order (the CLI's `--editions`).
    pub edition_portability: Vec<EditionPortability>,
    /// Axis 3: members whose meaning shifts at a known boundary version. Always empty, since no
    /// semantic-sensitivity catalog exists.
    pub semantic_sensitive: Vec<SemanticSensitiveFinding>,
    /// Which supported versions of each requested edition can build the
    /// source. One entry per entry of [`Self::edition_portability`], in
    /// the same order and for the same edition — both are fanned out of
    /// one [`EditionReport`] per edition, so the two cannot disagree.
    pub buildable_targets: Vec<BuildableTargets>,
    /// The versions `@intended_targets` names, verbatim and in source order.
    ///
    /// Not an axis: the file's own statement of what it was designed for, reported beside
    /// [`Self::buildable_targets`] because that is the row it can contradict.
    ///
    /// Verbatim, including a label no edition ships: whether a version exists is the answer of
    /// `buildable targets` and `W_INTENDED_TARGET_UNSUPPORTED`, and a row that dropped one would
    /// misreport the file.
    pub intended_targets: Vec<String>,
}

/// Everything one requested edition contributes to [`VersionAxes`].
///
/// The wire keeps the portability counts and the buildable versions in separate lists. Taking them
/// from the caller that way would leave three invariants unenforced (equal length, matching order,
/// one row per edition), so the caller builds one of these per edition and [`compute_axes`] fans
/// them out.
#[derive(Debug, Clone, PartialEq)]
pub struct EditionReport {
    /// Target edition this report describes.
    pub edition: Edition,
    /// Palette entries that compile straight through.
    ///
    /// A bare figure: such an entry has nothing to say about itself beyond being counted.
    pub portable: u32,
    /// Palette entries with no representable form on this edition.
    ///
    /// The wire's `unsupported` figure is this list's length, derived by [`compute_axes`], so a
    /// caller cannot write `unsupported: 1` beside an empty list.
    pub unsupported_entries: Vec<UnsupportedEntry>,
    /// Palette entries that compile but lose detail, and what each lost.
    ///
    /// The wire's `degraded` figure is this list's length, as with [`Self::unsupported_entries`].
    pub degraded_entries: Vec<DegradedEntry>,
    /// Versions from [`Self::considered`] a build would accept.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares.
    pub considered: Vec<String>,
    /// Why [`Self::buildable`] is empty, when it is.
    ///
    /// `None` when a version builds, which keeps the key off the ordinary report. The caller
    /// decides it, since it weighed the floors, the scopes and the pinned lowerings.
    pub refusal: Option<BuildableRefusal>,
}

/// Which supported versions of one edition can build the source.
///
/// Separate from [`EditionPortability`] because only this question has a per-version answer:
/// portability counts palette entries against the edition, and an id only part of the range spells
/// that way is not missing from the edition, so every entry can pass while no single version passes
/// them all.
///
/// A list rather than a `[min, max]` range: two ids whose version sets interleave leave a gap, and
/// a range would claim it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildableTargets {
    /// Target edition this row describes.
    pub edition: Edition,
    /// The versions in [`Self::considered`] a build would accept, in the same order.
    ///
    /// "Would accept" means the gates `cairn compile --target` applies before it writes anything:
    /// the pinned lowering raises no error, the source's `@requires` floor is at or below the
    /// version, and every scope the source declares lowered. The gates after that concern the
    /// filesystem, not the source, and are not asked here.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares, in ascending release order.
    ///
    /// Carried so an empty `buildable` can be read: "no version builds this" and "the pack declares
    /// no versions" are different facts.
    pub considered: Vec<String>,
    /// Why [`Self::buildable`] is empty, when it is. Omitted from the wire when a version builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<BuildableRefusal>,
}

/// Why no supported version of one edition can build the source.
///
/// An empty `buildable` has four causes with different repairs, so the bare list is a figure a
/// reader cannot act on.
///
/// A struct rather than one tag per cause, because causes co-occur in ordinary files. Two are facts
/// about the *edition*, the same for every release and answered before any is weighed: a floor the
/// version table cannot place, and a scope that produced no voxels. Two are facts about a
/// *version*, and a run can carry both. Reporting one cause would send the reader back for the next
/// after each repair, so the edition-wide answers sit beside the per-version list.
///
/// Fields carry the pieces of an answer rather than a sentence, as [`UnsupportedReason`] does; the
/// prose belongs to the renderer.
///
/// Nested under `reason` on [`BuildableTargets`] rather than flattened as
/// [`UnsupportedEntry::reason`] is: that one is a tagged union named by its tag, while this has no
/// tag, and nesting gives the row one key that says whether there is a reason and what it is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildableRefusal {
    /// Floors this edition's version table cannot place, in source order.
    ///
    /// Edition-wide: a floor naming no release of the edition can be weighed against none of them,
    /// so no version is certified. Distinct from a floor every release sits below; the repair is on
    /// the `@requires` line whatever the releases are.
    ///
    /// The versions are still weighed in [`Self::versions`], so fixing the floor is not a
    /// prerequisite for learning what else the file gets wrong.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unplaceable_floors: Vec<DeclaredFloor>,
    /// The scopes and walkways the source asked for that produced no voxels, as opaque keys minted
    /// by the producer of this refusal. Nothing in this crate reads them; the CLI's
    /// `dropped_scopes` and `spec/versioning-editions` define their format.
    ///
    /// Edition-wide, since a partial build is not certified under any version. Beside the
    /// per-version list rather than in it, because a version below a floor is *also* refused by the
    /// scope, and a reader repairing the floor should meet the scope on the same run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped_scopes: Vec<String>,
    /// The versions refused for a reason of their own, in `considered` order.
    ///
    /// A subsequence of `considered`, not a parallel array: a version refused only by an
    /// edition-wide answer has no entry, so a consumer joins on [`RefusedTarget::version`].
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
///
/// Two variants for two repairs, the `@requires` line and whatever the pinned lowering named. Both
/// differ between releases, so they are per-version and a run can need both; the causes identical
/// under every release are [`BuildableRefusal`]'s own fields.
///
/// Exclusive rather than a list: a floor is a relation between the source and the target that no id
/// table changes, so a version below one is not lowered at all.
///
/// Internally tagged, so a [`RefusedTarget`] reads `"refusal": "below_floor"` beside `"version"`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum TargetRefusal {
    /// The version is below a floor the source declares.
    BelowFloor {
        /// The floors it is below, in source order: only those that refuse *this* version, so a
        /// module declaring several shows the one line to repair.
        floors: Vec<DeclaredFloor>,
    },
    /// The lowering pinned to this version raised errors.
    LoweringRefused {
        /// The findings it raised, in the shape of `spec/lint` "Machine-readable payload": the same
        /// findings the run prints under the version on stderr.
        findings: Vec<RenderedDiagnostic>,
    },
}

/// A version floor as a reader has to act on it: what it says, where it is written, and which part
/// declared it.
///
/// A position rather than [`VersionFloor`]'s byte span: a consumer of this document has the file,
/// not the source string the span indexes, and `spec/lint` "Machine-readable payload" spells a
/// position this way for every finding.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeclaredFloor {
    /// The floor as the author wrote it, scope and all
    /// ([`VersionFloor::rendered`]).
    pub declared: String,
    /// Where the `@requires` directive or `requires` line is written.
    ///
    /// Flattened, so the wire carries `line` and `col` beside `declared`. [`Position`] rather than
    /// two `u32`s, so the 1-based invariant is the type's (`NonZeroU32`) rather than each
    /// producer's.
    #[serde(flatten)]
    pub position: Position,
    /// The part that declared it, absent for a floor on the file itself, whose line the position
    /// already points at.
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

/// The kinds of part a floor can be inherited from.
///
/// An enum rather than [`FloorOrigin::part`]'s `&'static str`, so that [`FloorOrigin`]'s promise
/// that a new variant breaks every caller reaches the wire too; a string field would absorb a new
/// route silently. Narrower than [`ItemKind`] on purpose: a `site` cannot declare a floor.
///
/// Spelled as [`ItemKind::keyword`] spells the same two words, pinned by
/// `floor_part_spells_the_surface_keyword`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloorPart {
    /// `def NAME`, inherited by every `place use=NAME`.
    Def,
    /// `theme NAME`, inherited by every scope that binds the theme.
    Theme,
}

/// The version range the file **declares**, per `spec/versioning-editions` "The `registry
/// compatibility` row".
///
/// `min` is the strictest **unscoped** floor (the module's `@requires version>=X` headers and the
/// `requires version>=X` line of every `def` and `theme` the build instantiates), or `"0.0"` when
/// none feeds this row. "Strictest" is [`compare_versions`]' order, and `derive_min_version` says
/// where that order is arbitrary. `max` is always `"latest"`: an upper edge belongs to a *derived*
/// range, which [`BuildableTargets`] carries.
///
/// Nothing here looks at the blocks the source uses, so `"0.0" .. "latest"` does not say every
/// version can build the file; the spec section says why the two rows stay separate.
///
/// `"0.0"` has two causes, no `@requires` line at all or only edition-scoped floors, and one row
/// for both editions cannot tell them apart. `cairn info` prints a note naming the scoped floors it
/// left out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryRange {
    /// Lower bound (inclusive).
    pub min: String,
    /// Upper bound (inclusive). Always `"latest"`; see the type's own doc
    /// for why a pack that grows an upper edge does not fill this in.
    pub max: String,
}

/// Per-edition portability counts.
///
/// `edition` is an [`Edition`] rather than a `String`, so a consumer cannot receive an unrecognised
/// label; [`Edition`]'s [`Serialize`] impl writes the lowercase name (`"edition":"java"`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditionPortability {
    /// Target edition this row's counts describe.
    pub edition: Edition,
    /// Palette entries that compile straight through.
    pub portable: u32,
    /// Palette entries that compile but lose detail (e.g. stair `shape` on
    /// Bedrock).
    pub degraded: u32,
    /// Palette entries with no representable form on this edition: a block
    /// the edition does not have, or states it cannot express.
    pub unsupported: u32,
    /// The entries [`Self::unsupported`] counts, named and with the reason each has no form here.
    ///
    /// One element per unit of the count, in palette order. The two ways an entry can be
    /// unsupported have different repairs, only one of them the author's, which a bare count cannot
    /// say.
    pub unsupported_entries: Vec<UnsupportedEntry>,
    /// The entries [`Self::degraded`] counts, named and with what each lost.
    ///
    /// One element per unit of the count, in palette order, so `cairn info` answers "which of the
    /// N" before a build does.
    pub degraded_entries: Vec<DegradedEntry>,
}

/// One palette entry that compiles into an edition and loses detail doing it.
///
/// Built by `cairn-lang-formats::portability` and declared here beside its row, as
/// [`UnsupportedEntry`] is.
///
/// Two lists rather than one under a category tag, and `states` rather than the id alone, per
/// `spec/versioning-editions` "The `edition portability` row". The figure is therefore the length,
/// and [`compute_axes`] derives both figures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DegradedEntry {
    /// The palette entry's block id, verbatim as the lowering interned it.
    pub id: String,
    /// The entry's `key=value` pairs, comma-joined, as [`UnsupportedReason::StatesUnmapped`] spells
    /// them.
    ///
    /// Degradation is a fact about the states, so one id reaches this list once per state
    /// combination. Pre-joined, unlike [`Self::dropped`], because
    /// [`BlockState::properties_text`](crate::block_array::BlockState::properties_text) already
    /// writes this spelling, and splitting it would put two spellings of one entry's states on the
    /// wire.
    pub states: String,
    /// What the edition had no form for, one per intent dropped.
    pub dropped: Vec<DroppedIntent>,
}

/// One piece of intent an edition cannot represent, as pieces rather than a sentence.
///
/// The prose belongs to the renderer: `cairn-lang-formats::bedrock_state` writes it, and the
/// build's `W_INTENT_DEGRADED` and `cairn info`'s note come off the same function there.
///
/// A closed set rather than a free `{key, value}` pair, so the sentence written for a dropped stair
/// `shape` cannot be inherited by a family with no corners. Each variant serializes under its own
/// `key`, so the wire is a `{"key", "value"}` object either way and a new variant is a new `key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "key", rename_all = "snake_case")]
pub enum DroppedIntent {
    /// A stair `shape` the edition has no state for.
    Shape {
        /// The value asked for and not written (`outer_left`). A value the edition can express
        /// never reaches this list: Bedrock's stairs are `straight`, so `shape=straight` is not a
        /// loss.
        value: String,
    },
}

/// One palette entry an edition has no form for, and why.
///
/// Built by `cairn-lang-formats::portability`, where the Bedrock state translator and the registry
/// pack's id tables live, and declared here beside its row so there is one shape rather than two
/// with a mapping between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsupportedEntry {
    /// The palette entry's block id, verbatim as the lowering interned it.
    pub id: String,
    /// Which of the two ways this entry has no form on the edition.
    #[serde(flatten)]
    pub reason: UnsupportedReason,
}

/// Why one palette entry counts as unsupported.
///
/// Two variants for two repairs: change the material, or wait for the backend to map the states.
///
/// A value outside the Java domain and a key the translator does not read are not here: both say a
/// blockstate the registry pack should have refused got through, nothing about the edition.
/// `cairn-lang-formats::portability` refuses to report figures over such a palette, which keeps
/// this enum a list of portability answers.
///
/// Variants carry the pieces of an answer rather than a sentence, so a consumer reading the tag
/// need not parse English. Internally tagged: `"reason": "absent_from_edition"` sits beside the
/// fields that reason carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// No supported version of the edition declares the block, so there are no states to ask about.
    AbsentFromEdition {
        /// The nearest id the edition declares, when one is close enough to be a plausible typo.
        /// `None` is the ordinary answer for a block of the other edition.
        suggestion: Option<String>,
        /// The ids this edition declares for the same block, from the registry pack's alias table:
        /// the case `suggestion` cannot reach, since `oak_sign` → `standing_sign` is seven edits.
        ///
        /// Empty when the pack names none, and then the edition does not have the block.
        aliases: Vec<String>,
    },
    /// The edition has the block and this compiler's backend has no mapping for the states the
    /// intent put on it: a statement about the backend, not the edition, which may accept those
    /// states in the game.
    StatesUnmapped {
        /// The entry's `key=value` pairs, comma-joined, so the row names
        /// the states rather than only the block carrying them.
        states: String,
        /// The families the backend does map, as the message lists them.
        mapped: String,
    },
}

/// One semantic-sensitivity finding.
///
/// Never produced, since no catalog exists; the type keeps the JSON shape stable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticSensitiveFinding {
    /// Identifier or keyword the finding refers to (`yard_water`,
    /// `wall`, ...).
    pub member: String,
    /// Human-readable reason (`"cauldron split at 1.17"`).
    pub reason: String,
    /// Version at which the meaning shifts.
    pub boundary_version: String,
}

/// Compute the three axes for `cairn info`.
///
/// `editions` comes from the caller and is forwarded verbatim. Classifying a palette needs the
/// Bedrock state translator in `cairn-lang-formats`, which `core` does not depend on, and deciding
/// the buildable versions needs one lowering per version against that version's id table, which is
/// the registry pack's data. The CLI's `run_info` builds it from the same dry-run lowerings the
/// compile backend would see.
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

/// One figure over a named list, as the list's own length.
///
/// The one place either figure is decided, so "the figure is the list's length" holds by
/// construction. A palette holds at most [`PALETTE_CAPACITY`](crate::block_array::PALETTE_CAPACITY)
/// (65,536) entries, so the clamp is unreachable rather than lossy.
fn named_count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The versions every `@intended_targets` header names, in source order.
///
/// Concatenated rather than first-wins: a second header is `E_DUPLICATE_HEADER` and the file is
/// refused before this row renders, so the choice decides nothing, and appending keeps this a
/// rendering of the file.
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

/// The `registry compatibility` row's lower edge, as text.
///
/// Only floors that name no edition feed it: the row is one range for a file reported against both
/// editions, and a floor in one edition's numbering means nothing in the other's.
///
/// The strictest is picked by [`compare_versions`], the label comparison, because the `DataVersion`
/// tables are per edition and this row has none. That is sound only because the row renders a
/// version the author wrote and decides no build: `E_VERSION_CAP` and `buildable targets` weigh
/// each floor against the target edition's table and never compare two floors.
///
/// A label the comparison cannot read as a number sorts above every one it can, so
/// `version>=1.21.4` beside `version>=24w14a` reports the snapshot. Fixed rather than meaningful,
/// which is all a total order promises.
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

/// Every version floor `module` declares that a build of `edition` is held to, in source order.
///
/// Floors compose by intersection (`spec/syntax` "Headers"): `[a, ∞) ∩ [b, ∞)` is `[max(a, b), ∞)`.
/// They are returned unfolded because the fold needs `DataVersion`, a per-edition table this crate
/// does not hold; a caller weighs each floor against the target and reports the first that refuses
/// it, without comparing two floors.
///
/// **The list is the composite's, not the file header's.** `spec/versioning-editions` "Fail-loud
/// and minimum-version inference" gives a `def` and a `theme` floors of their own and makes a
/// composite's minimum the max of its parts, so the walk covers the `@requires` headers plus every
/// part the build instantiates: a `def` some `place use=` names, and a `theme` some scope binds. A
/// part nothing instantiates contributes nothing: an unplaced `def` builds no voxels, and its floor
/// would refuse targets over a template left in the file.
///
/// `edition` keeps the floors scoped to it and the unscoped ones, and picks the variant a `theme=`
/// binds by the resolver's own rule. The edition-neutral question is [`unscoped_version_floors`]
/// rather than a `None` here, since a floor's own `None` means "every edition" and the argument's
/// would mean "no edition".
///
/// Requirements the grammar refuses declare nothing and are skipped; `check::requires` reports them
/// at `Error` severity, which stops a compile before it is held to half an expression.
/// `crates/cairn-lang-core/tests/resolve/silent_skip_arms.rs` covers a caller that bypasses
/// `check`.
#[must_use]
pub fn declared_version_floors(module: &Module, edition: Edition) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Pinned(edition))
}

/// The floors that constrain the file without naming an edition.
///
/// The question `cairn info`'s `registry compatibility` row asks: one row for both editions, so
/// only a floor that means something in both can feed it.
///
/// A part's floor is held to the same test through the part it is inherited by. A `theme`
/// contributes only when both editions bind the same one (`InstantiatedParts::of`): the unpinned
/// variant order matches no build, so `theme shop` with a floor beside `theme shop_bedrock` without
/// one would report a lower edge a Bedrock build does not have, and the reverse pairing `0.0` for a
/// Java build held to one. A `def` needs no such test, since `place use=NAME` names one def, not a
/// family of per-edition variants.
#[must_use]
pub fn unscoped_version_floors(module: &Module) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Neutral)
}

/// Which build is asking for a module's floors.
///
/// One value rather than an `Option<Edition>` beside a predicate, so `collect_floors(module,
/// Some(Java), |declared| declared.is_none())`, a question nothing asks, cannot be written: both
/// halves follow from the same fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloorScope {
    /// A build of one edition. Keeps the floors scoped to it and the
    /// unscoped ones, since an unscoped floor is a floor on whatever is
    /// being built, and binds the theme variants that build binds.
    Pinned(Edition),
    /// The edition-neutral row. Keeps only the floors that name no edition,
    /// and only the parts both editions inherit alike.
    Neutral,
}

impl FloorScope {
    /// Whether a floor scoped to `declared` constrains this build.
    ///
    /// A scoped floor is in its own edition's build and inert in the
    /// other's — inert, not violated, which is the reading that would make
    /// a file declaring one floor per edition unbuildable everywhere.
    fn keeps(self, declared: Option<Edition>) -> bool {
        match self {
            Self::Pinned(edition) => declared.is_none_or(|declared| declared == edition),
            Self::Neutral => declared.is_none(),
        }
    }
}

/// Walk the module's headers and its instantiated parts once, keeping the
/// floors `scope` accepts.
///
/// Source order, which is headers before items: a caller reporting "the
/// first floor that refuses the target" reports the one an author reading
/// top to bottom would reach first.
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
        // A part declared twice is `E_DUPLICATE_ITEM`, refused before any floor is weighed, so both
        // copies are walked rather than replaying first-binding-wins.
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

/// The parts of a module a build actually instantiates.
///
/// Both sets hold *declared* names, so a `use=` or `theme=` naming nothing
/// is absent rather than present-and-unmatched. Those are already
/// `E_UNRESOLVED_PLACE_REF` and `E_UNRESOLVED_THEME_REF`.
struct InstantiatedParts<'a> {
    /// Defs some `place use=NAME` in a `site` names.
    defs: HashSet<&'a str>,
    /// Themes some scope binds — see [`Self::of`] for what binds one.
    themes: HashSet<&'a str>,
}

impl<'a> InstantiatedParts<'a> {
    /// Read the placements out of `module`'s `site` bodies.
    ///
    /// **A theme's floor applies when the theme is bound, whether or not a member reads a slot from
    /// it.** Charging it only once a rule fires would make the floor depend on which selectors
    /// matched and which variant the pin picked, and would err unsafely: an over-applied floor is
    /// reported against its line and is one edit away, while an under-applied one certifies a build
    /// the file rules out, which `E_VERSION_CAP` exists to prevent.
    ///
    /// Two things bind a theme, both a scope a build lowers:
    ///
    /// - a `place ... theme=NAME`, which also instantiates the `def` it places (`theme=` is
    ///   required on a `place`);
    /// - the module-level auto-pick, read here only for a `struct`, the one scope a build lowers
    ///   without a placement.
    ///
    /// The auto-pick also binds `def` scopes, and this does not follow it there: an unplaced `def`
    /// builds no voxels and is not charged its own floor, so it must not take on a theme's. A
    /// placed `def` reaches the theme through its placement's `theme=`.
    ///
    /// Under [`FloorScope::Neutral`] both editions must bind the same theme; see
    /// [`unscoped_version_floors`].
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
                // A `place` outside a `site` body is `E_MISPLACED_MEMBER`, an Error, so the file is
                // refused before any floor is weighed.
                continue;
            };
            parts.walk(body, &declared_defs, &declared_themes, scope);
        }
        parts
    }

    /// Collect the `place` lines of one body, and of the bodies under it.
    ///
    /// Over-collects on purpose: a `place` nested under a member is `E_UNSUPPORTED_NESTING` and the
    /// resolver reads a site's placements flat, but where a `place` may stand is
    /// `check::member_scope`'s question, and over-collecting on a refused file costs nothing.
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

/// The theme `bind` selects under `scope`, or `None`.
///
/// A pinned scope is the build's own answer. A neutral one asks both editions and keeps the answer
/// only when they agree, since a theme one edition binds and the other does not is a per-edition
/// fact.
///
/// `bind` is the rule the caller asks about ([`pick_variant`] for the auto-pick,
/// [`bound_theme_name`] for a written `theme=`), so both share one neutral test.
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

/// The label value of `key` among `args`, when it has one.
///
/// `None` also for a value that is not label-shaped (`use=3`): that is `E_TYPE_MISMATCH_LABEL` from
/// `check::type_mismatch`, and reading it as a name would invent a part the author did not
/// reference.
fn label_arg<'a>(args: &'a [Arg], key: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.key == key)
        .and_then(|arg| arg.value.as_label_str())
}

/// A version floor a module declares, with the directive it came from.
///
/// The span lets a caller outside this crate (the CLI enforcing `--target`) point at the line that
/// set the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionFloor {
    /// The version label as written, already known to be version-shaped. Whether the target edition
    /// ships it is [`super::VersionOrder`]'s answer.
    pub version: String,
    /// The edition the directive scoped the floor to, when it named one.
    pub edition: Option<Edition>,
    /// Which part of the module declared it.
    pub origin: FloorOrigin,
    /// Byte range of the `@requires` directive or `requires` line that
    /// declared it.
    pub span: Span,
}

impl VersionFloor {
    /// The floor as the author wrote it, scope and all.
    ///
    /// Every sentence that quotes a floor reads it from here (`cairn compile`'s two refusals,
    /// `cairn info`'s notes, the `@intended_targets` findings): a floor quoted without its scope
    /// reads as a different floor, and the scope is usually the half the repair touches.
    #[must_use]
    pub fn rendered(&self) -> String {
        match self.edition {
            Some(edition) => format!("{edition} version>={}", self.version),
            None => format!("version>={}", self.version),
        }
    }

    /// Who declared the floor, as the subject of a sentence about it.
    ///
    /// "this file" for a header, and the part for a member-level floor: "this file requires ..."
    /// for an inherited floor is the reading `spec/versioning-editions` "Fail-loud and
    /// minimum-version inference" corrects.
    #[must_use]
    pub fn declarer(&self) -> String {
        self.origin.part().map_or_else(
            || "this file".to_owned(),
            |(keyword, name)| format!("`{keyword} {name}`"),
        )
    }
}

/// The candidates in `considered` that satisfy every floor at once.
///
/// Every floor, not only the one being reported: a candidate that clears it and trips the next is a
/// second refusal, and the closed set `spec/lint` "Diagnostic codes" asks every message to carry
/// has to be a set that builds.
///
/// A floor this edition's table cannot place refuses nothing here. It is its own refusal,
/// `E_REQUIRES_UNORDERABLE`, against the `requires` line, and folding it in would answer "no target
/// works" for a file whose repair is on that line.
///
/// Shared by `cairn compile`'s `E_VERSION_CAP` and the `@intended_targets` findings, so the offer
/// is the same from either.
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
///
/// A floor five `def`s deep in a library is not actionable as a bare version, and the span says
/// which line but not whose. Holds the part's name rather than a sentence, so each renderer phrases
/// it: `E_VERSION_CAP` and a `cairn info` note differ.
///
/// Exhaustive, unlike the rows of [`VersionAxes`]: each variant is a route with its own repair, and
/// a renderer meeting a new one through a wildcard would print the wrong prose. Adding a variant
/// should break every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorOrigin {
    /// An `@requires` header: a floor on the file itself.
    Module,
    /// A `requires` line in `def NAME`, inherited by every `place use=NAME`.
    Def(String),
    /// A `requires` line in `theme NAME`, inherited by every scope that
    /// binds the theme.
    Theme(String),
}

impl FloorOrigin {
    /// The part as `(keyword, name)`, or `None` for a module-level floor.
    ///
    /// A pair rather than `"def cottage"`, because the keyword is [`ItemKind`]'s to spell. `None`
    /// rather than `"the module"`: the span already points at the file's line.
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
