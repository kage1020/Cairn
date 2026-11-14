//! A `--target` resolved against its edition's data table, and the backend it selects.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArray;

use crate::bedrock_structure::{McStructure, ParityNote, prepare_mcstructure};
use crate::data_version::{
    BedrockTarget, JavaTarget, UnsupportedTarget, resolve_bedrock_target, resolve_java_target,
};
use crate::java_structure::{JavaStructure, OutputExt, prepare_structure};
use crate::registry::{RegistryPack, builtin_pack};

/// A `--target` resolved for one edition, which also selects the backend that writes it.
#[derive(Debug, Clone)]
pub enum ResolvedTarget {
    /// A row of Java's version table.
    Java(JavaTarget),
    /// A row of Bedrock's version table.
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// The extension of the structure files this target's backend writes.
    #[must_use]
    pub fn output_ext(&self) -> OutputExt {
        match self {
            ResolvedTarget::Java(_) => OutputExt::Nbt,
            ResolvedTarget::Bedrock(_) => OutputExt::Mcstructure,
        }
    }

    /// Check `array` against this edition's backend, with the parity notes it raises.
    ///
    /// # Errors
    ///
    /// The backend's refusal, as the message a front end prints after the scope.
    ///
    /// # Panics
    ///
    /// If the built-in Bedrock pack has no state map for a version it can build for.
    pub fn prepare<'a>(
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

    /// The version's label in its edition's table.
    #[must_use]
    pub fn mc_version(&self) -> &str {
        match self {
            ResolvedTarget::Java(t) => &t.mc_version,
            ResolvedTarget::Bedrock(t) => &t.mc_version,
        }
    }

    /// `DataVersion` on Java, the block-palette `version` on Bedrock: the integer the artifact
    /// and its lockfile carry.
    #[must_use]
    pub fn version_int(&self) -> i32 {
        match self {
            ResolvedTarget::Java(t) => t.data_version,
            ResolvedTarget::Bedrock(t) => t.block_version,
        }
    }

    /// The row's key in its edition's version order, which a floor is weighed against; two
    /// Bedrock releases can share a block-palette `version` and never a key.
    #[must_use]
    pub fn order_key(&self) -> i64 {
        i64::from(match self {
            ResolvedTarget::Java(t) => t.data_version,
            ResolvedTarget::Bedrock(t) => t.order_key,
        })
    }

    /// The built-in pack of this target's edition.
    #[must_use]
    pub fn registry_pack(&self) -> &'static RegistryPack {
        match self {
            ResolvedTarget::Java(_) => builtin_pack(Edition::Java),
            ResolvedTarget::Bedrock(_) => builtin_pack(Edition::Bedrock),
        }
    }
}

/// Resolve `target` against `edition`'s version table.
///
/// # Errors
///
/// [`UnsupportedTarget`] when the table has no such version.
pub fn resolve_target(edition: Edition, target: &str) -> Result<ResolvedTarget, UnsupportedTarget> {
    match edition {
        Edition::Java => resolve_java_target(target).map(ResolvedTarget::Java),
        Edition::Bedrock => resolve_bedrock_target(target).map(ResolvedTarget::Bedrock),
    }
}

/// Every version a pack can build for, in ascending release order.
#[must_use]
pub fn supported_versions(pack: &RegistryPack) -> Vec<String> {
    pack.targetable()
        .into_iter()
        .map(|entry| entry.mc_version.clone())
        .collect()
}

/// A structure its edition's backend has checked, waiting to be written.
pub enum PreparedStructure<'a> {
    /// A gzip `.nbt` structure.
    Java(JavaStructure<'a>),
    /// An uncompressed `.mcstructure`.
    Bedrock(McStructure<'a>),
}

impl PreparedStructure<'_> {
    /// Write the structure file's bytes to `writer`.
    ///
    /// # Errors
    ///
    /// Whatever `writer` refuses, or an NBT encoding failure.
    pub fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        let encoded = match self {
            PreparedStructure::Java(structure) => structure.write_gzip(writer),
            PreparedStructure::Bedrock(structure) => structure.write(writer),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }
}
