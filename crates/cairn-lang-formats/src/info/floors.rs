//! The per-version verdicts the `buildable targets` row reports, and the notes on the `@requires`
//! floors behind them.

use std::collections::HashSet;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    BuildableRefusal, DeclaredFloor, FloorDeclarer, FloorOrigin, FloorPart, FloorPlacement,
    FloorVerdict, RefusedTarget, TargetRefusal, VersionFloor, declared_version_floors,
    unscoped_version_floors,
};
use cairn_lang_core::{Edition, Module, Resolution, Severity};

use super::Reporter;
use crate::build::finding::Found;
use crate::build::floors::floor_origin_note;
use crate::registry::RegistryPack;

/// How each supported version of one edition answered.
#[derive(Debug, Default)]
pub(super) struct VersionVerdicts<'a> {
    /// Versions a build would accept.
    pub(super) buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, with those errors.
    refused: Vec<(String, Vec<cairn_lang_core::Diagnostic>)>,
    /// Versions below a `@requires` floor, each with the floors refusing it.
    below_floor: Vec<BelowFloor<'a>>,
    /// The floors that refuse any version, in source order and without repeats.
    refusing_floors: Vec<&'a VersionFloor>,
    /// Floors this edition's version table cannot place.
    unplaceable_floors: Vec<&'a VersionFloor>,
}

/// One version below the floors that refuse it.
#[derive(Debug)]
struct BelowFloor<'a> {
    version: String,
    floors: Vec<&'a VersionFloor>,
}

/// Weigh each supported version against the gates `cairn compile` applies to the source.
///
/// `refused_edition_wide` holds what refuses every version the floors let through: scopes that
/// produced no voxels, and palette entries this edition's writer cannot carry.
pub(super) fn weigh_versions<'a>(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    floors: &'a [VersionFloor],
    considered: &[String],
    refused_edition_wide: bool,
) -> VersionVerdicts<'a> {
    let order = pack.version_order();
    let mut verdicts = VersionVerdicts {
        unplaceable_floors: floors
            .iter()
            .filter(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
            .collect(),
        ..VersionVerdicts::default()
    };
    // An unplaceable floor refuses every version; the loop still runs to find what else is wrong.
    let unplaceable = !verdicts.unplaceable_floors.is_empty();
    for version in considered {
        let key = order
            .key_of(version)
            .expect("`considered` is this pack's own version list");
        let refusing: Vec<&VersionFloor> = floors
            .iter()
            .filter(|floor| order.verdict(&floor.version, key) == FloorVerdict::Below)
            .collect();
        if !refusing.is_empty() {
            for floor in &refusing {
                // By span: two lines naming one version are two lines to point at.
                if !verdicts
                    .refusing_floors
                    .iter()
                    .any(|kept| kept.span == floor.span)
                {
                    verdicts.refusing_floors.push(floor);
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
pub(super) fn buildable_refusal(
    source: &str,
    lines: &LineStarts,
    verdicts: &VersionVerdicts<'_>,
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
                    findings: refusals.iter().map(|d| d.render(source, lines)).collect(),
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
pub(super) fn declared_floors(
    source: &str,
    lines: &LineStarts,
    floors: &[&VersionFloor],
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

/// Name the floors the edition-neutral `registry compatibility` row does not read, which beside a
/// refusing `buildable targets` row would otherwise read as a contradiction.
pub(super) fn report_floors_left_out_of_the_neutral_row(out: &mut Reporter<'_>, module: &Module) {
    let neutral = unscoped_version_floors(module);
    let read_by_the_row = |floor: &VersionFloor| {
        neutral
            .iter()
            .any(|kept| kept.span == floor.span && kept.version == floor.version)
    };
    let mut reported = HashSet::new();
    for edition in [Edition::Java, Edition::Bedrock] {
        for floor in declared_version_floors(module, edition) {
            if read_by_the_row(&floor) || !reported.insert((floor.span.clone(), floor.edition)) {
                continue;
            }
            let reason = if floor.edition.is_some() {
                "is scoped to one edition".to_owned()
            } else {
                format!(
                    "is declared by {}, and the two editions bind different variants of it",
                    floor.declarer(),
                )
            };
            let line = format!(
                "note: {}: `{}` {}, so it is not part of the `registry compatibility` row, which \
                 is edition-neutral; `buildable targets` is where it is weighed",
                out.at(floor.span.start),
                floor.rendered(),
                reason,
            );
            out.line(&line);
        }
    }
}

/// What stderr says under one edition's `buildable targets` entry: each refused version's
/// findings, then the `note:` lines on the floors.
pub(super) fn report_version_notes(
    out: &mut Reporter<'_>,
    edition: Edition,
    verdicts: &VersionVerdicts<'_>,
    considered: &[String],
) {
    for (version, refusals) in &verdicts.refused {
        out.line(&format!(
            "note: {} {version} refuses this source",
            edition.as_str()
        ));
        for d in refusals {
            out.finding(&Found::Core(d.clone()));
        }
    }
    for floor in &verdicts.unplaceable_floors {
        let line = format!(
            "note: {}: no {} target can build this source: `{}` names no {} release, so no {} \
             version can be weighed against it",
            out.at(floor.span.start),
            edition.as_str(),
            floor.rendered(),
            edition.as_str(),
            edition.as_str(),
        );
        out.line(&line);
        if let Some(note) = floor_origin_note(floor) {
            out.line(&format!("  {note}"));
        }
        out.line(&format!(
            "  note: {} builds against {}",
            edition.as_str(),
            considered.join(", "),
        ));
    }
    for floor in &verdicts.refusing_floors {
        let below: Vec<&str> = verdicts
            .below_floor
            .iter()
            .filter(|below| below.floors.iter().any(|kept| kept.span == floor.span))
            .map(|below| below.version.as_str())
            .collect();
        let line = format!(
            "note: {}: {} {} {} below the `{}` {} declares",
            out.at(floor.span.start),
            edition.as_str(),
            below.join(", "),
            if below.len() == 1 { "is" } else { "are" },
            floor.rendered(),
            floor.declarer(),
        );
        out.line(&line);
    }
}
