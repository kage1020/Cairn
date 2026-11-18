//! The version floors a build is held to: the refusal of a target a floor rules out.

use cairn_lang_core::check::weigh_target;
use cairn_lang_core::resolve::VersionFloor;
use cairn_lang_core::{Diagnostic, Edition};

use crate::data_version::ResolvedTarget;
use crate::registry::builtin_version_tables;

/// The finding that refuses `target` for `floors`, the floors `edition`'s build is held to, or
/// `None` when every floor admits it.
#[must_use]
pub fn floor_refusal(
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
