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
//! Axis (1) is computed from `@requires` headers. Axis (2) is a pure
//! forwarding of the caller's per-edition figures — the `core` crate does
//! not itself depend on `cairn-lang-formats`, so the concrete
//! `translate_states` lookup happens one crate up and is handed in as a
//! `Vec<EditionPortability>`. Axis (3) remains structurally present but
//! empty until the semantic-sensitivity catalog lands.

use serde::Serialize;

use crate::ast::{Header, Module};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::IntentModule;

use super::requires_parse::{compare_versions, parse_min_version};
use super::resolver::Resolution;

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
/// `min` is derived from `@requires version>=X` (the max across all such
/// headers; `"0.0"` when no `@requires` line is present). `max` is the
/// literal string `"latest"` until the registry pack provides a real upper
/// bound.
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

fn derive_min_version(module: &Module) -> String {
    declared_version_floor(module).map_or_else(|| "0.0".to_owned(), |floor| floor.version)
}

/// The strictest version floor `module` declares, and the directive that
/// declared it.
///
/// `@requires` floors compose by taking the maximum: each line adds a
/// constraint rather than displacing the one before (spec syntax §5.3), and
/// `[a, ∞) ∩ [b, ∞)` is `[max(a, b), ∞)`. Two lines naming the *same*
/// version — `version>=1.21` and `version>=1.21.0` — leave the maximum
/// undecided, and the first one wins, so a diagnostic points at the line
/// that has been there longest rather than moving when an equivalent one is
/// appended below it.
///
/// Requirements the grammar refuses declare nothing and are skipped here.
/// They are reported by `check::requires` at `Error` severity, which is
/// what stops a compile before it can be held to half of an expression that
/// has already been called a mistake — see
/// `crates/cairn-lang-core/tests/silent_skip_arms.rs` for the caller that
/// bypasses `check` and what it gets instead.
///
/// Returns `None` when the module declares no usable floor, which is the
/// ordinary case: the constraint is optional. Callers wanting the
/// `cairn info` rendering of that (`"0.0"`) should ask [`compute_axes`].
#[must_use]
pub fn declared_version_floor(module: &Module) -> Option<VersionFloor> {
    let mut best: Option<VersionFloor> = None;
    for header in &module.headers {
        if let Header::Requires { requirement, span } = header
            && let Some(version) = parse_min_version(requirement.as_str())
        {
            let strictest = best
                .as_ref()
                .is_none_or(|prev| compare_versions(version, &prev.version).is_gt());
            if strictest {
                best = Some(VersionFloor {
                    version: version.to_owned(),
                    span: span.clone(),
                });
            }
        }
    }
    best
}

/// A version floor a module declares, with the directive it came from.
///
/// The span is what lets a caller outside this crate — the CLI enforcing
/// `--target` against the floor — point at the line that set it rather than
/// at the file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionFloor {
    /// The version as written, already known to be dotted decimal.
    pub version: String,
    /// Byte range of the `@requires` directive that declared it.
    pub span: Span,
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
        // is no candidate, so a consumer reads one shape per reason
        // whatever the answer was.
        let absent = serde_json::to_value(UnsupportedEntry {
            id: "minecraft:nothing_like_this".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition { suggestion: None },
        })
        .expect("the entry serializes");
        assert_eq!(
            absent,
            serde_json::json!({
                "id": "minecraft:nothing_like_this",
                "reason": "absent_from_edition",
                "suggestion": null,
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
