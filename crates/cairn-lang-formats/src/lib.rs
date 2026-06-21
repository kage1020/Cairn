//! Readers and writers around the Cairn block-array IR for existing schematic formats.
//!
//! Supported: vanilla Java `.nbt` structures (writer only).
//! Planned: Litematica `.litematic`, the `WorldEdit` `.schem` format, and
//! Bedrock `.mcstructure`.

pub mod data_version;
pub mod java_structure;

pub use data_version::{JavaTarget, UnsupportedTarget, resolve_java_target, supported_list};
pub use java_structure::{
    Compound, JavaStructureError, build_structure_tag, output_filename, write_compound_gzip,
    write_structure_gzip,
};
