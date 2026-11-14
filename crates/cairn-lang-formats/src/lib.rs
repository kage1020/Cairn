//! Java `.nbt`, `.litematic` and `.schem` and Bedrock `.mcstructure` writers for the block-array
//! IR, `.litematic` and `.schem` readers, and the registry packs and portability reports the
//! writers rely on.

pub mod bedrock_state;
pub mod bedrock_structure;
mod block_id;
pub mod data_version;
mod dims;
pub mod java_structure;
pub mod litematic;
pub mod portability;
pub mod registry;
pub mod schem;
mod site_frame;
pub mod volume;

pub use bedrock_state::{BedrockStateError, StateTranslation, translate_states};
pub use bedrock_structure::{
    BedrockStructureError, McStructure, ParityNote, build_mcstructure_tag, prepare_mcstructure,
    write_mcstructure,
};
pub use data_version::{
    BedrockTarget, JavaTarget, UnsupportedTarget, resolve_bedrock_target, resolve_java_target,
};
pub use java_structure::{
    Compound, JavaStructure, JavaStructureError, OutputExt, build_structure_tag, output_filename,
    prepare_structure, write_compound_gzip, write_structure_gzip,
};
pub use litematic::{
    Litematic, LitematicError, LitematicFile, LitematicMetadata, LitematicReadError,
    LitematicRegion, LitematicSchematic, ReadRegion, litematic_files, prepare_litematic,
    read_litematic,
};
pub use portability::{
    InvalidPalette, PortabilityCounts, PortabilityReport, portability_for_bedrock,
    portability_for_java,
};
pub use registry::{
    PackFiles, PackManifest, PackSource, RegistryError, RegistryPack, builtin_pack,
    load_builtin_pack, load_from_dir,
};
pub use schem::{
    Schem, SchemError, SchemFile, SchemReadError, SpongeSchematic, prepare_schem, read_schem,
    schem_files,
};
pub use volume::{BlockVolume, StoredBlockEntity};
