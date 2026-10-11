//! Registry pack ingest.
//!
//! A *registry pack* is the bundle of JSON files describing the Minecraft
//! identifiers, blockstate domains, item schemas, and `DataVersion` table
//! that a compile depends on. It is hashed into the lockfile's
//! `inputs.registry_pack_hash` field so a build is reproducible against a
//! pinned set of bytes.
//!
//! Its components are `data_versions` (`mc_version` → `DataVersion`),
//! `materials` (abstract `@token` → id), `blocks` (the per-version id table
//! an `E_UNKNOWN_ID` is decided against), and `aliases` (the spellings one
//! block has worn, which turns a refused id into the name the target does
//! use).

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
    resolve_bedrock_target, resolve_java_target,
};
pub use manifest::{PackFiles, PackManifest};

/// Prepend `namespace` to a component entry that does not carry one.
///
/// Shared by the `materials`, `blocks` and `aliases` components, which name
/// blocks the same way: `oak_planks` inherits the catalog's namespace and
/// `create:cogwheel` keeps its own. One copy matters because an alias group
/// is matched against a block table by string equality.
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
