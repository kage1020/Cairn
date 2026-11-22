//! What a villager's level saves beside it, the same in either edition.

use cairn_lang_core::block_array::VillagerLevel;

/// The experience a villager of `level` is saved with: its level's start, and 1 for a novice,
/// whom the game would otherwise take the profession of without a job site.
pub(crate) const fn saved_experience(level: VillagerLevel) -> i32 {
    match level {
        VillagerLevel::Novice => 1,
        VillagerLevel::Apprentice => 10,
        VillagerLevel::Journeyman => 70,
        VillagerLevel::Expert => 150,
        VillagerLevel::Master => 250,
    }
}
