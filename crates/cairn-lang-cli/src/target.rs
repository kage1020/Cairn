//! A `--target` resolved against its edition's data table, and the backend it selects.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArray;
use cairn_lang_formats::bedrock_structure::{ParityNote, prepare_mcstructure};
use cairn_lang_formats::data_version::{
    BedrockTarget, JavaTarget, resolve_bedrock_target, resolve_java_target,
};
use cairn_lang_formats::java_structure::{OutputExt, prepare_structure};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::artifacts::PreparedStructure;

/// A `--target` resolved for one edition, which also selects the backend that writes it.
pub(crate) enum ResolvedTarget {
    Java(JavaTarget),
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    pub(crate) fn output_ext(&self) -> OutputExt {
        match self {
            ResolvedTarget::Java(_) => OutputExt::Nbt,
            ResolvedTarget::Bedrock(_) => OutputExt::Mcstructure,
        }
    }

    /// Check `array` against this edition's backend, with the parity notes it raises.
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

    pub(crate) fn mc_version(&self) -> &str {
        match self {
            ResolvedTarget::Java(t) => &t.mc_version,
            ResolvedTarget::Bedrock(t) => &t.mc_version,
        }
    }

    /// `DataVersion` on Java, the block-palette `version` on Bedrock.
    pub(crate) fn version_int(&self) -> i32 {
        match self {
            ResolvedTarget::Java(t) => t.data_version,
            ResolvedTarget::Bedrock(t) => t.block_version,
        }
    }

    pub(crate) fn registry_pack(&self) -> &'static RegistryPack {
        match self {
            ResolvedTarget::Java(_) => builtin_pack(Edition::Java),
            ResolvedTarget::Bedrock(_) => builtin_pack(Edition::Bedrock),
        }
    }
}

pub(crate) fn resolve_target(
    edition: Edition,
    target: &str,
) -> Result<ResolvedTarget, cairn_lang_formats::data_version::UnsupportedTarget> {
    match edition {
        Edition::Java => resolve_java_target(target).map(ResolvedTarget::Java),
        Edition::Bedrock => resolve_bedrock_target(target).map(ResolvedTarget::Bedrock),
    }
}
