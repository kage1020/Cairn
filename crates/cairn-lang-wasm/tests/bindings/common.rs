//! Sources and readers the binding tests share.

use std::path::Path;

use cairn_lang_core::check::RenderedDiagnostic;

/// A struct with one painted floor, so a refusal cannot be mistaken for an empty build.
pub const BUILD: &str =
    "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// The source of `examples/<name>.crn`.
pub fn example(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(format!("{name}.crn"));
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Each finding's code and the line and column it starts at.
pub fn codes(diagnostics: &[RenderedDiagnostic]) -> Vec<(&'static str, u32, u32)> {
    diagnostics
        .iter()
        .map(|d| (d.code, d.line, d.col))
        .collect()
}
