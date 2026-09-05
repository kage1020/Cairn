//! `intended_targets` pass — weighs `@intended_targets` against the floors
//! the same file declares, and against the versions the edition can build.
//!
//! The two headers used to be two inert statements: neither reached
//! anything, so a file could say `@requires version>=1.21` on one line and
//! `@intended_targets ["1.20.4"]` on the next and be told nothing. Once
//! the floor became enforceable against `cairn compile --target`, the
//! arrangement got worse rather than better — one of the two declarations
//! decides a build and the other is ignored, and the ignored one is the
//! header that reads like an instruction. This pass is the comparison
//! between them.
//!
//! # Why it is not one of [`super::check`]'s passes
//!
//! Every question here is answered by the target edition's `DataVersion`
//! table (`spec/versioning-editions.md` §10.1), which lives in the
//! registry pack and reaches this crate only as a [`VersionOrder`] the
//! caller builds. `check` takes no pack and no order, so it cannot run
//! this pass; the CLI does, once per edition the command is about. That is
//! the same shape `E_THEME_VARIANT_MISSING` has — a finding only a pinned
//! edition can reach — and the alternative is worse: comparing two version
//! labels as text is the defect `VersionOrder` exists to remove, and a
//! comparison this file made by text would reach "1.21.40 satisfies
//! version>=1.21.4" exactly as the enforcement once did.
//!
//! # The three findings
//!
//! A target is weighed only when the edition can build it, because
//! "`--target 1.19` is not a version this compiler builds" answers before
//! any floor does and is the more actionable news. So each version the
//! header names is one of:
//!
//! - not a version the edition builds — `W_INTENDED_TARGET_UNSUPPORTED`,
//!   whether the table places it as a release the pack ships no block data
//!   for (`1.19` on Java) or cannot place it at all (`1.21.40` on Java,
//!   which is Bedrock's numbering);
//! - below a floor the file declares — the file states an intention the
//!   compiler refuses the moment anyone acts on it;
//! - buildable, at or above every floor, which is the ordinary case and
//!   raises nothing.
//!
//! The second splits by reach rather than by kind. Every version below a
//! floor is `E_INTENDED_TARGET_CAP`: the file can be built for nothing it
//! says it is for, which is the strongest reading a contradiction between
//! two declarations gets and the one an author cannot have meant. *Some*
//! of them is `W_INTENDED_TARGET_CAP`, because `spec/syntax.md` §5.3 calls
//! this header "a hint, not a verification record" — a list that reaches
//! past the floor at one end is a wish stated too widely, and the versions
//! it names above the floor still build.

use crate::ast::{Header, Module};
use crate::edition::Edition;
use crate::error::Span;
use crate::resolve::{
    FloorPlacement, FloorVerdict, VersionFloor, VersionOrder, compare_versions,
    declared_version_floors, versions_satisfying,
};

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};

/// Weigh every `@intended_targets` version a module declares against the
/// floors a build of `edition` is held to.
///
/// `order` is that edition's release table and `targetable` the versions
/// its registry pack can build for — the same two lists `cairn compile`
/// weighs `--target` against, so a version this pass calls buildable is
/// one the build accepts.
///
/// The findings are returned rather than pushed into a
/// [`super::DiagnosticSink`]: this pass runs outside [`super::check`] (see
/// the module doc), so the caller merges them into whatever stream it is
/// already reporting.
///
/// # Panics
///
/// If a `targetable` entry names no row of `order`. Both come from one
/// registry pack, where the buildable versions are a subset of the release
/// table by construction.
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
        // `@intended_targets []` states no intention, so there is nothing
        // for a floor to contradict. Reporting the empty list as "names no
        // buildable version" would be true and useless.
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
    /// A version the edition builds, below a floor the file declares —
    /// the first one in source order that refuses it, for the reason
    /// `E_VERSION_CAP` reports the first: an equivalent line appended
    /// below an existing one must not move the finding.
    BelowFloor(&'a VersionFloor),
    /// A release of this edition the registry pack ships no block data
    /// for, so `--target` cannot name it.
    NotShipped,
    /// Not a release of this edition at all: either a label its table
    /// cannot place — which is what a version written in the other
    /// edition's numbering looks like — or one outside the table's span.
    NoSuchRelease,
}

/// Place one intended version, then weigh it if the edition can build it.
///
/// The order is what keeps the two findings apart. A version the pack
/// cannot build is reported as that and not as a cap, however its label
/// sits against the floors: "`--target 1.19` does not exist here" is the
/// answer the author acts on first, and a cap reported beside it would
/// send them to raise a floor that is not what stops the build.
fn weigh<'a>(
    label: &str,
    floors: &'a [VersionFloor],
    order: &VersionOrder,
    targetable: &[String],
) -> Verdict<'a> {
    let Some(row) = targetable
        .iter()
        .find(|row| compare_versions(row, label).is_eq())
    else {
        // Placed rather than merely absent from the buildable list: a
        // release the pack can order and one it has never heard of are
        // different news to the author, and only the first has a version
        // of this edition behind it.
        return match order.place(label) {
            FloorPlacement::At(_) => Verdict::NotShipped,
            FloorPlacement::BelowEvery
            | FloorPlacement::AboveEvery
            | FloorPlacement::Unplaceable => Verdict::NoSuchRelease,
        };
    };
    let key = order
        .key_of(row)
        .expect("`targetable` is this pack's own version list");
    floors
        .iter()
        .find(|floor| order.verdict(&floor.version, key) == FloorVerdict::Below)
        .map_or(Verdict::Buildable, Verdict::BelowFloor)
}

/// The cap finding, when any intended version is below a floor.
///
/// One finding for the header rather than one per version: the header is
/// one declaration and the repair is one edit to it, and a list of five
/// versions below one floor is not five things to fix. Which versions, and
/// which floor put each of them out, is what the notes carry.
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
    // The first refusing floor in source order, which is the one the fix
    // line and the payload speak about — the same choice `E_VERSION_CAP`
    // makes, so an equivalent line appended below an existing one does not
    // move what the message names.
    let (_, first_floor) = *below.first()?;
    let every = below.len() == weighed.len();
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
            // Two reasons because they are two different repairs. A
            // release the pack does not ship is a version of *this*
            // edition and the answer is another target; a label it cannot
            // place is usually the other edition's numbering, and the
            // answer may be another edition.
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
    // Both halves of the repair, because the header is a statement of
    // intent and the wrong half of it may be the edition: a version this
    // edition cannot place is usually the other edition's numbering, and
    // telling its author to pick from the list above would be telling them
    // to give up the target they meant.
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

/// The closed set of versions that would satisfy every floor at once.
///
/// Every floor, not the one being reported: a version that clears this
/// floor and trips the next one is a second error in a different spelling,
/// and `spec/lint.md` §11.1 makes the valid candidates part of the message
/// rather than an extra.
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

/// How to name the line a floor was written on, as a repair points at it.
///
/// The two spellings are `@requires` on a module header and `requires`
/// inside a `def` or a `theme` body; a fix line naming the wrong one sends
/// its reader looking for a line the file does not contain.
fn keyword(floor: &VersionFloor) -> &'static str {
    if floor.origin.part().is_some() {
        "`requires`"
    } else {
        "`@requires`"
    }
}
