//! The findings a command must report before it builds anything: `check`'s, `@intended_targets`
//! weighed per edition, then the lowering's.

use cairn_lang_core::ast::Module;
use cairn_lang_core::check::check_with_intended_targets;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::{Diagnostic, Edition};

use crate::registry::builtin_version_tables;

/// Every diagnostic a command must report, in the order the passes ran: `check`'s, which include
/// the resolver's, with `@intended_targets` weighed in each edition of `asked`, then `lowering`'s.
#[must_use]
pub fn build_diagnostics(
    module: &Module,
    ir: &IntentModule,
    edition: Option<Edition>,
    asked: &[Edition],
    lowering: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let mut combined =
        check_with_intended_targets(module, ir, edition, asked, &builtin_version_tables());
    combined.extend(lowering);
    combined
}
