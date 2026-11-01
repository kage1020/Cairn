//! Registry packs: the JSON files of `DataVersion`s, materials, block ids and aliases a compile
//! depends on, hashed into the lockfile so a build is reproducible.

pub mod aliases;
pub mod blocks;
pub mod data_versions;
pub mod hash;
pub mod load;
pub mod manifest;
pub mod materials;

pub use aliases::{AliasCatalog, AliasError, AliasGroup, AliasIndex, SUPPORTED_ALIASES_SCHEMA};
pub use blocks::{
    BlocksBase, BlocksCatalog, BlocksDiff, BlocksError, BlocksIndex, SUPPORTED_BLOCKS_SCHEMA,
};
pub use data_versions::{DataVersionEntry, DataVersionTable};
pub use hash::pack_hash;
pub use load::{
    PackSource, PackView, RegistryError, RegistryPack, SUPPORTED_DATA_VERSIONS_SCHEMA,
    SUPPORTED_MANIFEST_SCHEMA, builtin_pack, load_builtin_pack, load_from_dir,
};
pub use manifest::{PackFiles, PackManifest};

/// Prepends `namespace` to a component entry that does not carry one.
pub(crate) fn namespaced(namespace: &str, block: &str) -> String {
    if block.contains(':') {
        block.to_owned()
    } else {
        format!("{namespace}:{block}")
    }
}
pub use materials::{
    MaterialEntry, MaterialsCatalog, MaterialsError, MaterialsIndex, SUPPORTED_MATERIALS_SCHEMA,
};
