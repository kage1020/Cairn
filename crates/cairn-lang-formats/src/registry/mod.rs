//! Registry pack ingest.
//!
//! A *registry pack* is the bundle of JSON files describing the Minecraft
//! identifiers, blockstate domains, item schemas, and `DataVersion` table
//! that a compile depends on. It is the long-term replacement for the
//! hardcoded tables this crate used to ship, and is hashed into the
//! lockfile's `inputs.registry_pack_hash` field so a build is reproducible
//! against a pinned set of bytes.
//!
//! Three components ship today: `data_versions` (which let `data_version.rs`
//! stop carrying a hardcoded `(mc_version, DataVersion)` array), `materials`
//! (abstract `@token` → id), and `blocks` (the per-version id table an
//! `E_UNKNOWN_ID` is decided against). Later additions — item and tag
//! tables, the semantic-sensitivity catalog — slot in by extending
//! [`PackFiles`] with new `Option` fields, so an older pack stays
//! loadable.

pub mod blocks;
pub mod data_versions;
pub mod hash;
pub mod load;
pub mod manifest;
pub mod materials;

pub use blocks::{
    BlocksBase, BlocksCatalog, BlocksDiff, BlocksError, BlocksIndex, SUPPORTED_BLOCKS_SCHEMA,
};
pub use data_versions::{DataVersionEntry, DataVersionTable};
pub use hash::pack_hash;
pub use load::{
    PackSource, PackView, RegistryError, RegistryPack, SUPPORTED_DATA_VERSIONS_SCHEMA,
    SUPPORTED_MANIFEST_SCHEMA, builtin_bedrock, builtin_java, load_builtin_bedrock,
    load_builtin_java, load_from_dir,
};
pub use manifest::{PackEdition, PackFiles, PackManifest};

/// Prepend `namespace` to a component entry that does not carry one.
///
/// Shared by the `materials` and `blocks` components because they name the
/// same thing the same way: an entry may write `oak_planks` and inherit the
/// catalog's namespace, or write `create:cogwheel` and keep its own. Two
/// copies of this rule is two places for the two components to drift apart
/// on what counts as namespaced.
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
