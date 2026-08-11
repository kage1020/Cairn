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
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VersionAxes {
    /// Axis 1: registry-compatible version range.
    pub registry_compat: RegistryRange,
    /// Axis 2: per-edition portability counts, in the order requested by
    /// the caller (CLI honours `--editions`).
    pub edition_portability: Vec<EditionPortability>,
    /// Axis 3: members whose meaning shifts at a known boundary version.
    /// Empty until the semantic-sensitivity catalog lands.
    pub semantic_sensitive: Vec<SemanticSensitiveFinding>,
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
    /// Members that compile straight through.
    pub portable: u32,
    /// Members that compile but lose detail (e.g. stair `shape` on Bedrock).
    pub degraded: u32,
    /// Members that have no representable form on this edition.
    pub unsupported: u32,
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
/// `_resolution` is accepted so future work that needs the resolved binding
/// (semantic-sensitivity checks per concrete token) can land without an API
/// change. Currently only `module.headers` and `edition_portability` are
/// consumed.
#[must_use]
pub fn compute_axes(
    module: &Module,
    _ir: &IntentModule,
    _resolution: &Resolution,
    edition_portability: Vec<EditionPortability>,
) -> VersionAxes {
    VersionAxes {
        registry_compat: RegistryRange {
            min: derive_min_version(module),
            max: "latest".to_owned(),
        },
        edition_portability,
        semantic_sensitive: Vec::new(),
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
/// `[a, ∞) ∩ [b, ∞)` is `[max(a, b), ∞)`. Requirements the grammar refuses
/// declare nothing and are skipped here — they are reported by
/// `check::requires`, and honouring half of an expression that has already
/// been called an error would be the worst of both.
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

    /// Fabricate a per-edition portability list matching the shape the CLI
    /// forwards from `cairn-lang-formats::portability`. Used to keep the
    /// axis-1 / axis-3 tests independent of the axis-2 data source.
    fn synthetic_portability(entries: &[(Edition, u32, u32, u32)]) -> Vec<EditionPortability> {
        entries
            .iter()
            .map(
                |&(edition, portable, degraded, unsupported)| EditionPortability {
                    edition,
                    portable,
                    degraded,
                    unsupported,
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
            synthetic_portability(&[(Edition::Java, 3, 0, 0), (Edition::Bedrock, 1, 1, 1)]),
        );
        assert_eq!(axes.edition_portability.len(), 2);
        assert_eq!(axes.edition_portability[0].edition, Edition::Java);
        assert_eq!(axes.edition_portability[0].portable, 3);
        assert_eq!(axes.edition_portability[0].degraded, 0);
        assert_eq!(axes.edition_portability[0].unsupported, 0);
        assert_eq!(axes.edition_portability[1].edition, Edition::Bedrock);
        assert_eq!(axes.edition_portability[1].portable, 1);
        assert_eq!(axes.edition_portability[1].degraded, 1);
        assert_eq!(axes.edition_portability[1].unsupported, 1);
        // JSON wire shape must remain unchanged so downstream consumers
        // treating `edition_portability[].edition` as a lowercase string
        // continue to work under the enum-typed field.
        let json = serde_json::to_string(&axes).unwrap();
        assert!(json.contains(r#""edition":"java""#), "got: {json}");
        assert!(json.contains(r#""edition":"bedrock""#), "got: {json}");
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
