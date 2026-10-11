//! A `--target` resolved against its edition's data table, and the
//! backend it selects.

use cairn_lang_core::block_array::BlockArray;
use cairn_lang_formats::bedrock_structure::{ParityNote, prepare_mcstructure};
use cairn_lang_formats::data_version::{
    BedrockTarget, JavaTarget, resolve_bedrock_target, resolve_java_target,
};
use cairn_lang_formats::java_structure::{OutputExt, prepare_structure};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

use crate::artifacts::PreparedStructure;
use crate::cli::EditionArg;

/// A `--target` resolved for one edition, which also selects the backend
/// that serialises it.
///
/// Every edition-varying step of a compile matches on this type or on the
/// [`PreparedStructure`] it produces, so adding an edition means adding a
/// variant to both and the compiler flags every arm left to write.
pub(crate) enum ResolvedTarget {
    /// Java vanilla structure target (`.nbt`, gzip).
    Java(JavaTarget),
    /// Bedrock structure target (`.mcstructure`, uncompressed).
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// On-disk extension the backend writes.
    pub(crate) fn output_ext(&self) -> OutputExt {
        match self {
            ResolvedTarget::Java(_) => OutputExt::Nbt,
            ResolvedTarget::Bedrock(_) => OutputExt::Mcstructure,
        }
    }

    /// Check a structure against this edition's backend, with the
    /// `W_INTENT_DEGRADED` parity notes lowering intent to the edition
    /// raised (none on Java, which is lossless). Both backends' errors are
    /// rendered to a string so the caller has one shape to report. Nothing
    /// per voxel is built: [`PreparedStructure::write`] encodes the grid as
    /// it writes.
    ///
    /// The notes stay [`ParityNote`]s so the warning line can print the
    /// degraded block id as a field of its own. One id has a note per state
    /// combination that degrades, and only the message, which names the
    /// entry as `id[states]`, tells them apart.
    pub(crate) fn prepare<'a>(
        &self,
        array: &'a BlockArray,
    ) -> Result<(PreparedStructure<'a>, Vec<ParityNote>), String> {
        match self {
            ResolvedTarget::Java(t) => prepare_structure(array, t)
                .map(|prepared| (PreparedStructure::Java(prepared), Vec::new()))
                .map_err(|e| e.to_string()),
            ResolvedTarget::Bedrock(t) => prepare_mcstructure(array, t)
                .map(|(prepared, notes)| (PreparedStructure::Bedrock(prepared), notes))
                .map_err(|e| e.to_string()),
        }
    }

    /// Human-facing Minecraft version string for the lockfile.
    pub(crate) fn mc_version(&self) -> &str {
        match self {
            ResolvedTarget::Java(t) => &t.mc_version,
            ResolvedTarget::Bedrock(t) => &t.mc_version,
        }
    }

    /// Edition-specific version integer for the lockfile (`DataVersion`
    /// for Java, block-palette `version` for Bedrock).
    pub(crate) fn version_int(&self) -> i32 {
        match self {
            ResolvedTarget::Java(t) => t.data_version,
            ResolvedTarget::Bedrock(t) => t.block_version,
        }
    }

    /// Registry pack whose bytes the compile resolved against, hashed into
    /// the lockfile.
    pub(crate) fn registry_pack(&self) -> &'static RegistryPack {
        match self {
            ResolvedTarget::Java(_) => builtin_java(),
            ResolvedTarget::Bedrock(_) => builtin_bedrock(),
        }
    }
}

/// Resolve `--target` against the pack for `--edition`.
///
/// Returns the failure instead of printing it: `run_compile` resolves the
/// target before lowering, which checks block ids against it, but reports
/// a bad target after the parse and lowering diagnostics, so a syntax
/// error is still the first problem a user is told about.
pub(crate) fn resolve_target(
    edition: EditionArg,
    target: &str,
) -> Result<ResolvedTarget, cairn_lang_formats::data_version::UnsupportedTarget> {
    match edition {
        EditionArg::Java => resolve_java_target(target).map(ResolvedTarget::Java),
        EditionArg::Bedrock => resolve_bedrock_target(target).map(ResolvedTarget::Bedrock),
    }
}
