//! Readers and writers around the Cairn block-array IR for existing schematic formats.
//!
//! Supported: vanilla Java `.nbt` structures and Bedrock `.mcstructure`
//! (writers only; the Bedrock writer emits stateless palettes until
//! per-edition state mapping lands).
//! Planned: Litematica `.litematic` and the `WorldEdit` `.schem` format.

pub mod bedrock_structure;
pub mod data_version;
pub mod java_structure;
pub mod registry;

pub use bedrock_structure::{BedrockStructureError, build_mcstructure_tag, write_mcstructure};
pub use data_version::{
    BedrockTarget, JavaTarget, UnsupportedTarget, resolve_bedrock_target, resolve_java_target,
    supported_list,
};
pub use java_structure::{
    Compound, JavaStructureError, OutputExt, build_structure_tag, output_filename,
    write_compound_gzip, write_structure_gzip,
};
pub use registry::{
    PackEdition, PackFiles, PackManifest, PackSource, RegistryError, RegistryPack, builtin_bedrock,
    builtin_java, load_builtin_bedrock, load_builtin_java, load_from_dir,
};
