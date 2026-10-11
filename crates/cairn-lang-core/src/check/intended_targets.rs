//! The `intended_targets` pass: `@intended_targets` weighed against the file's version floors and
//! the versions the edition builds. It runs outside [`super::check`] because it needs the
//! edition's [`VersionOrder`]
//! (`spec/versioning-editions` "The target is a compile-time parameter").

use crate::ast::{Header, Module};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::resolve::{VersionFloor, declared_version_floors, versions_satisfying};
use crate::version::{FloorVerdict, VersionOrder, compare_versions};

/// The findings for each `@intended_targets` version weighed against `edition`'s floors.
///
/// `targetable` lists the versions the registry pack builds; one `order` cannot place is dropped.
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
    /// Built by the edition, at or above every floor.
    Buildable,
    /// Built by the edition, below this floor: the first in source order to refuse it.
    BelowFloor(&'a VersionFloor),
    /// A release of this edition the registry pack ships no block data for.
    NotShipped,
    /// Not a release of this edition, such as the other edition's numbering.
    NoSuchRelease,
}

/// Only a buildable version meets the floors: a floor is not what stops an unbuildable one.
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
    // Buildable versions only: an unbuildable name must not soften the error to a warning.
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

/// The versions that satisfy every floor at once (`spec/lint` "Diagnostic codes").
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

fn keyword(floor: &VersionFloor) -> &'static str {
    if floor.origin.part().is_some() {
        "`requires`"
    } else {
        "`@requires`"
    }
}
