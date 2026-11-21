//! A Java world save, as a structure file is placed in one (`spec/versioning-editions`
//! "Cross-version application"): the version its `level.dat` records, the `DataVersion` a
//! structure file is stamped with, the structure id a file is placed under and the file in the
//! world that id names, and how the two versions stand.

mod entries;
mod level_dat;
mod structure_file;
mod structure_id;

pub use level_dat::{LEVEL_DAT, LevelDatError, WorldVersion, read_level_dat, worlds_in};
pub use structure_file::{
    MissingLists, StructureFileError, StructureList, read_structure_data_version,
};
pub use structure_id::{DEFAULT_NAMESPACE, IdPart, Namespace, StructureId, StructureIdError};

use crate::registry::{DataVersionEntry, RegistryPack};

/// How a structure file's `DataVersion` stands to the world it is installed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossVersion {
    /// Both carry one `DataVersion`.
    Same,
    /// The structure is older than the world, which upgrades it as it loads.
    Upgrade,
    /// The structure is newer than the world, whose release cannot read it.
    Downgrade,
}

impl WorldVersion {
    /// How a structure file stamped `stamp` stands to this world.
    #[must_use]
    pub fn cross_version(&self, stamp: i32) -> CrossVersion {
        match stamp.cmp(&self.data_version) {
            std::cmp::Ordering::Equal => CrossVersion::Same,
            std::cmp::Ordering::Less => CrossVersion::Upgrade,
            std::cmp::Ordering::Greater => CrossVersion::Downgrade,
        }
    }
}

/// The newest version the Java `pack` builds for at or below a world saved at `world`: the
/// `--target` a structure for that world is recompiled at, `None` when every one is newer.
#[must_use]
pub fn recompile_target(pack: &RegistryPack, world: i32) -> Option<&DataVersionEntry> {
    pack.targetable()
        .into_iter()
        .rfind(|row| row.data_version <= world)
}
