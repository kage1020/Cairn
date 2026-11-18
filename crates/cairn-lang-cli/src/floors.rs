//! The version floors a source declares: the compile's refusal of a target a floor rules out.

use std::path::Path;

use cairn_lang_core::check::{LineStarts, weigh_target};
use cairn_lang_core::resolve::VersionFloor;
use cairn_lang_core::{Diagnostic, Edition};
use cairn_lang_formats::registry::builtin_version_tables;

use crate::target::ResolvedTarget;

/// The finding that refuses `target` for `floors`, the floors `edition`'s build is held to, or
/// `None` when every floor admits it.
pub(crate) fn floor_refusal(
    floors: &[VersionFloor],
    edition: Edition,
    target: &ResolvedTarget,
) -> Option<Diagnostic> {
    weigh_target(
        floors,
        edition,
        target.mc_version(),
        &builtin_version_tables(),
        &|version| format!("--target {version}"),
    )
}

/// Report a floor's refusal of `--target` on stderr, the way a run-level refusal reads.
pub(crate) fn report_floor_refusal(file: &Path, source: &str, refusal: &Diagnostic) {
    eprintln!(
        "{}[{}]: {}:{}: {}.",
        refusal.severity().as_str(),
        refusal.code.as_str(),
        file.display(),
        LineStarts::new(source).position(source, refusal.span.start),
        refusal.primary,
    );
    for note in &refusal.notes {
        eprintln!("  {}", note.message);
    }
}
