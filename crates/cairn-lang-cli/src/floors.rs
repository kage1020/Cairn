//! The notes `cairn info` prints about the `@requires` floors under its rows.

use std::collections::HashSet;
use std::path::Path;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{VersionFloor, declared_version_floors, unscoped_version_floors};
use cairn_lang_core::{Edition, Module};
use cairn_lang_formats::build::floors::{VersionVerdicts, floor_origin_note};

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
    verdicts: &VersionVerdicts,
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
            eprintln!("  {note}");
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
