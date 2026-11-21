//! The backend a resolved `--target` selects, and the structure file it prepares.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArray;
use thiserror::Error;

use crate::bedrock_structure::{
    BedrockStructureError, McStructure, ParityNote, prepare_mcstructure,
};
use crate::data_version::ResolvedTarget;
use crate::java_structure::{JavaStructure, JavaStructureError, prepare_structure};
use crate::litematic::Litematic;
use crate::registry::builtin_pack;
use crate::schem::Schem;

/// Why the backend a resolved target selects refuses a structure.
#[derive(Debug, Error)]
pub enum BackendRefusal {
    /// The `.nbt` writer's refusal.
    #[error(transparent)]
    Java(#[from] JavaStructureError),
    /// The `.mcstructure` writer's refusal.
    #[error(transparent)]
    Bedrock(#[from] BedrockStructureError),
}

/// Check `array` against `target`'s backend, with the parity notes it raises.
///
/// # Errors
///
/// The backend's refusal of the structure.
///
/// # Panics
///
/// If the built-in Bedrock pack has no state map for a version it can build for.
pub(crate) fn prepare<'a>(
    target: &ResolvedTarget,
    array: &'a BlockArray,
) -> Result<(PreparedStructure<'a>, Vec<ParityNote>), BackendRefusal> {
    match target {
        ResolvedTarget::Java(t) => Ok((
            PreparedStructure::Java(prepare_structure(array, t)?),
            Vec::new(),
        )),
        ResolvedTarget::Bedrock(t) => {
            let states = builtin_pack(Edition::Bedrock)
                .states_at(&t.mc_version)
                .expect("the built-in Bedrock pack maps every targetable version");
            let (prepared, notes) = prepare_mcstructure(array, t, &states)?;
            Ok((PreparedStructure::Bedrock(prepared), notes))
        }
    }
}

/// A structure file its backend has checked, waiting to be written.
pub enum PreparedStructure<'a> {
    /// A gzip `.nbt` structure.
    Java(JavaStructure<'a>),
    /// An uncompressed `.mcstructure`.
    Bedrock(McStructure<'a>),
    /// A gzip `.litematic` schematic of a struct or a site.
    Litematic(Litematic<'a>),
    /// A gzip `.schem` schematic of a struct, a placement or a walkway.
    Schem(Schem<'a>),
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
            PreparedStructure::Litematic(schematic) => schematic.write_gzip(writer),
            PreparedStructure::Schem(schematic) => schematic.write_gzip(writer),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }
}
