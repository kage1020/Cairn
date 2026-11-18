//! The version floors a source declares: how a floor's refusal of the target reads on stderr.

use std::path::Path;

use cairn_lang_core::Diagnostic;
use cairn_lang_core::check::LineStarts;

/// Report a floor's refusal of the build's target on stderr, the way a run-level refusal reads.
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
