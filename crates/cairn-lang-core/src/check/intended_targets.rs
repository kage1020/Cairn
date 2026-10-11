//! `intended_targets` pass: weighs `@intended_targets` against the version
//! floors the same file declares and against the versions the edition can
//! build, so `@requires version>=1.21` beside `@intended_targets ["1.20.4"]`
//! does not pass unremarked.
//!
//! It runs outside [`super::check`] because every question here is
//! answered by the target edition's `DataVersion` table
//! (`spec/versioning-editions` "The target is a compile-time parameter"),
//! which reaches this crate only as a [`VersionOrder`] built from a
//! registry pack. Comparing version labels as text instead would accept
//! "1.21.40 satisfies version>=1.21.4".
//!
//! Each version the header names is one of:
//!
//! - not one the edition builds: `W_INTENDED_TARGET_UNSUPPORTED`, whether
//!   the table places it as a release the pack ships no block data for
//!   (`1.19` on Java) or cannot place it at all (`1.21.40` on Java, which
//!   is Bedrock's numbering);
//! - below a floor the file declares;
//! - buildable and at or above every floor, which raises nothing.
//!
//! The cap splits by reach, counted over the buildable versions only. All
//! of them below a floor is `E_INTENDED_TARGET_CAP`: the file builds for
//! nothing it is meant for. Some of them is `W_INTENDED_TARGET_CAP`,
//! because `spec/syntax` "Headers" calls this header "a hint, not a
//! verification record" and the versions above the floor still build.
//! Leaving unbuildable names out of the count keeps `["1.20.4", "1.19"]`
//! under `version>=1.21` an error.

use crate::ast::{Header, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::resolve::{VersionFloor, declared_version_floors, versions_satisfying};
use crate::version::{FloorVerdict, VersionOrder, compare_versions};

/// Weigh every `@intended_targets` version a module declares against the
/// floors a build of `edition` is held to, for the caller to merge with
/// the findings of [`super::check`].
///
/// `order` is that edition's release table and `targetable` the versions
/// its registry pack builds, the same two lists `cairn compile` weighs
/// `--target` against. A `targetable` entry `order` cannot place is
/// dropped rather than panicked on, since no `--target` could be ordered
/// against it either.
#[must_use]
pub fn weigh_intended_targets(
    module: &Module,
    edition: Edition,
    order: &VersionOrder,
    targetable: &[String],
) -> Vec<Diagnostic> {
    let floors = declared_version_floors(module, edition);
    let mut findings = Vec::new();
    for header in &module.headers {
        let Header::IntendedTargets { targets, span } = header else {
            continue;
        };
        // An empty list states no intention for a floor to contradict.
        if targets.is_empty() {
            continue;
        }
        let weighed: Vec<(&str, Verdict<'_>)> = targets
            .iter()
            .map(|target| (target.as_str(), weigh(target, &floors, order, targetable)))
            .collect();
        if let Some(finding) = capped(&weighed, span, edition, order, &floors, targetable) {
            findings.push(finding);
        }
        if let Some(finding) = unsupported(&weighed, span, edition, targetable) {
            findings.push(finding);
        }
    }
    findings
}

/// What one intended version is, weighed against one edition.
enum Verdict<'a> {
    /// A version the edition builds, at or above every floor that applies.
    Buildable,
    /// A version the edition builds, below a floor the file declares: the
    /// first in source order that refuses it, as for `E_VERSION_CAP`, so
    /// an equivalent line appended below does not move the finding.
    BelowFloor(&'a VersionFloor),
    /// A release of this edition the registry pack ships no block data
    /// for, so `--target` cannot name it.
    NotShipped,
    /// Not a release of this edition: a label its table cannot place,
    /// typically the other edition's numbering, or one outside its span.
    NoSuchRelease,
}

/// Place one intended version, then weigh it against the floors only if
/// the edition can build it.
///
/// A cap reported beside "this target does not exist here" would send the
/// author to change a floor that is not what stops the build.
fn weigh<'a>(
    label: &str,
    floors: &'a [VersionFloor],
    order: &VersionOrder,
    targetable: &[String],
) -> Verdict<'a> {
    // Weighed by its `DataVersion` key, never by the label's text.
    let buildable = targetable
        .iter()
        .find(|row| compare_versions(row, label).is_eq())
        .and_then(|row| order.key_of(row));
    let Some(key) = buildable else {
        return if order.place(label).names_release() {
            Verdict::NotShipped
        } else {
            Verdict::NoSuchRelease
        };
    };
    floors
        .iter()
        .find(|floor| order.verdict(&floor.version, key) == FloorVerdict::Below)
        .map_or(Verdict::Buildable, Verdict::BelowFloor)
}

/// The cap finding, when any intended version is below a floor.
///
/// One finding for the header, whose repair is one edit, with a note per
/// version naming the floor that put it out.
fn capped(
    weighed: &[(&str, Verdict<'_>)],
    span: &Span,
    edition: Edition,
    order: &VersionOrder,
    floors: &[VersionFloor],
    targetable: &[String],
) -> Option<Diagnostic> {
    let below: Vec<(&str, &VersionFloor)> = weighed
        .iter()
        .filter_map(|(target, verdict)| match verdict {
            Verdict::BelowFloor(floor) => Some((*target, *floor)),
            Verdict::Buildable | Verdict::NotShipped | Verdict::NoSuchRelease => None,
        })
        .collect();
    let (_, first_floor) = *below.first()?;
    // Counted over buildable versions only (see the module doc). `below`
    // is non-empty, so `every` never means "every one of none".
    let buildable = weighed
        .iter()
        .filter(|(_, verdict)| matches!(verdict, Verdict::Buildable | Verdict::BelowFloor(_)))
        .count();
    let every = below.len() == buildable;
    let named = below
        .iter()
        .map(|(target, _)| *target)
        .collect::<Vec<_>>()
        .join(", ");
    let code = if every {
        DiagnosticCode::IntendedTargetCap
    } else {
        DiagnosticCode::IntendedTargetCapPartial
    };
    let primary = if every {
        format!(
            "`@intended_targets` names no {edition} version this file can be built for: \
             {named} {} below a version floor the file declares",
            if below.len() == 1 { "is" } else { "are" },
        )
    } else {
        format!(
            "`@intended_targets` names {named}, below a version floor the file declares, \
             so no {edition} build can be made for {}",
            if below.len() == 1 { "it" } else { "them" },
        )
    };
    let mut notes: Vec<DiagnosticNote> = below
        .iter()
        .map(|(target, floor)| DiagnosticNote {
            span: Some(floor.span.clone()),
            message: format!(
                "{target} is below `{}`, which {} declares",
                floor.rendered(),
                floor.declarer(),
            ),
        })
        .collect();
    notes.push(candidates_note(edition, order, floors, targetable));
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "fix: name a version at or above the floor in `@intended_targets`, or lower the {} \
             floor",
            keyword(first_floor),
        ),
    });
    Some(Diagnostic {
        code,
        span: span.clone(),
        primary,
        notes,
        data: Some(DiagnosticData::IntendedTargets {
            edition: edition.as_str().to_owned(),
            targets: below
                .iter()
                .map(|(target, _)| (*target).to_owned())
                .collect(),
            floor: Some(first_floor.rendered()),
        }),
    })
}

/// The finding for versions no `--target` of this edition names.
fn unsupported(
    weighed: &[(&str, Verdict<'_>)],
    span: &Span,
    edition: Edition,
    targetable: &[String],
) -> Option<Diagnostic> {
    let outside: Vec<(&str, String)> = weighed
        .iter()
        .filter_map(|(target, verdict)| match verdict {
            // Two reasons, two repairs: another target for an unshipped
            // release, possibly another edition for a label it cannot place.
            Verdict::NotShipped => Some((
                *target,
                format!("is a {edition} release this compiler ships no block data for"),
            )),
            Verdict::NoSuchRelease => Some((*target, format!("names no {edition} release"))),
            Verdict::Buildable | Verdict::BelowFloor(_) => None,
        })
        .collect();
    if outside.is_empty() {
        return None;
    }
    let named = outside
        .iter()
        .map(|(target, _)| *target)
        .collect::<Vec<_>>()
        .join(", ");
    let mut notes: Vec<DiagnosticNote> = outside
        .iter()
        .map(|(target, reason)| DiagnosticNote {
            span: None,
            message: format!("`{target}` {reason}"),
        })
        .collect();
    notes.push(DiagnosticNote {
        span: None,
        message: format!("{edition} builds against {}", targetable.join(", ")),
    });
    // The wrong half may be the edition; picking from the list above would
    // give up the target the author meant.
    notes.push(DiagnosticNote {
        span: None,
        message: "fix: name a version this edition builds, or build for the edition that has \
                  the one you named"
            .to_owned(),
    });
    Some(Diagnostic {
        code: DiagnosticCode::IntendedTargetUnsupported,
        span: span.clone(),
        primary: format!("`@intended_targets` names {named}, which no {edition} target builds"),
        notes,
        data: Some(DiagnosticData::IntendedTargets {
            edition: edition.as_str().to_owned(),
            targets: outside
                .iter()
                .map(|(target, _)| (*target).to_owned())
                .collect(),
            floor: None,
        }),
    })
}

/// The closed set of versions that would satisfy every floor at once,
/// which `spec/lint` "Diagnostic codes" makes part of the message.
///
/// Every floor, because a version that clears the reported one and trips
/// the next is a second error.
fn candidates_note(
    edition: Edition,
    order: &VersionOrder,
    floors: &[VersionFloor],
    targetable: &[String],
) -> DiagnosticNote {
    let usable = versions_satisfying(order, floors, targetable);
    let message = if usable.is_empty() {
        format!(
            "no {edition} target satisfies the floors this file declares: {} builds against {}",
            edition,
            targetable.join(", "),
        )
    } else {
        format!("valid {edition} targets: {}", usable.join(", "))
    };
    DiagnosticNote {
        span: None,
        message,
    }
}

/// How a repair names the line a floor was written on: `@requires` on a
/// module header, `requires` inside a `def` or a `theme` body.
fn keyword(floor: &VersionFloor) -> &'static str {
    if floor.origin.part().is_some() {
        "`requires`"
    } else {
        "`@requires`"
    }
}
