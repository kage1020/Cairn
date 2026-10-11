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
use cairn_lang_formats::registry::RegistryPack;

use crate::cli::EditionArg;
use crate::pipeline::supported_versions;
use crate::report::render_diagnostics;
use crate::target::ResolvedTarget;

/// How to name the line a floor was written on, as a repair points at it.
///
/// The two spellings are `@requires` on a module header and `requires`
/// inside a `def` or `theme` body. A fix line that names the wrong one
/// sends its reader looking for a line the file does not contain.
fn floor_keyword(floor: &VersionFloor) -> &'static str {
    match floor.origin {
        FloorOrigin::Module => "`@requires`",
        FloorOrigin::Def(_) | FloorOrigin::Theme(_) => "`requires`",
    }
}

/// The line naming which part of the module imposed a floor, when a part
/// did.
///
/// `None` for an `@requires` header. The message it would sit under already
/// carries the position of the line, and that line is the file's own, so a
/// note saying "the file declares it" adds a sentence and no fact.
///
/// A member-level floor is the case this exists for. `spec/versioning-editions`
/// "Fail-loud and minimum-version inference" makes the minimum version of a
/// composite the max of its parts, and a target refused by a floor written
/// inside a `def` in a library is not actionable as a bare version number: the
/// reader has to be told whose floor it is and by what route this build
/// inherited it, because the repair is at the other end of that route.
fn floor_origin_note(floor: &VersionFloor) -> Option<String> {
    // Exhaustive, and deliberately not routed through `part()`: the clause
    // is what differs between the routes, and a wildcard here would give a
    // fourth route the prose of one of these three.
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

/// One pack's version table, as the `DataVersion` ordering
/// `spec/versioning-editions` "The target is a compile-time parameter" makes
/// canonical.
///
/// **Every** row, including the ones `--target` may not name. Ordering a
/// floor and building for a version are different questions: the pack has
/// block data for three versions per edition and knows where every release
/// sits, and it is the second set that decides whether a floor can be
/// placed at all. A table holding only the buildable rows is what made a
/// floor naming a real release the pack does not ship — `1.21.1` on Java —
/// indistinguishable from one naming another edition's release.
///
/// Built per call rather than cached: it is a few dozen rows, and a cache
/// would have to be keyed by pack to stay correct once `--registry-pack`
/// can supply one.
pub(crate) fn version_order(pack: &RegistryPack) -> VersionOrder {
    VersionOrder::new(
        pack.data_versions
            .versions
            .iter()
            .map(|entry| (entry.mc_version.clone(), i64::from(entry.data_version))),
    )
}

/// How each supported version of one edition answered.
///
/// Borrows the floors it names rather than carrying them rendered. Two
/// floors on different lines can render alike — `@requires version>=1.21`
/// twice is a file a reader has to be pointed at, not deduplicated for —
/// and the rendered text drops the span, so a note built from it cannot
/// print the position the matching `E_VERSION_CAP` prints.
#[derive(Debug, Default)]
pub(crate) struct VersionVerdicts<'a> {
    /// Versions a build would accept.
    pub(crate) buildable: Vec<String>,
    /// Versions whose pinned lowering raised errors, carrying them: the
    /// caller prints them because nothing else in the run will.
    pub(crate) refused: Vec<(String, Vec<cairn_lang_core::check::Diagnostic>)>,
    /// Versions below the `@requires` floor, which are refused without
    /// being lowered at all — the floor is a relation between the source
    /// and the target, and no id table changes it.
    ///
    /// Each carries the floors that put it there rather than sharing
    /// [`Self::refusing_floors`]: a module declaring two floors may have
    /// one release under the first and the next release under both, and
    /// the repair a reader is sent to is the line that refuses the version
    /// they asked about.
    below_floor: Vec<BelowFloor<'a>>,
    /// The floors that put them there, in source order and without
    /// repeats.
    ///
    /// Carried rather than left to the reader to infer: a module may
    /// declare several floors and only one of them refuse, so a note that
    /// names the versions without naming the line is a report the reader
    /// cannot act on.
    refusing_floors: Vec<&'a VersionFloor>,
    /// Floors this edition's version table cannot place.
    ///
    /// Not a per-version answer, because it is not a per-version fact: a
    /// floor naming no release of the edition can be weighed against none
    /// of them. Kept apart from [`Self::below_floor`] because the two are
    /// different news — "this target is too low" points at `--target`, and
    /// "this floor is not in this edition's numbering" points at the
    /// `@requires` line.
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
/// version refuses, so none is lowered a second time to find that out.
///
/// The floors are weighed by `DataVersion` through the pack's own table
/// (`spec/versioning-editions` "The target is a compile-time parameter"),
/// which is why the order is built here from `pack` rather than passed in:
/// every version in `considered` is a row of that same table, so the key
/// lookup below cannot miss.
pub(crate) fn weigh_versions<'a>(
    ir: &cairn_lang_core::intent::IntentModule,
    resolution: &cairn_lang_core::Resolution,
    pack: &RegistryPack,
    floors: &'a [VersionFloor],
    considered: &[String],
    dropped: &[String],
) -> VersionVerdicts<'a> {
    let order = version_order(pack);
    let mut verdicts = VersionVerdicts {
        unplaceable_floors: floors
            .iter()
            .filter(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
            .collect(),
        ..VersionVerdicts::default()
    };
    // An unplaceable floor answers for every version at once, but it is
    // not the only thing the file may have wrong, and the loop below is
    // where the rest is found. So it is run anyway and `buildable` is
    // emptied after it: what the floor costs is the certification, not
    // the reader's view of the ids.
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
                // By span, not by text: two `@requires` lines naming the
                // same version are two lines, and a reader sent to one of
                // them has to be sent to the right one.
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
            // A partial build is not certified, so this refuses every
            // version at once. The caller reports it edition-wide rather
            // than once per release, which is why nothing is recorded
            // here.
            continue;
        }
        let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
        // Severity rather than a list of codes. `E_UNKNOWN_ID` is the only
        // finding pinning a target raises today, and naming it here would
        // keep reporting a version as buildable the day a second one
        // lands. Nothing is filtered against what the caller already
        // printed: this is reached only after the edition-neutral gate,
        // which returns on any Error, so everything already reported is a
        // warning and no warning refuses a version.
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
        // A floor this edition's table cannot place can be weighed
        // against no release of it, so a lowering that raised nothing
        // says the ids are fine and not that the version builds. The
        // list is the certification and there is none to give.
        verdicts.buildable.clear();
    }
    verdicts
}

/// Why this edition's `buildable` list is empty, or `None` because it is
/// not.
///
/// The same four verdicts `report_version_notes` and the per-version
/// refusal block print as prose, in the shape a consumer reads them —
/// `spec/versioning-editions` "The `buildable targets` row" is the
/// contract. Built here rather than in `weigh_versions` because it needs
/// the source and its line table to spell a floor's position, and the
/// walk is about versions rather than about where anything is written.
///
/// Every cause that holds is reported, rather than the first one found:
/// two of them are the edition's and two are a version's, and a reader
/// told only the edition's would meet the rest one run at a time.
///
/// `None` the moment one version builds: the row is then an answer on its
/// own, and a reason beside a non-empty list would be a reason for
/// something that did not happen.
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
    // In `considered` order rather than verdict order, so the list reads
    // against the row above it. A version in neither list has nothing
    // against it of its own — an edition-wide answer above refused it —
    // and contributes no entry rather than an empty one.
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
        // Only where the pack declares no versions, and there the empty
        // `considered` beside the empty `buildable` is the whole answer.
        // With versions to weigh, each one lands in a verdict list or is
        // refused edition-wide, so at least one field is filled.
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
            // Matched rather than read off `FloorOrigin::part`, so the
            // route a floor arrived by is answered here too: a fourth
            // variant stops this arm rather than passing its keyword
            // through as a string nothing checks.
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
/// Built only for a source that declares a floor, so a floor-less compile
/// never builds the order, and built once, so the id-check gate in
/// [`load_and_lower`](crate::pipeline::load_and_lower) and the report in [`enforce_version_floor`] ask
/// [`floor_refusing_target`] of the same floors against the same order.
pub(crate) struct DeclaredFloors {
    /// Never empty.
    ///
    /// Every applicable floor rather than the strictest of them: picking
    /// the strictest means ordering two floors against each other, and the
    /// order is the target edition's `DataVersion` table — which the
    /// enforcement below already consults once per floor, reaching the
    /// same answer without the extra comparison.
    pub(crate) floors: Vec<VersionFloor>,
    /// The target edition's `DataVersion` table, as [`version_order`]
    /// builds it.
    pub(crate) order: VersionOrder,
}

/// Refuse a `--target` the floors the source declares rule out.
///
/// Checked here rather than in `check()`: the constraint is a relation
/// between the source and `--target`, and the lockfile is what must not
/// certify a target the source disowns. `cairn check --target` writes no
/// lock and is deliberately not held to the floors. This runs before any
/// artifact is prepared, so a refusal leaves nothing on disk.
///
/// The ordering key is the target edition's `DataVersion` table
/// ([versioning-editions](https://cairn-lang.dev/spec/versioning-editions)
/// "The target is a compile-time parameter"): Java and Bedrock number their
/// releases differently, so a floor the table cannot place is refused as its
/// own failure rather than compared by its text. Which floor is reported,
/// and as which failure, is [`floor_refusing_target`]'s answer: the same
/// answer that kept [`load_and_lower`](crate::pipeline::load_and_lower) from pinning the target.
///
/// # Errors
///
/// Returns exit code 1 when a floor refuses the target, or names a version
/// this edition's table cannot place.
pub(crate) fn enforce_version_floor(
    file: &Path,
    source: &str,
    declared: &DeclaredFloors,
    edition: EditionArg,
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
    // makes the closed set of candidates valid in the target part of the
    // message, not an extra. Naming the floor alone sends an author to
    // `--target >=99.0`, which is a second error and no closer to a build;
    // whether *any* supported target satisfies the floor is the fact that
    // decides what they do next.
    //
    // Every floor, not the one being reported. A candidate that clears this
    // floor and trips the next one is the same second error in a different
    // spelling — the offer has to be a target that builds.
    //
    // And "builds" means the buildable rows, not every row the order
    // carries: the table also names the releases the pack can only order
    // against, and offering one of those sends the author to `unsupported
    // target` — the second error this list exists to prevent.
    let buildable = supported_versions(edition.registry_pack());
    let usable = versions_satisfying(order, floors, &buildable);
    if usable.is_empty() {
        eprintln!(
            "  no supported {} target satisfies it: {}",
            edition.as_str(),
            edition.registry_pack().supported_list(),
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
/// Two grounds because they send the author to different lines, under
/// different codes: a target below a floor is `E_VERSION_CAP`, repaired by
/// `--target` or by lowering the floor, and a floor the edition's table
/// cannot place is `E_REQUIRES_UNORDERABLE` at every target, repaired on
/// the floor's own line.
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
/// The one predicate both halves of a compile's version check ask:
/// [`load_and_lower`](crate::pipeline::load_and_lower) pins no version for a target this refuses, and
/// [`enforce_version_floor`] reports the floor it names. So the id check is
/// suspended for exactly the targets the build is then refused on the
/// version axis, and a run that suspended it never writes.
///
/// An unplaceable floor is named ahead of a below-target one wherever the
/// two sit in the source, for the reason `E_INVALID_REQUIRES` precedes
/// `E_VERSION_CAP`: it is the line the author can act on, and a cap
/// reported beside it would tell them to raise `--target` when the mistake
/// is that the floor names no version of what they are building. Among the
/// floors on one ground, the first in source order is named.
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
/// Its own code rather than `E_VERSION_CAP`, because it is not a cap: no
/// `--target` satisfies this floor and none violates it either. The
/// build's edition numbers its releases in a scheme the floor is not
/// written in, so the repair is on the `@requires` line — either the
/// edition scope that says which scheme it meant, or a version of the one
/// being built.
fn report_unplaceable_floor(
    file: &Path,
    source: &str,
    floor: &VersionFloor,
    edition: EditionArg,
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
    // The two releases it falls between, not the whole table. The table
    // names every release of the edition now, so printing it is dozens of
    // versions where two say the same thing: where the label would sit if
    // it were one of them, and that it is not.
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
    // The scope is offered only when it would change the answer, and what
    // decides that is whether the *other* edition's table names the label
    // — a row, or the pre-release of one — not whether this floor carries
    // a scope already. Recommending one without checking is recommending a
    // guess: scoped to an edition that does not name it either, the floor
    // goes inert there and the constraint the author wrote evaporates. A
    // label the other table places below or above every row is a
    // comparison rather than a release, and scoped there it is the same
    // inert floor: satisfied by every target of that edition, or by none.
    let other = edition.other();
    let other_edition_names_it = floor.edition.is_none()
        && version_order(other.registry_pack())
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
/// reports `0.0 .. latest` — which reads as "no constraint" beside a
/// `buildable targets` row that refuses versions. The two lines are both
/// right and disagree on their face, so the reason goes on stderr rather
/// than being left for the reader to work out.
///
/// Two reasons a floor is left out, and the note says which. A floor that
/// names an edition means nothing in the other's numbering. A floor a
/// `theme` declares is left out when the two editions bind different
/// variants of that theme, because then only one of them inherits it —
/// a case that arrived with the member-level floor and that a filter on
/// the floor's own scope cannot see, since such a floor names no edition.
///
/// Derived by difference rather than by re-deciding either rule: whatever
/// a build of either edition is held to and the neutral row does not read
/// is, by construction, exactly what this has to name.
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
/// Extracted from `edition_rows` so the loop there reads as the five
/// steps it is. Both notes name the `requires` line they came from — and
/// the part that declared it, when a part did — at the position
/// `E_VERSION_CAP` would print for the same floor: a report that says which
/// versions are out without saying which line put them there is half a
/// report.
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
        // The versions *this* floor refuses, not every version some floor
        // refuses: with two floors at different heights the second reads
        // "1.21.40 is below `version>=1.21.40`", which is not true of the
        // version and sends the reader to a line that does not refuse it.
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
