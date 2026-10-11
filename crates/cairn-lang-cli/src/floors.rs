//! The `@requires` floors and the version table they are weighed in:
//! the compile's refusal of a target a floor rules out, and the
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

use crate::pipeline::supported_versions;
use crate::report::render_diagnostics;
use crate::target::ResolvedTarget;

/// How a repair names the line a floor was written on: `@requires` on a
/// module header, `requires` inside a `def` or `theme` body.
fn floor_keyword(floor: &VersionFloor) -> &'static str {
    match floor.origin {
        FloorOrigin::Module => "`@requires`",
        FloorOrigin::Def(_) | FloorOrigin::Theme(_) => "`requires`",
    }
}

/// The line naming which part of the module imposed a floor, or `None` for
/// an `@requires` header, whose position the message already carries.
///
/// `spec/versioning-editions` "Fail-loud and minimum-version inference"
/// makes a composite's minimum version the max of its parts, so a floor
/// written inside a library `def` is not actionable as a bare version: the
/// repair is at the other end of the route this build inherited it by.
fn floor_origin_note(floor: &VersionFloor) -> Option<String> {
    // Exhaustive rather than through `part()`, so a fourth route cannot
    // inherit one of these clauses.
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
///
/// Borrows the floors rather than carrying them rendered: two floors on
/// different lines can render alike, and only the span gives a note the
/// position the matching `E_VERSION_CAP` prints.
#[derive(Debug, Default)]
pub(crate) struct VersionVerdicts<'a> {
    /// Versions a build would accept.
    pub(crate) buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, with those errors,
    /// which nothing else in the run prints.
    pub(crate) refused: Vec<(String, Vec<cairn_lang_core::check::Diagnostic>)>,
    /// Versions below a `@requires` floor, refused without being lowered,
    /// since no id table changes a floor's relation to the target.
    ///
    /// Each carries its own refusing floors: with two floors, one release
    /// may be under the first and the next under both, and the reader is
    /// sent to the line refusing the version they asked about.
    below_floor: Vec<BelowFloor<'a>>,
    /// The floors that put them there, in source order and without
    /// repeats, so a note names the refusing line among several floors.
    refusing_floors: Vec<&'a VersionFloor>,
    /// Floors this edition's version table cannot place.
    ///
    /// Not a per-version fact, and kept apart from [`Self::below_floor`]
    /// because it points elsewhere: a target too low points at `--target`,
    /// a floor outside the edition's numbering at the `@requires` line.
    unplaceable_floors: Vec<&'a VersionFloor>,
}

/// One version below the floors that refuse it.
#[derive(Debug)]
struct BelowFloor<'a> {
    version: String,
    floors: Vec<&'a VersionFloor>,
}

/// Weigh each supported version against the gates `run_compile` applies to
/// the source.
///
/// `dropped` is the edition's unlowered scopes: non-empty means every
/// version refuses, so none is lowered again to find that out.
///
/// The floors are weighed by `DataVersion` through `pack`'s own table
/// (`spec/versioning-editions` "The target is a compile-time parameter"),
/// built here rather than passed in so that every version in `considered`
/// is one of its rows and the key lookup cannot miss.
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
    // An unplaceable floor refuses every version, but the loop still runs
    // to find whatever else is wrong; `buildable` is emptied after it.
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
                // By span, not text: two lines naming the same version are
                // two lines to point at.
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
            // A partial build is not certified. The caller reports that
            // edition-wide, so nothing is recorded per version.
            continue;
        }
        let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
        // By severity rather than by code, so any error a pinned target
        // raises refuses the version, not only `E_UNKNOWN_ID`. Nothing the
        // caller printed needs filtering out: the edition-neutral gate ahead
        // of this returns on any error, and no warning refuses a version.
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
        // A clean lowering says the ids are fine, not that the version
        // builds: a floor no release can be weighed against leaves nothing
        // to certify.
        verdicts.buildable.clear();
    }
    verdicts
}

/// Why this edition's `buildable` list is empty, or `None` when it is not.
///
/// The verdicts `report_version_notes` and the per-version refusal block
/// print as prose, in the shape `spec/versioning-editions` "The `buildable
/// targets` row" gives a consumer. Built apart from `weigh_versions`
/// because spelling a floor's position needs the source and its line
/// table.
///
/// Every cause that holds is reported, not the first one found, so a
/// reader does not meet them one run at a time.
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
    // In `considered` order, so the list reads against the row above it.
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
        // Only when the pack declares no versions: otherwise each one lands
        // in a verdict list or is refused edition-wide.
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
            // Matched rather than read off `FloorOrigin::part`, so a fourth
            // variant stops here instead of passing an unchecked keyword on.
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

/// The `@requires` floors one edition's build is held to, with that
/// edition's version order to weigh them by.
///
/// Built only for a source that declares a floor, and once, so the
/// id-check gate in [`load_and_lower`](crate::pipeline::load_and_lower) and
/// [`enforce_version_floor`] ask [`floor_refusing_target`] of the same
/// floors against the same order.
pub(crate) struct DeclaredFloors {
    /// Never empty. Every applicable floor rather than the strictest, since
    /// enforcement weighs each one against the table anyway.
    pub(crate) floors: Vec<VersionFloor>,
    /// The target edition's `DataVersion` table, as [`RegistryPack::version_order`]
    /// builds it.
    pub(crate) order: VersionOrder,
}

/// Refuse a `--target` the floors the source declares rule out.
///
/// Checked here rather than in `check()`, because it is the lockfile that
/// must not certify a target the source disowns; `cairn check --target`
/// writes no lock and is not held to the floors. Runs before any artifact
/// is prepared, so a refusal leaves nothing on disk.
///
/// Floors are ordered by the target edition's `DataVersion` table
/// (`spec/versioning-editions` "The target is a compile-time parameter"),
/// so one the table cannot place is its own failure rather than compared
/// by its text. Which floor is reported, and how, is
/// [`floor_refusing_target`]'s answer, the same one that kept
/// [`load_and_lower`](crate::pipeline::load_and_lower) from pinning the
/// target.
///
/// # Errors
///
/// Returns exit code 1 when a floor refuses the target, or names a version
/// this edition's table cannot place.
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
    // `spec/versioning-editions` "Fail-loud and minimum-version inference"
    // makes the valid candidates part of the message. They satisfy every
    // floor, not only this one, and come from the buildable rows, not every
    // row the order carries, so none of them is a second error.
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
///
/// A target below a floor is `E_VERSION_CAP`, repaired by `--target` or
/// by lowering the floor; a floor the edition's table cannot place is
/// `E_REQUIRES_UNORDERABLE` at every target, repaired on its own line.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FloorRefusal<'a> {
    /// The edition's version table cannot place this floor, so no target
    /// satisfies it and none violates it.
    Unplaceable(&'a VersionFloor),
    /// The target sits below this floor.
    Below(&'a VersionFloor),
}

/// The floor that refuses `target`, and why, or `None` when every floor
/// lets it through.
///
/// Both halves of a compile's version check ask this:
/// [`load_and_lower`](crate::pipeline::load_and_lower) pins no version for a
/// target it refuses, and [`enforce_version_floor`] reports the floor it
/// names. So the id check is suspended for exactly the targets the build is
/// then refused, and a run that suspended it never writes.
///
/// An unplaceable floor is named ahead of a below-target one wherever they
/// sit, as `E_INVALID_REQUIRES` precedes `E_VERSION_CAP`: a cap beside it
/// would send the author to raise `--target` when the floor names no
/// version of what they build. Among floors on one ground, the first in
/// source order is named.
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

/// Report a floor the target edition's table cannot place.
///
/// Not `E_VERSION_CAP`: no `--target` satisfies this floor or violates
/// it, so the repair is on the `@requires` line, either an edition scope
/// naming the numbering it meant or a version of the edition being built.
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
    // The two releases it falls between, rather than dozens of rows.
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
    // Offer an edition scope only when the other edition's table names the
    // label as a release or the pre-release of one. Scoped to an edition
    // that does not, or that places it below or above every row, the floor
    // goes inert and the author's constraint is lost.
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

/// Name the floors the `registry compatibility` row does not read.
///
/// That row is edition-neutral, so a file whose floors it all leaves out
/// reports `0.0 .. latest`, which beside a `buildable targets` row refusing
/// versions reads as a contradiction; the note says why.
///
/// A floor is left out for naming an edition, or for being declared by a
/// `theme` the two editions bind different variants of, so only one
/// inherits it. Derived by difference rather than by re-deciding either
/// rule: what a build of either edition is held to and the neutral row does
/// not read is exactly what this names.
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
///
/// Each names the `requires` line it came from, and the declaring part
/// when a part did, at the position `E_VERSION_CAP` prints for the same
/// floor.
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
        // Only the versions this floor refuses: with two floors at
        // different heights, every refused version would put `1.21.40`
        // "below `version>=1.21.40`".
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
