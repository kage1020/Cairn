//! A lowered structure checked against its target's backend and named, waiting to be written.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};

use super::run_level::{RunLevelMessage, backend_refusal, degraded};
use crate::java_structure::output_filename;
use crate::target::{PreparedStructure, ResolvedTarget};

/// A structure its target's backend has checked, under the file name a build gives it.
pub struct PreparedArtifact<'a> {
    /// The structure file's name, its extension the target's.
    pub file_name: String,
    /// The checked structure.
    pub structure: PreparedStructure<'a>,
    /// One `W_INTENT_DEGRADED` per intent the target cannot carry.
    pub degraded: Vec<RunLevelMessage>,
}

/// Check the structure `scope` of `block_ir` against `target`'s backend and name its file.
///
/// # Errors
///
/// The backend's refusal of the structure.
pub fn prepare_artifact<'a>(
    block_ir: &BlockArrayIr,
    scope: &str,
    array: &'a BlockArray,
    target: &ResolvedTarget,
) -> Result<PreparedArtifact<'a>, RunLevelMessage> {
    let (structure, notes) = target
        .prepare(array)
        .map_err(|reason| backend_refusal(scope, &reason))?;
    Ok(PreparedArtifact {
        file_name: output_filename(&block_ir.artifact_stem(scope), target.output_ext()),
        structure,
        degraded: notes.iter().map(|note| degraded(scope, note)).collect(),
    })
}
