//! The backend a resolved `--target` selects, and the structure file it prepares.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArray;
use cairn_lang_formats::bedrock_structure::{McStructure, ParityNote, prepare_mcstructure};
use cairn_lang_formats::data_version::JavaTarget;
use cairn_lang_formats::java_structure::{JavaStructure, prepare_structure};
use cairn_lang_formats::litematic::Litematic;
use cairn_lang_formats::registry::{PackView, builtin_constraints, builtin_pack};
use cairn_lang_formats::schem::Schem;

use crate::cli::StructureFormat;

pub(crate) use cairn_lang_formats::data_version::{ResolvedTarget, resolve_target};

/// Check `array` against `target`'s backend, with the parity notes it raises.
pub(crate) fn prepare<'a>(
    target: &ResolvedTarget,
    array: &'a BlockArray,
) -> Result<(PreparedStructure<'a>, Vec<ParityNote>), String> {
    match target {
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

/// The built-in pack's view of `edition` at `pinned`, which runs the constraint catalog's checks
/// only with a version pinned (`spec/lint` "Constraint checks").
pub(crate) fn registry_view(edition: Edition, pinned: Option<&str>) -> PackView<'_> {
    let view = builtin_pack(edition).view(pinned);
    match pinned {
        Some(_) => view.with_constraints(builtin_constraints()),
        None => view,
    }
}

/// What a build's artifacts are checked against and written by: its target with, on Java, the
/// file format.
#[derive(Clone, Copy)]
pub(crate) enum Backend<'t> {
    /// A structure file per array, in the target's edition's own format.
    PerArray(&'t ResolvedTarget),
    /// A `.litematic` per struct and per site.
    Litematic(&'t JavaTarget),
    /// A `.schem` per array.
    Schem(&'t JavaTarget),
}

impl<'t> Backend<'t> {
    /// The backend `target` writes through, `format` picking Java's; Bedrock has one format, and
    /// `cairn compile` refuses `--structure-format` beside it.
    pub(crate) fn new(target: &'t ResolvedTarget, format: StructureFormat) -> Self {
        match (target, format) {
            (ResolvedTarget::Java(java), StructureFormat::Litematic) => Self::Litematic(java),
            (ResolvedTarget::Java(java), StructureFormat::Schem) => Self::Schem(java),
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
    Schem(Schem<'a>),
}

impl PreparedStructure<'_> {
    pub(crate) fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        let encoded = match self {
            PreparedStructure::Java(structure) => structure.write_gzip(writer),
            PreparedStructure::Bedrock(structure) => structure.write(writer),
            PreparedStructure::Litematic(schematic) => schematic.write_gzip(writer),
            PreparedStructure::Schem(schematic) => schematic.write_gzip(writer),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }
}
