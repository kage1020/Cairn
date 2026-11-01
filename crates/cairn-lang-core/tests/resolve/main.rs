//! Name, theme and version resolution, and what it hands the passes after it.

#[path = "../common/mod.rs"]
pub mod common;

mod composite_version_floors;
mod def_member;
mod intended_targets;
mod resolver;
mod selector_list_attrs;
mod silent_skip_arms;
