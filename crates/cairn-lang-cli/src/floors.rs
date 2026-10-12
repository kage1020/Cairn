//! The `@requires` floors: the compile's refusal of a target a floor rules out, and the
//! per-version verdicts `cairn info` reports.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    BuildableRefusal, DeclaredFloor, FloorDeclarer, FloorOrigin, FloorPart, FloorPlacement,
    FloorVerdict, RefusedTarget, TargetRefusal, VersionFloor, VersionOrder,
    declared_version_floors, unscoped_version_floors, versions_satisfying,
};
use cairn_lang_core::{Edition, Module, Severity};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::report::render_diagnostics;
use crate::target::{ResolvedTarget, supported_versions};

fn floor_keyword(floor: &VersionFloor) -> &'static str {
    match floor.origin {
        FloorOrigin::Module => "`@requires`",
        FloorOrigin::Def(_) | FloorOrigin::Theme(_) => "`requires`",
    }
}

/// The line naming the `def` or `theme` that imposed a floor, or `None` for an `@requires` header.
fn floor_origin_note(floor: &VersionFloor) -> Option<String> {
    let inherited_by = match &floor.origin {
        FloorOrigin::Module => return None,
        FloorOrigin::Def(name) => format!("every `place use={name}` inherits it"),
        FloorOrigin::Theme(_) => "every scope that binds the theme inherits it".to_owned(),
    };
    Some(format!(
        "  {} declares it, and {inherited_by}",
        floor.declarer(),
    ))
}

/// How each supported version of one edition answered.
#[derive(Debug, Default)]
pub(crate) struct VersionVerdicts<'a> {
    /// Versions a build would accept.
    pub(crate) buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, with those errors.
    pub(crate) refused: Vec<(String, Vec<cairn_lang_core::check::Diagnostic>)>,
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

/// Weigh each supported version against the gates `run_compile` applies to the source.
pub(crate) fn weigh_versions<'a>(
    ir: &cairn_lang_core::intent::IntentModule,
    resolution: &cairn_lang_core::Resolution,
    pack: &RegistryPack,
    floors: &'a [VersionFloor],
    considered: &[String],
    dropped: &[String],
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
        if !dropped.is_empty() {
            // The caller reports a partial build edition-wide.
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
pub(crate) fn buildable_refusal(
    source: &str,
    lines: &LineStarts,
    verdicts: &VersionVerdicts<'_>,
    considered: &[String],
    dropped: &[String],
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
                    findings: render_diagnostics(source, lines, refusals),
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
        versions,
    };
    if refusal.unplaceable_floors.is_empty()
        && refusal.dropped_scopes.is_empty()
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

/// The `@requires` floors one edition's build is held to, with that edition's version order.
pub(crate) struct DeclaredFloors {
    /// Never empty.
    pub(crate) floors: Vec<VersionFloor>,
    /// The target edition's `DataVersion` table.
    pub(crate) order: VersionOrder,
}

/// Refuse a `--target` the floors the source declares rule out.
///
/// # Errors
///
/// Exit code 1 when a floor refuses the target or names a version the edition cannot place.
pub(crate) fn enforce_version_floor(
    file: &Path,
    source: &str,
    declared: &DeclaredFloors,
    edition: Edition,
    target: &ResolvedTarget,
) -> Result<(), ExitCode> {
    let DeclaredFloors { floors, order } = declared;
    let floor = match floor_refusing_target(floors, order, target) {
        None => return Ok(()),
        Some(FloorRefusal::Unplaceable(floor)) => {
            report_unplaceable_floor(file, source, floor, edition, order);
            return Err(ExitCode::from(1));
        }
        Some(FloorRefusal::Below(floor)) => floor,
    };
    let position = LineStarts::new(source).position(source, floor.span.start);
    eprintln!(
        "error[E_VERSION_CAP]: {}:{}: {} requires {} (target {}).",
        file.display(),
        position,
        floor.declarer(),
        floor.rendered(),
        target.mc_version(),
    );
    if let Some(note) = floor_origin_note(floor) {
        eprintln!("{note}");
    }
    // `spec/versioning-editions` "Fail-loud and minimum-version inference": name the valid targets.
    let buildable = supported_versions(builtin_pack(edition));
    let usable = versions_satisfying(order, floors, &buildable);
    if usable.is_empty() {
        eprintln!(
            "  no supported {} target satisfies it: {}",
            edition.as_str(),
            builtin_pack(edition).supported_list(),
        );
        eprintln!(
            "  fix: lower the {} floor, or build against another edition",
            floor_keyword(floor),
        );
    } else {
        eprintln!(
            "  valid {} targets: {}",
            edition.as_str(),
            usable.join(", ")
        );
        eprintln!(
            "  fix: --target {}, or lower the {} floor",
            usable[0],
            floor_keyword(floor),
        );
    }
    Err(ExitCode::from(1))
}

/// The floor that refuses a target, and on which ground.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FloorRefusal<'a> {
    /// The edition's version table cannot place this floor.
    Unplaceable(&'a VersionFloor),
    /// The target sits below this floor.
    Below(&'a VersionFloor),
}

/// The floor that refuses `target`, or `None`; an unplaceable floor is named first, since raising
/// `--target` cannot satisfy it.
pub(crate) fn floor_refusing_target<'a>(
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
    let target_key = i64::from(target.version_int());
    floors
        .iter()
        .find(|floor| order.verdict(&floor.version, target_key) == FloorVerdict::Below)
        .map(FloorRefusal::Below)
}

/// Report a floor the target edition's table cannot place, as `E_REQUIRES_UNORDERABLE`.
fn report_unplaceable_floor(
    file: &Path,
    source: &str,
    floor: &VersionFloor,
    edition: Edition,
    order: &VersionOrder,
) {
    let position = LineStarts::new(source).position(source, floor.span.start);
    eprintln!(
        "error[E_REQUIRES_UNORDERABLE]: {}:{}: {} requires {}, which names no {} release.",
        file.display(),
        position,
        floor.declarer(),
        floor.rendered(),
        edition.as_str(),
    );
    if let Some(note) = floor_origin_note(floor) {
        eprintln!("{note}");
    }
    match order.neighbours(&floor.version) {
        (Some(below), Some(above)) => eprintln!(
            "  {edition_name} ships {below} and then {above}, and nothing between them; \
             Cairn orders versions by DataVersion and a label that names no release has none",
            edition_name = edition.as_str(),
        ),
        _ => eprintln!(
            "  {} releases: {}",
            edition.as_str(),
            order
                .rows()
                .map(|(label, _)| label)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
    // Offer an edition scope only where the other edition names the label; elsewhere the floor
    // would go inert.
    let other = match edition {
        Edition::Java => Edition::Bedrock,
        Edition::Bedrock => Edition::Java,
    };
    let other_edition_names_it = floor.edition.is_none()
        && builtin_pack(other)
            .version_order()
            .place(&floor.version)
            .names_release();
    if other_edition_names_it {
        eprintln!(
            "  `{}` is a {} release; if that is the numbering this floor is written in, say so",
            floor.version,
            other.as_str(),
        );
        eprintln!(
            "  fix: `@requires {} version>={}`, or name a {} release",
            other.as_str(),
            floor.version,
            edition.as_str(),
        );
    } else {
        eprintln!("  fix: name a {} release", edition.as_str());
    }
}

/// Name the floors the edition-neutral `registry compatibility` row does not read, which beside a
/// refusing `buildable targets` row would otherwise read as a contradiction.
pub(crate) fn report_floors_left_out_of_the_neutral_row(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    module: &Module,
) {
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
            eprintln!(
                "note: {}:{}: `{}` {}, so it is not part of the \
                 `registry compatibility` row, which is edition-neutral; `buildable targets` is \
                 where it is weighed",
                file.display(),
                lines.position(source, floor.span.start),
                floor.rendered(),
                reason,
            );
        }
    }
}

/// The `note:` lines under one edition's `buildable targets` entry.
pub(crate) fn report_version_notes(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    edition: Edition,
    verdicts: &VersionVerdicts<'_>,
    considered: &[String],
) {
    for floor in &verdicts.unplaceable_floors {
        eprintln!(
            "note: {}:{}: no {} target can build this source: `{}` names no {} release, \
             so no {} version can be weighed against it",
            file.display(),
            lines.position(source, floor.span.start),
            edition.as_str(),
            floor.rendered(),
            edition.as_str(),
            edition.as_str(),
        );
        if let Some(note) = floor_origin_note(floor) {
            eprintln!("{note}");
        }
        eprintln!(
            "  note: {} builds against {}",
            edition.as_str(),
            considered.join(", "),
        );
    }
    for floor in &verdicts.refusing_floors {
        let below: Vec<&str> = verdicts
            .below_floor
            .iter()
            .filter(|below| below.floors.iter().any(|kept| kept.span == floor.span))
            .map(|below| below.version.as_str())
            .collect();
        eprintln!(
            "note: {}:{}: {} {} {} below the `{}` {} declares",
            file.display(),
            lines.position(source, floor.span.start),
            edition.as_str(),
            below.join(", "),
            if below.len() == 1 { "is" } else { "are" },
            floor.rendered(),
            floor.declarer(),
        );
    }
}
