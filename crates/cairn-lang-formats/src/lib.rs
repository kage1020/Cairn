//! Readers and writers around the Cairn block-array IR for existing schematic formats.
//!
//! Supported: vanilla Java `.nbt` structures and Bedrock `.mcstructure`
//! (writers only; the Bedrock writer maps the stair family's blockstate
//! properties to Bedrock `states` and drops unrepresentable intent with a
//! degradation note).
//! Planned: Litematica `.litematic` and the `WorldEdit` `.schem` format.

pub mod bedrock_state;
pub mod bedrock_structure;
pub mod data_version;
pub mod java_structure;
pub mod portability;
pub mod registry;

pub use bedrock_state::{BedrockStateError, StateTranslation, translate_states};
pub use bedrock_structure::{
    BedrockStructureError, ParityNote, build_mcstructure_tag, write_mcstructure,
};
pub use data_version::{
    BedrockTarget, JavaTarget, UnsupportedTarget, resolve_bedrock_target, resolve_java_target,
    supported_list,
};
pub use java_structure::{
    Compound, JavaStructureError, OutputExt, build_structure_tag, output_filename,
    write_compound_gzip, write_structure_gzip,
};
pub use portability::{PortabilityCounts, portability_for_bedrock, portability_for_java};
pub use registry::{
    PackEdition, PackFiles, PackManifest, PackSource, RegistryError, RegistryPack, builtin_bedrock,
    builtin_java, load_builtin_bedrock, load_builtin_java, load_from_dir,
};
