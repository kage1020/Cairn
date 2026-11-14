//! The `@requires` floors: the compile's refusal of a target a floor rules out.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{VersionFloor, VersionOrder, versions_satisfying};
use cairn_lang_formats::build::{BuildTarget, DeclaredFloors, FloorRefusal};
use cairn_lang_formats::info::{floor_keyword, floor_origin_note, supported_versions};
use cairn_lang_formats::registry::builtin_pack;

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
    target: BuildTarget<'_>,
) -> Result<(), ExitCode> {
    let DeclaredFloors { floors, order } = declared;
    let floor = match declared.refusing(target.order_key) {
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
        target.mc_version,
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
