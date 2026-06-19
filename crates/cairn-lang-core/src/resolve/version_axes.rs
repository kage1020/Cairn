//! Computation of the three version axes reported by `cairn info`.
//!
//! Spec source: `spec/versioning-editions.md` §10.5. The three axes are:
//!
//! 1. **registry-compatible range** `[Vmin, Vmax]` — the intersection of
//!    every `since/until` over the tokens/states the file actually uses.
//! 2. **edition portability** — per edition, how many members compile
//!    portably, are degraded, or are unsupported.
//! 3. **semantic-sensitive members** — registry-valid IDs whose meaning or
//!    behavior shifts at a known boundary version (the catalog half of
//!    `spec/versioning-editions.md` §10.3).
//!
//! M2-PR3 implements axis (1) from `@requires` alone and leaves axes (2)
//! and (3) as **structurally present but empty**: the registry pack and
//! semantic-sensitivity catalog arrive in 2026.12.0, at which point the
//! same `VersionAxes` shape gains real data without a contract break for
//! downstream tooling.

use serde::Serialize;

use crate::ast::{Header, Module};
use crate::intent::{IntentModule, Member, SiteIr, StructIr};

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
    /// Empty in M2-PR3 — populated once the semantic-sensitivity catalog
    /// lands.
    pub semantic_sensitive: Vec<SemanticSensitiveFinding>,
}

/// Registry-compatible Minecraft version range.
///
/// `min` is derived from `@requires version>=X` (the max across all such
/// headers; `"0.0"` when no `@requires` line is present). `max` is the
/// literal string `"latest"` until the registry pack provides a real upper
/// bound in 2026.12.0.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryRange {
    /// Lower bound (inclusive).
    pub min: String,
    /// Upper bound (inclusive). Always `"latest"` in M2-PR3.
    pub max: String,
}

/// Per-edition portability counts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditionPortability {
    /// Edition identifier as supplied on the CLI (`"java"`, `"bedrock"`).
    pub edition: String,
    /// Members that compile straight through.
    pub portable: u32,
    /// Members that compile but lose detail (e.g. stair `shape` on Bedrock).
    pub degraded: u32,
    /// Members that have no representable form on this edition.
    pub unsupported: u32,
}

/// One semantic-sensitivity finding.
///
/// Always empty in M2-PR3; the type exists so the JSON shape is stable from
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
/// `_resolution` is accepted so future work that needs the resolved binding
/// (semantic-sensitivity checks per concrete token) can land without an API
/// change. M2-PR3 only consumes `module.headers` and `ir`.
#[must_use]
pub fn compute_axes(
    module: &Module,
    ir: &IntentModule,
    _resolution: &Resolution,
    editions: &[String],
) -> VersionAxes {
    // Hoisted out of the per-edition loop: the count is edition-agnostic
    // in M2-PR3 (registry pack arrives in 2026.12.0, at which point per-
    // edition counts diverge and the call moves back inside the map).
    let portable_total = count_members(ir);
    VersionAxes {
        registry_compat: RegistryRange {
            min: derive_min_version(module),
            max: "latest".to_owned(),
        },
        edition_portability: editions
            .iter()
            .map(|edition| EditionPortability {
                edition: edition.clone(),
                portable: portable_total,
                degraded: 0,
                unsupported: 0,
            })
            .collect(),
        semantic_sensitive: Vec::new(),
    }
}

fn derive_min_version(module: &Module) -> String {
    let mut best: Option<String> = None;
    for header in &module.headers {
        if let Header::Requires { requirement, .. } = header
            && let Some(v) = parse_min_version(requirement.as_str())
        {
            best = Some(match best {
                None => v.to_owned(),
                Some(prev) => {
                    if compare_versions(v, &prev).is_gt() {
                        v.to_owned()
                    } else {
                        prev
                    }
                }
            });
        }
    }
    best.unwrap_or_else(|| "0.0".to_owned())
}

fn count_members(ir: &IntentModule) -> u32 {
    let mut total = 0u32;
    for s in &ir.structs {
        total = total.saturating_add(count_struct(s));
    }
    for d in &ir.defs {
        total = total.saturating_add(count_def(d));
    }
    for site in &ir.sites {
        total = total.saturating_add(count_site(site));
    }
    total
}

fn count_struct(s: &StructIr) -> u32 {
    count_members_recursive(&s.members)
}

fn count_def(d: &crate::intent::DefIr) -> u32 {
    count_members_recursive(&d.members)
}

fn count_site(site: &SiteIr) -> u32 {
    count_members_recursive(&site.placements)
}

fn count_members_recursive(members: &[Member]) -> u32 {
    let mut total = 0u32;
    for m in members {
        total = total.saturating_add(1);
        total = total.saturating_add(count_members_recursive(&m.children.members));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolve;
    use crate::{lower, parse};

    fn module_with(source: &str) -> (Module, IntentModule, Resolution) {
        let m = parse(source).expect("parse");
        let i = lower(&m);
        let r = resolve(&i);
        (m, i, r)
    }

    #[test]
    fn registry_compat_min_from_requires_header() {
        let src = "@requires version>=1.20\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(&m, &i, &r, &["java".to_owned()]);
        assert_eq!(axes.registry_compat.min, "1.20");
        assert_eq!(axes.registry_compat.max, "latest");
    }

    #[test]
    fn registry_compat_takes_max_when_multiple_requires_present() {
        let src = "@requires version>=1.20\n@requires version>=1.21\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(&m, &i, &r, &["java".to_owned()]);
        assert_eq!(axes.registry_compat.min, "1.21");
    }

    #[test]
    fn registry_compat_defaults_when_requires_absent() {
        let src = "struct s size=4x4\n  walls mat_slot=wall height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(&m, &i, &r, &["java".to_owned()]);
        assert_eq!(axes.registry_compat.min, "0.0");
    }

    #[test]
    fn edition_portability_counts_all_members_as_portable() {
        let src = "struct s size=4x4\n  walls height=3\n  floor\n  roof kind=gable\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(&m, &i, &r, &["java".to_owned(), "bedrock".to_owned()]);
        assert_eq!(axes.edition_portability.len(), 2);
        for ep in &axes.edition_portability {
            assert_eq!(ep.portable, 3);
            assert_eq!(ep.degraded, 0);
            assert_eq!(ep.unsupported, 0);
        }
    }

    #[test]
    fn semantic_sensitive_is_empty_in_m2_pr3() {
        let src = "struct s size=4x4\n  walls height=3\n";
        let (m, i, r) = module_with(src);
        let axes = compute_axes(&m, &i, &r, &["java".to_owned()]);
        assert!(axes.semantic_sensitive.is_empty());
    }
}
