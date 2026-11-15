//! A `--target` resolved against its edition's data table, and the backend it selects.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArray;
use cairn_lang_formats::bedrock_structure::{McStructure, ParityNote, prepare_mcstructure};
use cairn_lang_formats::data_version::{
    BedrockTarget, JavaTarget, resolve_bedrock_target, resolve_java_target,
};
use cairn_lang_formats::java_structure::{JavaStructure, OutputExt, prepare_structure};
use cairn_lang_formats::litematic::Litematic;
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::cli::StructureFormat;

/// A `--target` resolved for one edition, which also selects the backend that writes it.
pub(crate) enum ResolvedTarget {
    Java(JavaTarget),
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// The extension of one array's structure file in this edition's own format.
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
            ResolvedTarget::Bedrock(t) => {
                let states = builtin_pack(Edition::Bedrock)
                    .states_at(&t.mc_version)
                    .expect("the built-in Bedrock pack maps every targetable version");
                prepare_mcstructure(array, t, &states)
                    .map(|(prepared, notes)| (PreparedStructure::Bedrock(prepared), notes))
                    .map_err(|e| e.to_string())
            }
        }
    }

    pub(crate) fn mc_version(&self) -> &str {
        match self {
            ResolvedTarget::Java(t) => &t.mc_version,
            ResolvedTarget::Bedrock(t) => &t.mc_version,
        }
    }

    /// `DataVersion` on Java, the block-palette `version` on Bedrock: the integer the artifact
    /// and its lockfile carry.
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

/// Every version a pack can build for, in ascending release order.
pub(crate) fn supported_versions(pack: &RegistryPack) -> Vec<String> {
    pack.targetable()
        .into_iter()
        .map(|entry| entry.mc_version.clone())
        .collect()
}

/// What a build's artifacts are checked against and written by: its target with, on Java, the
/// file format.
#[derive(Clone, Copy)]
pub(crate) enum Backend<'t> {
    /// A structure file per array, in the target's edition's own format.
    PerArray(&'t ResolvedTarget),
    /// A `.litematic` per struct and per site.
    Litematic(&'t JavaTarget),
}

impl<'t> Backend<'t> {
    /// The backend `target` writes through, `format` picking Java's; Bedrock has one format, and
    /// `cairn compile` refuses `--structure-format` beside it.
    pub(crate) fn new(target: &'t ResolvedTarget, format: StructureFormat) -> Self {
        match (target, format) {
            (ResolvedTarget::Java(java), StructureFormat::Litematic) => Self::Litematic(java),
            (ResolvedTarget::Java(_), StructureFormat::Nbt) | (ResolvedTarget::Bedrock(_), _) => {
                Self::PerArray(target)
            }
        }
    }
}

/// A structure file its backend has checked, waiting to be written.
pub(crate) enum PreparedStructure<'a> {
    Java(JavaStructure<'a>),
    Bedrock(McStructure<'a>),
    Litematic(Litematic<'a>),
}

impl PreparedStructure<'_> {
    pub(crate) fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        let encoded = match self {
            PreparedStructure::Java(structure) => structure.write_gzip(writer),
            PreparedStructure::Bedrock(structure) => structure.write(writer),
            PreparedStructure::Litematic(schematic) => schematic.write_gzip(writer),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }
}
