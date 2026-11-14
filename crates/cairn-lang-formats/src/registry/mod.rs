//! Registry packs: the JSON files of `DataVersion`s, materials, block ids, aliases, absences,
//! meanings, state maps and connection tables a compile depends on, hashed into the lockfile so a
//! build is reproducible.

pub mod absent;
pub mod aliases;
pub mod blocks;
pub mod connections;
pub mod data_versions;
pub mod hash;
pub mod load;
pub mod manifest;
pub mod materials;
pub mod meanings;
pub mod states;

pub use absent::{AbsentCatalog, AbsentEntry, AbsentError, AbsentIndex, SUPPORTED_ABSENT_SCHEMA};
pub use aliases::{AliasCatalog, AliasError, AliasGroup, AliasIndex, SUPPORTED_ALIASES_SCHEMA};
pub use blocks::{
    BlockEntries, BlockStates, BlocksBase, BlocksCatalog, BlocksDiff, BlocksError, BlocksIndex,
    SUPPORTED_BLOCKS_SCHEMA, StateValue,
};
pub use connections::{
    ByState, ConnectionIndex, ConnectionKind, ConnectionRow, ConnectionsCatalog, ConnectionsError,
    SUPPORTED_CONNECTIONS_SCHEMA, StateLookup,
};
pub use data_versions::{DataVersionEntry, DataVersionTable};
pub use hash::pack_hash;
pub use load::{
    PackSource, PackView, RegistryError, RegistryPack, Respelling, SUPPORTED_DATA_VERSIONS_SCHEMA,
    SUPPORTED_MANIFEST_SCHEMA, builtin_pack, load_builtin_pack, load_from_dir,
};
pub use manifest::{PackFiles, PackManifest};
pub use states::{
    BedrockForm, DroppedState, SUPPORTED_SENSITIVITY_SCHEMA, SUPPORTED_STATES_SCHEMA,
    SensitivityCatalog, SensitivityKind, StateMapIndex, StateRefusal, StatesCatalog, StatesError,
    VersionStates,
};

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
pub use meanings::{
    MeaningIndex, MeaningRow, MeaningsCatalog, MeaningsError, SUPPORTED_MEANINGS_SCHEMA,
};
