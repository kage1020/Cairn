//! A pinned target held to the version floors its edition's build declares
//! (`spec/versioning-editions` "The declared floor is enforced").

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::edition::Edition;
use crate::resolve::{VersionFloor, versions_satisfying};
use crate::version::{FloorPlacement, FloorVerdict, VersionOrder};

/// The finding that refuses `target` for `floors`, given the versions the edition's pack builds
/// and each edition's version table, or `None` when every floor admits it.
///
/// # Panics
///
/// When `target` names no row of `edition`'s table.
#[must_use]
pub fn weigh_target(
    floors: &[VersionFloor],
    edition: Edition,
    target: &str,
    targetable: &[String],
    order_of: &dyn Fn(Edition) -> VersionOrder,
) -> Option<Diagnostic> {
    if floors.is_empty() {
        return None;
    }
    let order = order_of(edition);
    // No higher target meets a floor the table cannot place, so it is named ahead of any cap.
    if let Some(floor) = floors
        .iter()
        .find(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
    {
        return Some(requires_unorderable(floor, edition, &order, order_of));
    }
    let key = order
        .key_of(target)
        .unwrap_or_else(|| panic!("`{target}` is not a row of {edition}'s version table"));
    let floor = floors
        .iter()
        .find(|floor| order.verdict(&floor.version, key) == FloorVerdict::Below)?;
    Some(version_cap(
        floor,
        edition,
        target,
        versions_satisfying(&order, floors, targetable),
        targetable,
    ))
}

/// `E_VERSION_CAP`: `target` is below `floor`, and `valid` holds the targets every floor admits.
fn version_cap(
    floor: &VersionFloor,
    edition: Edition,
    target: &str,
    valid: Vec<&str>,
    targetable: &[String],
) -> Diagnostic {
    let mut notes: Vec<String> = floor.inheritance().into_iter().collect();
    let keyword = floor.keyword();
    if let Some(lowest) = valid.first() {
        notes.push(format!("valid {edition} targets: {}", valid.join(", ")));
        notes.push(format!(
            "fix: --target {lowest}, or lower the {keyword} floor"
        ));
    } else {
        // `latest` is a `--target` value too, so it is listed with the versions.
        notes.push(format!(
            "no supported {edition} target satisfies it: {}, latest",
            targetable.join(", "),
        ));
        notes.push(format!(
            "fix: lower the {keyword} floor, or build against another edition"
        ));
    }
    Diagnostic {
        code: DiagnosticCode::VersionCap,
        span: floor.span.clone(),
        primary: format!(
            "{} requires {} (target {target})",
            floor.declarer(),
            floor.rendered(),
        ),
        notes: spanless(notes),
        data: Some(DiagnosticData::VersionCap {
            edition: edition.as_str().to_owned(),
            target: target.to_owned(),
            floor: floor.rendered(),
            valid: valid.into_iter().map(str::to_owned).collect(),
        }),
    }
}

/// `E_REQUIRES_UNORDERABLE`: `floor` names no release `order` holds.
fn requires_unorderable(
    floor: &VersionFloor,
    edition: Edition,
    order: &VersionOrder,
    order_of: &dyn Fn(Edition) -> VersionOrder,
) -> Diagnostic {
    let mut notes: Vec<String> = floor.inheritance().into_iter().collect();
    notes.push(match order.neighbours(&floor.version) {
        (Some(below), Some(above)) => format!(
            "{edition} ships {below} and then {above}, and nothing between them; Cairn orders \
             versions by DataVersion and a label that names no release has none",
        ),
        _ => format!(
            "{edition} releases: {}",
            order
                .rows()
                .map(|(label, _)| label)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    });
    // An edition scope is offered only where the other edition names the label; elsewhere the
    // floor would go inert.
    let other = edition.other();
    if floor.edition.is_none() && order_of(other).place(&floor.version).names_release() {
        notes.push(format!(
            "`{}` is a {other} release; if that is the numbering this floor is written in, say so",
            floor.version,
        ));
        notes.push(format!(
            "fix: `@requires {other} version>={}`, or name a {edition} release",
            floor.version,
        ));
    } else {
        notes.push(format!("fix: name a {edition} release"));
    }
    Diagnostic {
        code: DiagnosticCode::RequiresUnorderable,
        span: floor.span.clone(),
        primary: format!(
            "{} requires {}, which names no {edition} release",
            floor.declarer(),
            floor.rendered(),
        ),
        notes: spanless(notes),
        data: None,
    }
}

fn spanless(messages: Vec<String>) -> Vec<DiagnosticNote> {
    messages
        .into_iter()
        .map(|message| DiagnosticNote {
            span: None,
            message,
        })
        .collect()
}
