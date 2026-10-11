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

/// A resolved compile target: an edition-specific version-integer wrapper
/// plus the knowledge of which backend serialises it. Every downstream
/// step (filename extension, tag builder, writer, lockfile row) branches
/// on this one value so a new edition is added in a single place.
pub(crate) enum ResolvedTarget {
    /// Java vanilla structure target (`.nbt`, gzip).
    Java(JavaTarget),
    /// Bedrock structure target (`.mcstructure`, uncompressed).
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// On-disk extension the backend writes. The three edition-varying
    /// steps of a compile — extension, backend check ([`Self::prepare`]),
    /// and writer ([`PreparedStructure::write`]) — are matched on this type
    /// or on the [`PreparedStructure`] it produces, so their correspondence
    /// is co-located rather than kept in step by convention across
    /// scattered `match`es. Adding an edition means adding a variant to
    /// both types and the arms that match them, and the compiler flags any
    /// arm it misses.
    pub(crate) fn output_ext(&self) -> OutputExt {
        match self {
            ResolvedTarget::Java(_) => OutputExt::Nbt,
            ResolvedTarget::Bedrock(_) => OutputExt::Mcstructure,
        }
    }

    /// Check a structure against this edition's backend, plus any
    /// `W_INTENT_DEGRADED` parity notes raised while lowering intent to the
    /// edition (Java is always lossless, so its note list is empty). The two
    /// backends raise different error types; both are rendered to a message
    /// string here so the caller has one error shape to report. Nothing per
    /// voxel is built: [`PreparedStructure::write`] encodes the grid as it
    /// writes.
    ///
    /// [`ParityNote`] is threaded verbatim rather than flattened to a message
    /// string so the CLI can print the block id that degraded as a field of
    /// its own on the warning line. That field groups the warnings by
    /// block, not by entry: one id has a note per state combination that
    /// degrades, and only the message, which names the entry as
    /// `id[states]`, tells those apart. A tool that wants the entries
    /// structured reads `cairn info`'s
    /// `edition_portability[].degraded_entries` instead.
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
/// target before it lowers (block-array lowering needs the pinned version
/// to check block ids against) but reports the failure where it always
/// did, after the parse and lowering diagnostics. A command-line mistake
/// that jumped ahead of a syntax error would change which problem a user
/// is told about first.
pub(crate) fn resolve_target(
    edition: EditionArg,
    target: &str,
) -> Result<ResolvedTarget, cairn_lang_formats::data_version::UnsupportedTarget> {
    match edition {
        EditionArg::Java => resolve_java_target(target).map(ResolvedTarget::Java),
        EditionArg::Bedrock => resolve_bedrock_target(target).map(ResolvedTarget::Bedrock),
    }
}
