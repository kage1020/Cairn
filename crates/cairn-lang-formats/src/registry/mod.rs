//! Registry pack ingest.
//!
//! A *registry pack* is the bundle of JSON files describing the Minecraft
//! identifiers, blockstate domains, item schemas, and `DataVersion` table
//! that a compile depends on. It is the long-term replacement for the
//! hardcoded tables this crate used to ship, and is hashed into the
//! lockfile's `inputs.registry_pack_hash` field so a build is reproducible
//! against a pinned set of bytes.
//!
//! The 2026.12.0 cut introduced here ships only the `data_versions`
//! component — the smallest piece that lets `data_version.rs` stop carrying
//! a hardcoded `(mc_version, DataVersion)` array. Subsequent PRs slot in
//! block/item/tag tables and the semantic-sensitivity catalog by extending
//! `PackFiles` with new `Option` fields, so an older pack stays loadable.

pub mod data_versions;
pub mod hash;
pub mod load;
pub mod manifest;

pub use data_versions::{DataVersionEntry, DataVersionTable};
pub use hash::pack_hash;
pub use load::{
    PackSource, RegistryError, RegistryPack, SUPPORTED_DATA_VERSIONS_SCHEMA,
    SUPPORTED_MANIFEST_SCHEMA, builtin_java, load_builtin_java, load_from_dir,
};
pub use manifest::{PackEdition, PackFiles, PackManifest};
