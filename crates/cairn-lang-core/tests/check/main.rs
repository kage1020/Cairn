//! The check passes, driven through `check` on small sources.

#[path = "../common/mod.rs"]
pub mod common;

mod arguments;
mod binding;
mod cairn_version;
mod connect_arity;
mod diagnostic_text;
mod duplicate;
mod incomplete_place;
mod keyword_allowlist;
mod member_scope;
mod missing_material;
mod nesting;
mod output_name_collision;
mod positional;
mod requires;
mod truth_table;
mod type_mismatch;
