//! `cairn info`: a source's version axes, with one portability and buildable-targets row per
//! requested edition.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::lock::LockReading;
use cairn_lang_core::resolve::{BuildableTargets, InferredRange, VersionAxes};
use cairn_lang_formats::info::{InfoRequest, default_lock_path, info, requested_editions};

use crate::cli::InfoFormat;
use crate::lockfile::describe_verified;
use crate::pipeline::read_source;
use crate::report::{Stderr, print_json, report_failure_document};

pub(crate) fn run_info(
    file: &Path,
    editions: &[String],
    lock: Option<&Path>,
    format: InfoFormat,
) -> ExitCode {
    let editions = match requested_editions(editions) {
        Ok(editions) => editions,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    let source = match read_source(file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let lock = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    let lines = LineStarts::new(&source);
    let request = InfoRequest {
        file,
        source: &source,
        editions: &editions,
        lock: &lock,
        errors_in_document: matches!(format, InfoFormat::Json),
    };
    let mut stderr = Stderr {
        file,
        source: &source,
        lines: &lines,
    };
    let document = match info(&request, &mut stderr) {
        Ok(document) => document,
        Err(refused) => {
            return report_failure_document(file, &source, format.failure_report(), &refused);
        }
    };
    match format {
        InfoFormat::Text => {
            print_axes_report(&document.axes);
            outln!(
                "verified target:         {}",
                verified_target_text(&document.verified_target.reading)
            );
            ExitCode::SUCCESS
        }
        InfoFormat::Json => match print_json("version axes", &document) {
            Ok(()) => ExitCode::SUCCESS,
            Err(code) => code,
        },
    }
}

/// The `verified target` row's text after its label.
fn verified_target_text(reading: &LockReading) -> String {
    match reading {
        LockReading::Missing => "(no lock)".to_owned(),
        LockReading::Read {
            target,
            verified,
            source_unchanged,
            cairn_version,
            ..
        } => format!(
            "{}{}, source {}, written by Cairn {cairn_version}",
            describe_verified(target, true),
            if *verified { "" } else { " (not verified)" },
            if *source_unchanged {
                "unchanged"
            } else {
                "changed"
            },
        ),
        LockReading::NewerSchema {
            lock_schema_version,
            ..
        } => format!("(lock schema {lock_schema_version}, newer than this Cairn reads)"),
        LockReading::Unreadable { .. } => "(unreadable lock)".to_owned(),
    }
}

fn joined_or(empty: &str, separator: &str, items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = items.into_iter().collect();
    if items.is_empty() {
        empty.to_owned()
    } else {
        items.join(separator)
    }
}

fn print_axes_report(axes: &VersionAxes) {
    // One range for every edition: `spec/versioning-editions` "The `registry compatibility` row".
    outln!(
        "registry compatibility:  {} .. {}",
        axes.registry_compat.min,
        axes.registry_compat.max,
    );

    let inferred_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.inferred_range.iter().map(|range| {
            format!(
                "{}: {}",
                capitalise(range.edition.as_str()),
                inferred_text(range)
            )
        }),
    );
    outln!("inferred range:          {inferred_line}");

    let portability_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.edition_portability.iter().map(|ep| {
            format!(
                "{}: portable: {}  degraded: {}  unsupported: {}",
                capitalise(ep.edition.as_str()),
                ep.portable,
                ep.degraded,
                ep.unsupported,
            )
        }),
    );
    outln!("edition portability:     {portability_line}");

    let respelled_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.respelled.iter().map(|row| {
            let mut ids: Vec<&str> = Vec::new();
            for token in &row.tokens {
                if !ids.contains(&token.id.as_str()) {
                    ids.push(&token.id);
                }
            }
            let ids = if ids.is_empty() {
                "none".to_owned()
            } else {
                ids.join(", ")
            };
            format!("{}: {ids}", capitalise(row.edition.as_str()))
        }),
    );
    outln!("respelled:               {respelled_line}");

    let buildable_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.buildable_targets.iter().map(|bt| {
            format!(
                "{}: {}",
                capitalise(bt.edition.as_str()),
                buildable_text(bt)
            )
        }),
    );
    outln!("buildable targets:       {buildable_line}");

    let intended_line = joined_or(
        "(none declared)",
        ", ",
        axes.intended_targets.iter().cloned(),
    );
    outln!("intended targets:        {intended_line}");

    let semantic_line = joined_or(
        "(none)",
        ", ",
        axes.semantic_sensitive
            .iter()
            .map(|f| format!("{}({} @{})", f.member, f.reason, f.boundary_version)),
    );
    outln!("semantic-sensitive:      {semantic_line}");
}

/// One edition's inferred range as its edges, or `none` when no version can lie in it.
fn inferred_text(range: &InferredRange) -> String {
    if range.empty {
        "none".to_owned()
    } else {
        format!("{} .. {}", range.min, range.max)
    }
}

/// One edition's buildable versions, with the refusing ones named after them.
fn buildable_text(targets: &BuildableTargets) -> String {
    let refused: Vec<&str> = targets
        .considered
        .iter()
        .filter(|version| !targets.buildable.contains(version))
        .map(String::as_str)
        .collect();
    let built = if targets.buildable.is_empty() {
        String::from("none")
    } else {
        targets.buildable.join(", ")
    };
    if refused.is_empty() {
        return built;
    }
    let tail = if let [only] = refused.as_slice() {
        format!("{only} refuses")
    } else if targets.buildable.is_empty() {
        format!("{} all refuse", refused.join(", "))
    } else {
        format!("{} refuse", refused.join(", "))
    };
    format!("{built} ({tail})")
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
