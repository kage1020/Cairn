//! The `@requires` floors: the target a floor rules out of a build, and the per-version verdicts
//! `cairn info` reports.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    BuildableRefusal, DeclaredFloor, FloorDeclarer, FloorOrigin, FloorPart, FloorPlacement,
    FloorVerdict, RefusedTarget, TargetRefusal, VersionFloor, VersionOrder,
};
use cairn_lang_core::{Resolution, Severity};

use super::finding::render_findings;
use crate::registry::RegistryPack;
use crate::target::ResolvedTarget;

/// The line naming the `def` or `theme` that imposed a floor, or `None` for an `@requires` header.
#[must_use]
pub fn floor_origin_note(floor: &VersionFloor) -> Option<String> {
    let inherited_by = match &floor.origin {
        FloorOrigin::Module => return None,
        FloorOrigin::Def(name) => format!("every `place use={name}` inherits it"),
        FloorOrigin::Theme(_) => "every scope that binds the theme inherits it".to_owned(),
    };
    Some(format!(
        "{} declares it, and {inherited_by}",
        floor.declarer()
    ))
}

/// How each supported version of one edition answered.
#[derive(Debug, Default)]
pub struct VersionVerdicts {
    /// Versions a build would accept.
    pub buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, with those errors.
    pub refused: Vec<(String, Vec<cairn_lang_core::check::Diagnostic>)>,
    /// Versions below a `@requires` floor, each with the floors refusing it.
    pub below_floor: Vec<BelowFloor>,
    /// The floors that refuse any version, in source order and without repeats.
    pub refusing_floors: Vec<VersionFloor>,
    /// Floors this edition's version table cannot place.
    pub unplaceable_floors: Vec<VersionFloor>,
}

/// One version below the floors that refuse it.
#[derive(Debug)]
pub struct BelowFloor {
    /// The version, as the registry pack spells it.
    pub version: String,
    /// The floors that refuse it.
    pub floors: Vec<VersionFloor>,
}

/// Weigh each supported version against the gates a compile applies to the source.
///
/// `refused_edition_wide` holds what refuses every version the floors let through: scopes that
/// produced no voxels, and palette entries this edition's writer cannot carry.
///
/// # Panics
///
/// If `considered` names a version `pack` does not order.
#[must_use]
pub fn weigh_versions(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    floors: &[VersionFloor],
    considered: &[String],
    refused_edition_wide: bool,
) -> VersionVerdicts {
    let order = pack.version_order();
    let mut verdicts = VersionVerdicts {
        unplaceable_floors: floors
            .iter()
            .filter(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
            .cloned()
            .collect(),
        ..VersionVerdicts::default()
    };
    // An unplaceable floor refuses every version; the loop still runs to find what else is wrong.
    let unplaceable = !verdicts.unplaceable_floors.is_empty();
    for version in considered {
        let key = order
            .key_of(version)
            .expect("`considered` is this pack's own version list");
        let refusing: Vec<VersionFloor> = floors
            .iter()
            .filter(|floor| order.verdict(&floor.version, key) == FloorVerdict::Below)
            .cloned()
            .collect();
        if !refusing.is_empty() {
            for floor in &refusing {
                // By span: two lines naming one version are two lines to point at.
                if !verdicts
                    .refusing_floors
                    .iter()
                    .any(|kept| kept.span == floor.span)
                {
                    verdicts.refusing_floors.push(floor.clone());
                }
            }
            verdicts.below_floor.push(BelowFloor {
                version: version.clone(),
                floors: refusing,
            });
            continue;
        }
        if refused_edition_wide {
            // The caller reports the refusal edition-wide.
            continue;
        }
        let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
        // Every error refuses the version, not only `E_UNKNOWN_ID`.
        let refusals: Vec<_> = pinned
            .diagnostics
            .into_iter()
            .filter(|d| d.severity() == Severity::Error)
            .collect();
        if refusals.is_empty() {
            verdicts.buildable.push(version.clone());
        } else {
            verdicts.refused.push((version.clone(), refusals));
        }
    }
    if unplaceable {
        verdicts.buildable.clear();
    }
    verdicts
}

/// Why this edition's `buildable` list is empty, every cause at once, or `None` when it is not.
#[must_use]
pub fn buildable_refusal(
    source: &str,
    lines: &LineStarts,
    verdicts: &VersionVerdicts,
    considered: &[String],
    dropped: &[String],
    states_unmapped: &[String],
) -> Option<BuildableRefusal> {
    if !verdicts.buildable.is_empty() {
        return None;
    }
    // A version in neither list was refused edition-wide and gets no entry.
    let versions: Vec<RefusedTarget> = considered
        .iter()
        .filter_map(|version| {
            let refusal = if let Some(below) = verdicts
                .below_floor
                .iter()
                .find(|below| &below.version == version)
            {
                TargetRefusal::BelowFloor {
                    floors: declared_floors(source, lines, &below.floors),
                }
            } else {
                let (_, refusals) = verdicts
                    .refused
                    .iter()
                    .find(|(refused, _)| refused == version)?;
                TargetRefusal::LoweringRefused {
                    findings: render_findings(source, lines, refusals),
                }
            };
            Some(RefusedTarget {
                version: version.clone(),
                refusal,
            })
        })
        .collect();
    let refusal = BuildableRefusal {
        unplaceable_floors: declared_floors(source, lines, &verdicts.unplaceable_floors),
        dropped_scopes: dropped.to_vec(),
        states_unmapped: states_unmapped.to_vec(),
        versions,
    };
    if refusal.unplaceable_floors.is_empty()
        && refusal.dropped_scopes.is_empty()
        && refusal.states_unmapped.is_empty()
        && refusal.versions.is_empty()
    {
        debug_assert!(
            considered.is_empty(),
            "`buildable` is empty over {} considered version(s) and no cause was recorded",
            considered.len(),
        );
        return None;
    }
    Some(refusal)
}

/// Floors as the wire spells them: what they say, and where.
fn declared_floors(
    source: &str,
    lines: &LineStarts,
    floors: &[VersionFloor],
) -> Vec<DeclaredFloor> {
    floors
        .iter()
        .map(|floor| DeclaredFloor {
            declared: floor.rendered(),
            position: lines.position(source, floor.span.start),
            declared_by: match &floor.origin {
                FloorOrigin::Module => None,
                FloorOrigin::Def(name) => Some(FloorDeclarer {
                    keyword: FloorPart::Def,
                    name: name.clone(),
                }),
                FloorOrigin::Theme(name) => Some(FloorDeclarer {
                    keyword: FloorPart::Theme,
                    name: name.clone(),
                }),
            },
        })
        .collect()
}

/// The `@requires` floors one edition's build is held to, with that edition's version order.
#[derive(Debug)]
pub struct DeclaredFloors {
    /// Never empty.
    pub floors: Vec<VersionFloor>,
    /// The target edition's `DataVersion` table.
    pub order: VersionOrder,
}

/// The floor that refuses a target, and on which ground.
#[derive(Debug, Clone, Copy)]
pub enum FloorRefusal<'a> {
    /// The edition's version table cannot place this floor.
    Unplaceable(&'a VersionFloor),
    /// The target sits below this floor.
    Below(&'a VersionFloor),
}

/// The floor that refuses `target`, or `None`; an unplaceable floor is named first, since raising
/// `--target` cannot satisfy it.
#[must_use]
pub fn floor_refusing_target<'a>(
    floors: &'a [VersionFloor],
    order: &VersionOrder,
    target: &ResolvedTarget,
) -> Option<FloorRefusal<'a>> {
    if let Some(floor) = floors
        .iter()
        .find(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
    {
        return Some(FloorRefusal::Unplaceable(floor));
    }
    let target_key = target.order_key();
    floors
        .iter()
        .find(|floor| order.verdict(&floor.version, target_key) == FloorVerdict::Below)
        .map(FloorRefusal::Below)
}
