//! Computation of the three version axes reported by `cairn info`.
//!
//! Spec source: `spec/versioning-editions.md` §10.5. The three axes are:
//!
//! 1. **registry-compatible range** `[Vmin, Vmax]` — the intersection of
//!    every `since/until` over the tokens/states the file actually uses.
//! 2. **edition portability** — per edition, how many members compile
//!    portably, are degraded, or are unsupported. Data-source: the caller
//!    (typically `cairn-lang-cli`, which runs a per-edition dry-run through
//!    `cairn-lang-formats::portability` on the lowered block-array IR).
//! 3. **semantic-sensitive members** — registry-valid IDs whose meaning or
//!    behavior shifts at a known boundary version (the catalog half of
//!    `spec/versioning-editions.md` §10.3).
//!
//! Beside them sits **buildable targets**, which is not one of the three
//! but the answer axis (2) deliberately does not give. Portability asks of
//! the *edition* — a block one part of the range spells differently is not
//! missing from the edition — and two entries can be declared by disjoint
//! sets of versions while each answers yes. This row asks of each version
//! in turn, so a source no supported version can build cannot report
//! clean.
//!
//! Axis (1) is computed from the module's declared version floors — its
//! `@requires` headers and the member-level `requires` lines of every part
//! the build instantiates (`spec/versioning-editions.md` §10.4). Axis (2) is a pure
//! forwarding of the caller's per-edition figures — the `core` crate does
//! not itself depend on `cairn-lang-formats`, so the concrete
//! `translate_states` lookup happens one crate up and is handed in as a
//! `Vec<EditionPortability>`. Axis (3) remains structurally present but
//! empty until the semantic-sensitivity catalog lands.

use std::collections::HashSet;

use serde::Serialize;

use crate::ast::{Arg, Header, Item, ItemKind, Module, RawRequirement, Statement};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::IntentModule;

use super::requires_parse::{compare_versions, parse_min_version};
use super::resolver::Resolution;
use super::theme_variant::{bound_theme_name, pick_variant, single_logical_theme};

/// The three-axis answer to "which version is this `.crn` for?".
///
/// `#[non_exhaustive]` because this type is only ever *read* outside the
/// crate: [`compute_axes`] builds it and the CLI renders it, so a fourth
/// axis should not be a breaking change. The types the caller *builds* —
/// [`EditionReport`] — stay exhaustive for the opposite reason: a field
/// added there is a value the caller has to supply, and the compiler
/// saying so is the point.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VersionAxes {
    /// Axis 1: registry-compatible version range.
    pub registry_compat: RegistryRange,
    /// Axis 2: per-edition portability counts, in the order requested by
    /// the caller (CLI honours `--editions`).
    pub edition_portability: Vec<EditionPortability>,
    /// Axis 3: members whose meaning shifts at a known boundary version.
    /// Empty until the semantic-sensitivity catalog lands.
    pub semantic_sensitive: Vec<SemanticSensitiveFinding>,
    /// Which supported versions of each requested edition can build the
    /// source. One entry per entry of [`Self::edition_portability`], in
    /// the same order and for the same edition — both are fanned out of
    /// one [`EditionReport`] per edition, so the two cannot disagree.
    pub buildable_targets: Vec<BuildableTargets>,
}

/// Everything one requested edition contributes to [`VersionAxes`].
///
/// The wire shape keeps the portability counts and the buildable versions
/// in separate lists, because that is what `cairn info --format json` has
/// always emitted. Taking them from the caller that way would put three
/// invariants in the API with nothing to enforce them — equal length,
/// matching order, one row per edition — so the caller builds one of
/// these per edition and [`compute_axes`] does the fanning out.
#[derive(Debug, Clone, PartialEq)]
pub struct EditionReport {
    /// Target edition this report describes.
    pub edition: Edition,
    /// Palette entries that compile straight through.
    pub portable: u32,
    /// Palette entries that compile but lose detail.
    pub degraded: u32,
    /// Palette entries with no representable form on this edition.
    pub unsupported: u32,
    /// The entries [`Self::unsupported`] counts, named.
    pub unsupported_entries: Vec<UnsupportedEntry>,
    /// Versions from [`Self::considered`] a build would accept.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares.
    pub considered: Vec<String>,
}

/// Which supported versions of one edition can build the source.
///
/// Separate from [`EditionPortability`] because the two ask different
/// questions and only one of them has a per-version answer: portability
/// counts palette entries against the edition, and an id that only part of
/// the range spells that way is not missing from the edition. Every entry
/// of a source can pass that test while no single version passes all of
/// them at once, which is the shape this row exists to report.
///
/// Not a `[min, max]` range: the answer need not be contiguous — two ids
/// whose version sets interleave leave a gap in the middle — and a range
/// would claim the gap.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildableTargets {
    /// Target edition this row describes.
    pub edition: Edition,
    /// The versions in [`Self::considered`] a build would accept, in the
    /// same order.
    ///
    /// "Would accept" is the gates `cairn compile --target` applies before
    /// it writes anything: the pinned lowering raises no error, the
    /// source's `@requires` floor is at or below the version, and every
    /// scope the source declares lowered. The gates it applies afterwards
    /// are about the filesystem rather than about the source, and are not
    /// asked here.
    pub buildable: Vec<String>,
    /// Every version the edition's registry pack declares, in ascending
    /// release order.
    ///
    /// Carried so an empty `buildable` can be read: "no version builds
    /// this" and "the pack declares no versions" are different facts and
    /// the first one alone cannot tell them apart.
    pub considered: Vec<String>,
}

/// Registry-compatible Minecraft version range.
///
/// `min` is derived from the **unscoped** `@requires version>=X` headers:
/// the strictest of them, or `"0.0"` when the file declares none that feed
/// this row. `max` is the literal string `"latest"` until the registry
/// pack provides a real upper bound.
///
/// `"0.0"` therefore has two causes — no `@requires` line at all, and only
/// floors scoped to an edition — and the row cannot tell them apart,
/// because it is one row for a file that may be reported against both
/// editions at once. `cairn info` prints a note naming the scoped floors
/// it left out, so the reader is not left to infer it from a `0.0` beside
/// a `buildable targets` row that refuses versions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryRange {
    /// Lower bound (inclusive).
    pub min: String,
    /// Upper bound (inclusive). Always `"latest"` until the registry pack
    /// catalog supplies an explicit upper edition for the file.
    pub max: String,
}

/// Per-edition portability counts.
///
/// The `edition` field is the [`Edition`] enum (not a raw `String`) so
/// downstream consumers cannot silently receive an unrecognised edition
/// label — the CLI's `--editions` parser already rejects unknown strings,
/// and this type keeps that invariant load-bearing in the type system.
/// [`Edition`]'s [`Serialize`] impl emits the canonical lowercase name so
/// the JSON wire shape (`"edition":"java"` / `"bedrock"`) is unchanged.
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
    /// The entries [`Self::unsupported`] counts, named and with the reason
    /// each of them has no form here.
    ///
    /// One element per unit of the count, in palette order. The count is
    /// what the row has always carried and is kept as it is; this is the
    /// answer to the question a bare integer cannot be read as, since the
    /// three ways an entry can be unsupported have three different repairs
    /// and only one of them is the author's.
    pub unsupported_entries: Vec<UnsupportedEntry>,
}

/// One palette entry an edition has no form for, and why.
///
/// Built by `cairn-lang-formats::portability`, which is where the question
/// is decided — the Bedrock state translator and the registry pack's id
/// tables both live there. The type is declared here, beside the row it is
/// a field of, so there is one shape rather than two identical ones with a
/// mapping between them to fall out of step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsupportedEntry {
    /// The palette entry's block id, verbatim as the lowering interned it.
    pub id: String,
    /// Which of the three ways this entry has no form on the edition.
    #[serde(flatten)]
    pub reason: UnsupportedReason,
}

/// Why one palette entry counts as unsupported.
///
/// Four variants for four different repairs — change the material, wait
/// for the backend, fix the pack, edit the blockstate — which is the whole
/// reason the figure they fold into cannot be acted on.
///
/// Every variant carries the pieces of its answer rather than a rendered
/// sentence. The prose belongs to whatever is doing the rendering, and a
/// consumer that reads the tag should not then have to parse English out
/// of a field beside it.
///
/// Serialized as an internally tagged union, so a consumer reads
/// `"reason": "absent_from_edition"` beside the fields that reason carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// No supported version of the edition declares the block. The states
    /// question is not asked — there are none to translate on a block that
    /// does not exist.
    AbsentFromEdition {
        /// The nearest id the edition does declare, when one is close
        /// enough to be a plausible typo. `None` is the ordinary answer for
        /// a block that simply belongs to the other edition.
        suggestion: Option<String>,
        /// The ids this edition does declare for the same block, from the
        /// registry pack's alias table. This is the answer for the case
        /// `suggestion` structurally cannot reach: an edition that has the
        /// block under another name is not one edit away from it, and
        /// `oak_sign` → `standing_sign` is seven.
        ///
        /// Empty when the pack names none — which is also the whole of
        /// what a pack shipping no alias table can say — and then the
        /// entry really is a block this edition does not have.
        aliases: Vec<String>,
    },
    /// The edition has the block and this compiler's backend has no
    /// mapping for the states the intent put on it. A statement about the
    /// backend, not about the edition: the block may well accept those
    /// states in the game.
    StatesUnmapped {
        /// The entry's `key=value` pairs, comma-joined, so the row names
        /// the states rather than only the block carrying them.
        states: String,
        /// The families the backend does map, as the message lists them.
        mapped: String,
    },
    /// A state value outside the Java domain reached the translator. The
    /// registry pack is expected to reject these, so an author reading
    /// this has nothing to edit — though no pack schema can express a
    /// value domain today, which is why one got through.
    StateValueUnexpected {
        /// The property key carrying the value.
        key: String,
        /// The offending value verbatim.
        value: String,
        /// Comma-joined valid values for `key`.
        valid: String,
    },
    /// A state key the backend does not read at all. Refused rather than
    /// ignored so a key handled later cannot retroactively change what
    /// already-shipped output meant — and unlike the three above, the
    /// repair is the author's: remove it.
    StateKeyUnread {
        /// The unread property key.
        key: String,
        /// Comma-joined keys the backend does read.
        handled: String,
    },
}

/// One semantic-sensitivity finding.
///
/// Always empty for now; the type exists so the JSON shape is stable from
/// the first `cairn info` release.
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
/// `edition_portability` is threaded in from the caller because the
/// per-edition classification lives in `cairn-lang-formats` (it consults
/// [`crate::block_array::BlockArrayIr`] palettes against the Bedrock state
/// translator), and `core` does not depend on `formats`. The passed list is
/// forwarded verbatim — the CLI's `run_info` runs one dry-run lowering per
/// requested edition, feeds the palette through
/// `cairn-lang-formats::portability`, and hands the built list here so the
/// JSON / text shape does not diverge from what the compile backend would
/// see.
///
/// The buildable versions ride in the same [`EditionReport`] and for the
/// same reason: deciding them means lowering once per supported version
/// against that version's id table, which is the registry pack's data and
/// so the caller's to hold.
///
/// `_resolution` is accepted so future work that needs the resolved binding
/// (semantic-sensitivity checks per concrete token) can land without an API
/// change. Currently only `module.headers` and `editions` are consumed.
#[must_use]
pub fn compute_axes(
    module: &Module,
    _ir: &IntentModule,
    _resolution: &Resolution,
    editions: Vec<EditionReport>,
) -> VersionAxes {
    let mut edition_portability = Vec::with_capacity(editions.len());
    let mut buildable_targets = Vec::with_capacity(editions.len());
    for report in editions {
        edition_portability.push(EditionPortability {
            edition: report.edition,
            portable: report.portable,
            degraded: report.degraded,
            unsupported: report.unsupported,
            unsupported_entries: report.unsupported_entries,
        });
        buildable_targets.push(BuildableTargets {
            edition: report.edition,
            buildable: report.buildable,
            considered: report.considered,
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
    }
}

/// The `registry compatibility` row's lower edge, as text.
///
/// Only the floors that carry no edition feed it. The row is
/// edition-neutral — one range for a file `cairn info` may be reporting
/// against both editions at once — and a floor written in one edition's
/// numbering has no meaning in the other's, so it belongs to the
/// per-edition `buildable targets` row instead.
///
/// The strictest of several is picked with [`compare_versions`], which is
/// the label comparison rather than the `DataVersion` key. There is no key
/// available: the row is edition-neutral and the tables are per edition,
/// so the only thing left to order two unscoped floors by is their text.
/// That is sound here and nowhere else, because this row *renders* a
/// version the author wrote and decides no build — every gate that does
/// (`E_VERSION_CAP`, `buildable targets`) weighs each floor against the
/// target edition's table separately and never compares two floors at all.
///
/// The comparison's arbitrary half shows here and only here: a label it
/// cannot read as a number sorts above every one it can, so a file
/// declaring both `version>=1.21.4` and `version>=24w14a` reports the
/// snapshot as its lower edge. Fixed rather than meaningful, which is
/// what a total order promises; the row is a rendering of the file, and
/// `buildable targets` is the answer that is weighed.
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

/// Every version floor `module` declares that a build of `edition` is held
/// to, in source order.
///
/// Floors compose by taking the intersection: each line adds a constraint
/// rather than displacing the one before (spec syntax §5.3), and
/// `[a, ∞) ∩ [b, ∞)` is `[max(a, b), ∞)`. They are returned as a list
/// rather than folded to that maximum here, because the fold needs an
/// ordering and the ordering is `DataVersion` — a per-edition table this
/// crate does not hold. A caller that has one weighs each floor against
/// the target and reports the first that refuses it, which reaches the
/// same answer without ever comparing two floors to each other.
///
/// **The list is the composite's, not the file header's.**
/// `spec/versioning-editions.md` §10.4 gives a `def` and a `theme` a floor
/// of their own, and says the minimum version of a composite is the max of
/// its parts. So the walk is the module's `@requires` headers plus every
/// part the build instantiates: a `def` some `place use=` names, and a
/// `theme` some scope binds. That is what lets a library of templates carry
/// its own requirements instead of every consumer restating them — a
/// module-level `@requires` cannot, because it applies to the whole file
/// rather than to the template.
///
/// A part nothing instantiates contributes nothing. A `def` no `place`
/// names builds no voxels (and earns `W_UNUSED_DEF`), and holding a build
/// to a floor for geometry it does not contain would refuse targets over a
/// template the author left in the file.
///
/// `edition` says which build is asking: the floors scoped to it and the
/// unscoped ones, since an unscoped floor is a floor on whatever is being
/// built. It also decides which per-edition theme variant a `theme=`
/// reference binds, through the same [`super::theme_variant`] rule the
/// resolver uses. The edition-neutral question is a different function
/// ([`unscoped_version_floors`]) rather than a `None` here, because
/// `Option<Edition>` would then carry two opposite senses in one API — a
/// floor's own `None` means "every edition" and the argument's would mean
/// "no edition".
///
/// Requirements the grammar refuses declare nothing and are skipped here.
/// They are reported by `check::requires` at `Error` severity, which is
/// what stops a compile before it can be held to half of an expression that
/// has already been called a mistake — see
/// `crates/cairn-lang-core/tests/silent_skip_arms.rs` for the caller that
/// bypasses `check` and what it gets instead.
#[must_use]
pub fn declared_version_floors(module: &Module, edition: Edition) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Pinned(edition))
}

/// The floors that constrain the file without naming an edition.
///
/// The edition-neutral question, which `cairn info`'s `registry
/// compatibility` row asks: one row for a file it may be reporting against
/// both editions at once, so only a floor that means something in both can
/// feed it. A floor written in Java's numbering says nothing about the
/// file's Bedrock range, and reading it as if it did is the defect the
/// scope exists to remove.
///
/// A floor a *part* declares is held to the same test, and for the part it
/// is inherited through rather than for the words on the line. A `theme`
/// contributes here only when both editions bind the same one — see
/// [`InstantiatedParts::of`]. Picking a variant by the unpinned order
/// instead put a floor no build is held to into this row, in both
/// directions: `theme shop` (with a floor) beside `theme shop_bedrock`
/// (without) reported a lower edge a Bedrock build does not have, and the
/// reverse pairing reported `0.0` for a Java build that is held to one —
/// which `--format json` carries as `registry_compat.min` with nothing on
/// the row to correct it.
///
/// A `def` needs no such test. It is instantiated by a `place use=NAME`,
/// which names one def and not a family of per-edition variants, so the
/// same def is inherited whichever edition is being built.
#[must_use]
pub fn unscoped_version_floors(module: &Module) -> Vec<VersionFloor> {
    collect_floors(module, FloorScope::Neutral)
}

/// Which build is asking for a module's floors.
///
/// One value rather than an `Option<Edition>` beside a predicate. The two
/// used to be separate arguments, which made
/// `collect_floors(module, Some(Java), |declared| declared.is_none())`
/// type-check — "bind Java's theme variants, then keep only the floors that
/// name no edition", a question nothing asks. Both halves are decided by
/// the same fact, so they are read off one value.
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
        // A part declared twice is `E_DUPLICATE_ITEM` and the file is
        // refused before any of this is weighed, so both copies are walked
        // rather than replaying the resolver's first-binding-wins rule for
        // a case that never reaches a build.
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
    /// **A theme's floor applies when the theme is bound, whether or not a
    /// member reads a slot from it.** The alternative — charge the floor
    /// only once a rule fires — was rejected on two counts. It makes the
    /// floor depend on which selectors matched and which variant the pin
    /// picked, so one source could require 1.21 on Java and nothing on
    /// Bedrock for a reason that is not about editions; and it errs in the
    /// unsafe direction, since an over-applied floor is reported against
    /// the line that set it and is one edit away, while an under-applied
    /// one certifies a build the file itself rules out, which is the defect
    /// `E_VERSION_CAP` exists to remove. Binding a theme is the act of
    /// taking on what it declares.
    ///
    /// Two things bind one, and both of them are a scope a build lowers:
    ///
    /// - a `place ... theme=NAME` reference, which is also what instantiates
    ///   the `def` it places (`theme=` is required on a `place`, so a
    ///   placement always names the theme its body resolves against);
    /// - the module-level auto-pick, read here only for a `struct`, which
    ///   is the one scope a build lowers without a placement.
    ///
    /// The auto-pick binds to `def` scopes too, and this deliberately does
    /// not follow it there. A `def` no `place` names builds no voxels, so
    /// charging a theme's floor because such a `def` exists would read the
    /// same `def` as instantiated enough to take on a theme's floor and not
    /// instantiated enough to be charged its own — the reading "a part
    /// nothing instantiates contributes nothing" exists to rule out. A
    /// `def` that *is* placed reaches the theme through its placement's own
    /// `theme=` instead, so nothing is lost by not following it.
    ///
    /// Under [`FloorScope::Neutral`] a theme has one more test to pass:
    /// both editions must bind the same one. A row that is about neither
    /// edition cannot read a floor only one of them inherits, and the
    /// unpinned variant order corresponds to no value of `--editions` — see
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
                // A `place` outside a `site` body is `E_MISPLACED_MEMBER`,
                // an Error, so the file is refused before any floor is
                // weighed and the placement it writes instantiates nothing.
                continue;
            };
            parts.walk(body, &declared_defs, &declared_themes, scope);
        }
        parts
    }

    /// Collect the `place` lines of one body, and of the bodies under it.
    ///
    /// The recursion over-collects rather than under-collects: a `place`
    /// nested under a member is `E_UNSUPPORTED_NESTING`, and the resolver
    /// reads a site's placements flat, so nothing a nested `place` names is
    /// ever instantiated. It is walked anyway because the alternative is a
    /// walk that decides where a `place` may stand, which is
    /// `check::member_scope`'s question and not this one's; over-collecting
    /// on a file that is already refused costs nothing.
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
/// A pinned scope is the build's own answer. A neutral one asks both
/// editions and keeps the answer only when they agree: a theme one edition
/// binds and the other does not is a per-edition fact, and a row about
/// neither edition has no business reading a floor from it.
///
/// `bind` is the selection rule the caller is asking about —
/// [`pick_variant`] for the module-level auto-pick, [`bound_theme_name`]
/// for a written `theme=` reference — so the neutral test is the same test
/// in both places rather than two spellings of it.
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
/// `None` covers both an absent key and one whose value is not label-shaped
/// (`use=3`); the second is `E_TYPE_MISMATCH_LABEL`, reported by
/// `check::type_mismatch`, and reading it as a name here would invent a
/// part the author did not reference.
fn label_arg<'a>(args: &'a [Arg], key: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.key == key)
        .and_then(|arg| arg.value.as_label_str())
}

/// A version floor a module declares, with the directive it came from.
///
/// The span is what lets a caller outside this crate — the CLI enforcing
/// `--target` against the floor — point at the line that set it rather than
/// at the file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionFloor {
    /// The version label as written, already known to be shaped like a
    /// version. Whether it names one the target edition ships is the
    /// [`super::VersionOrder`]'s answer, not this type's.
    pub version: String,
    /// The edition the directive scoped the floor to, when it named one.
    pub edition: Option<Edition>,
    /// Which part of the module declared it.
    pub origin: FloorOrigin,
    /// Byte range of the `@requires` directive or `requires` line that
    /// declared it.
    pub span: Span,
}

/// Which part of a module a [`VersionFloor`] came from.
///
/// Carried because the whole value of a composite floor is knowing which
/// piece of the build wants it: a target refused by a floor five `def`s
/// deep in a library is not actionable as a bare version number, and the
/// span alone answers "which line" without answering "whose".
///
/// Holds the part's name rather than a rendered sentence, so the prose
/// belongs to whatever is rendering — the CLI writes one phrasing under
/// `E_VERSION_CAP` and another under a `cairn info` note.
///
/// Exhaustive, unlike the rows of [`VersionAxes`] beside it. Each variant
/// is a different route by which a build inherited the floor, and each
/// route has its own repair at its own end; a renderer that met a fourth
/// one through a wildcard would print the three-route prose for it. Adding
/// a variant should break every caller, which is what says the new route
/// has been answered rather than absorbed.
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
    /// A pair rather than a rendered `"def cottage"`, because this type
    /// holds the part rather than prose about it, and because the keyword
    /// is [`ItemKind`]'s to spell — the surface word for a kind is written
    /// down once, and a renderer that hand-wrote it here would be a second
    /// copy to fall out of step.
    ///
    /// `None` rather than `"the module"`: a caller printing a note about
    /// where a floor came from has nothing to add for the header form —
    /// the span already points at the line, and the line is the file's.
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
mod tests {
    use super::*;
    use crate::resolve::resolve;
    use crate::{lower, parse};

    fn module_with(source: &str) -> (Module, IntentModule, Resolution) {
        let m = parse(source).expect("parse");
        let i = lower(&m);
        let r = resolve(&i, None);
        (m, i, r)
    }

    /// Fabricate a per-edition report matching the shape the CLI builds
    /// from `cairn-lang-formats::portability` and its own version loop.
    /// Used to keep the axis-1 / axis-3 tests independent of the axis-2
    /// data source; the version lists are left empty because those tests
    /// are not about them.
    fn synthetic_portability(entries: &[(Edition, u32, u32, u32)]) -> Vec<EditionReport> {
        entries
            .iter()
            .map(
                |&(edition, portable, degraded, unsupported)| EditionReport {
                    edition,
                    portable,
                    degraded,
                    unsupported,
                    unsupported_entries: Vec::new(),
                    buildable: Vec::new(),
                    considered: Vec::new(),
                },
            )
            .collect()
    }

    #[test]
    fn registry_compat_min_from_requires_header() {
        let src = "@requires version>=1.20\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(
            &m,
            &i,
            &r,
            synthetic_portability(&[(Edition::Java, 0, 0, 0)]),
        );
        assert_eq!(axes.registry_compat.min, "1.20");
        assert_eq!(axes.registry_compat.max, "latest");
    }

    #[test]
    fn registry_compat_takes_max_when_multiple_requires_present() {
        let src = "@requires version>=1.20\n@requires version>=1.21\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(
            &m,
            &i,
            &r,
            synthetic_portability(&[(Edition::Java, 0, 0, 0)]),
        );
        assert_eq!(axes.registry_compat.min, "1.21");
    }

    #[test]
    fn registry_compat_defaults_when_requires_absent() {
        let src = "struct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(
            &m,
            &i,
            &r,
            synthetic_portability(&[(Edition::Java, 0, 0, 0)]),
        );
        assert_eq!(axes.registry_compat.min, "0.0");
    }

    #[test]
    fn edition_portability_is_forwarded_verbatim_from_caller() {
        // The core-crate `compute_axes` no longer synthesises portability
        // figures — the CLI hands in the per-edition counts (computed by
        // `cairn-lang-formats::portability`) and this function forwards
        // them into the output. Pin the forwarding so a future refactor
        // can't quietly re-introduce a zero-fill or reordering.
        let src = "struct s size=4x4\n  walls height=3\n  floor\n  roof kind=gable\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(
            &m,
            &i,
            &r,
            vec![
                EditionReport {
                    edition: Edition::Java,
                    portable: 3,
                    degraded: 0,
                    unsupported: 0,
                    unsupported_entries: Vec::new(),
                    buildable: vec!["1.20.4".to_owned()],
                    considered: vec!["1.20.4".to_owned()],
                },
                EditionReport {
                    edition: Edition::Bedrock,
                    portable: 1,
                    degraded: 1,
                    unsupported: 4,
                    unsupported_entries: one_entry_per_reason(),
                    buildable: vec!["1.21.0".to_owned()],
                    considered: vec!["1.21.0".to_owned(), "1.21.40".to_owned()],
                },
            ],
        );
        assert_eq!(axes.edition_portability.len(), 2);
        assert_eq!(axes.edition_portability[0].edition, Edition::Java);
        assert_eq!(axes.edition_portability[0].portable, 3);
        assert_eq!(axes.edition_portability[0].degraded, 0);
        assert_eq!(axes.edition_portability[0].unsupported, 0);
        assert_eq!(axes.edition_portability[1].edition, Edition::Bedrock);
        assert_eq!(axes.edition_portability[1].portable, 1);
        assert_eq!(axes.edition_portability[1].degraded, 1);
        assert_eq!(axes.edition_portability[1].unsupported, 4);
        // JSON wire shape must remain unchanged so downstream consumers
        // treating `edition_portability[].edition` as a lowercase string
        // continue to work under the enum-typed field.
        let json = serde_json::to_string(&axes).unwrap();
        assert!(json.contains(r#""edition":"java""#), "got: {json}");
        assert!(json.contains(r#""edition":"bedrock""#), "got: {json}");
        // The buildable row is fanned out of the same input, so it has
        // one entry per portability row, in the same order, for the same
        // edition — the invariant two independent `Vec` parameters could
        // not carry. It also keeps the versions it weighed: an empty
        // `buildable` says nothing on its own about how many there were.
        assert_eq!(axes.buildable_targets.len(), axes.edition_portability.len());
        let editions: Vec<Edition> = axes
            .buildable_targets
            .iter()
            .map(|row| row.edition)
            .collect();
        assert_eq!(
            editions,
            axes.edition_portability
                .iter()
                .map(|row| row.edition)
                .collect::<Vec<_>>(),
        );
        assert_eq!(axes.buildable_targets[1].buildable, ["1.21.0"]);
        assert_eq!(axes.buildable_targets[1].considered, ["1.21.0", "1.21.40"]);
        assert!(json.contains(r#""buildable":["1.21.0"]"#), "got: {json}");
        assert!(
            json.contains(r#""considered":["1.21.0","1.21.40"]"#),
            "got: {json}",
        );
        // The named entries ride the same forwarding, and an edition with
        // nothing unsupported carries an empty list rather than a missing
        // one.
        assert_eq!(
            axes.edition_portability[1].unsupported_entries,
            one_entry_per_reason(),
        );
        assert_eq!(axes.edition_portability[0].unsupported_entries, Vec::new());
    }

    /// One [`UnsupportedEntry`] per [`UnsupportedReason`] variant, so a
    /// test asserting something about all of them cannot quietly stop
    /// covering one.
    fn one_entry_per_reason() -> Vec<UnsupportedEntry> {
        vec![
            UnsupportedEntry {
                id: "minecraft:oak_sign".to_owned(),
                reason: UnsupportedReason::AbsentFromEdition {
                    suggestion: Some("minecraft:oak_log".to_owned()),
                    aliases: vec!["minecraft:standing_sign".to_owned()],
                },
            },
            UnsupportedEntry {
                id: "minecraft:oak_door".to_owned(),
                reason: UnsupportedReason::StatesUnmapped {
                    states: "facing=north".to_owned(),
                    mapped: "the stair family".to_owned(),
                },
            },
            UnsupportedEntry {
                id: "minecraft:oak_stairs".to_owned(),
                reason: UnsupportedReason::StateValueUnexpected {
                    key: "facing".to_owned(),
                    value: "up".to_owned(),
                    valid: "east, west, south, north".to_owned(),
                },
            },
            UnsupportedEntry {
                id: "minecraft:oak_stairs".to_owned(),
                reason: UnsupportedReason::StateKeyUnread {
                    key: "waterlogged".to_owned(),
                    handled: "facing, half, shape".to_owned(),
                },
            },
        ]
    }

    /// Every reason's JSON, pinned here because three of the four are
    /// unreachable from a `.crn` and so never pass through a CLI test.
    ///
    /// `reason` is the internal tag and every other key is that variant's
    /// own, flattened beside it: a consumer reads the tag and then reads
    /// fields, which is the whole reason none of them carries a rendered
    /// sentence.
    #[test]
    fn every_unsupported_reason_carries_its_own_wire_shape() {
        let entries = serde_json::to_value(one_entry_per_reason()).expect("the entries serialize");
        assert_eq!(
            entries,
            serde_json::json!([
                {
                    "id": "minecraft:oak_sign",
                    "reason": "absent_from_edition",
                    "suggestion": "minecraft:oak_log",
                    "aliases": ["minecraft:standing_sign"],
                },
                {
                    "id": "minecraft:oak_door",
                    "reason": "states_unmapped",
                    "states": "facing=north",
                    "mapped": "the stair family",
                },
                {
                    "id": "minecraft:oak_stairs",
                    "reason": "state_value_unexpected",
                    "key": "facing",
                    "value": "up",
                    "valid": "east, west, south, north",
                },
                {
                    "id": "minecraft:oak_stairs",
                    "reason": "state_key_unread",
                    "key": "waterlogged",
                    "handled": "facing, half, shape",
                },
            ]),
        );
        // `suggestion` is present and null rather than absent when there
        // is no candidate, and `aliases` is present and empty on the same
        // terms, so a consumer reads one shape per reason whatever the
        // answer was.
        let absent = serde_json::to_value(UnsupportedEntry {
            id: "minecraft:nothing_like_this".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
        })
        .expect("the entry serializes");
        assert_eq!(
            absent,
            serde_json::json!({
                "id": "minecraft:nothing_like_this",
                "reason": "absent_from_edition",
                "suggestion": null,
                "aliases": [],
            }),
        );
    }

    #[test]
    fn semantic_sensitive_is_empty_without_catalog() {
        let src = "struct s size=4x4\n  walls height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(
            &m,
            &i,
            &r,
            synthetic_portability(&[(Edition::Java, 0, 0, 0)]),
        );
        assert!(axes.semantic_sensitive.is_empty());
    }
}
