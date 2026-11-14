//! A Java world save, as `cairn install` places a structure file in one
//! (`spec/versioning-editions` "Cross-version application"): the version its `level.dat` records,
//! the `DataVersion` a structure file is stamped with, the structure id a file is installed under
//! and the file in the world that id names, and how the two versions stand.

mod level_dat;
mod structure_file;
mod structure_id;

pub use level_dat::{LEVEL_DAT, LevelDatError, WorldVersion, read_level_dat, worlds_in};
pub use structure_file::{StructureFileError, read_structure_data_version};
pub use structure_id::{DEFAULT_NAMESPACE, IdPart, StructureId, StructureIdError};

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

impl CrossVersion {
    /// How a structure stamped `structure` stands to a world saved at `world`.
    #[must_use]
    pub fn of(structure: i32, world: i32) -> Self {
        match structure.cmp(&world) {
            std::cmp::Ordering::Equal => Self::Same,
            std::cmp::Ordering::Less => Self::Upgrade,
            std::cmp::Ordering::Greater => Self::Downgrade,
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
