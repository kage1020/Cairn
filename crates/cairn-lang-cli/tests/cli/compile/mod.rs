//! `cairn compile`: the artifacts and the lockfile it writes, and the
//! sources and targets it refuses before writing anything.

mod artifacts;
mod atomicity;
mod block_ids;
mod edition_theme;
mod lock_readback;
mod output_name_collision;
mod roof_material;
mod state_literal;
mod unknown_argument;
mod version_cap;
