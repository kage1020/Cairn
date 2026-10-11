//! Minecraft version labels: reading an `@requires` floor, and placing a
//! label in an edition's `DataVersion` order.
//!
//! The parser reads a member-level `requires` line with the same grammar
//! the `@requires` header uses, and `check` and `resolve` both weigh the
//! floors, so this sits below all three rather than inside one of them.
//! [`mod@crate::resolve`] re-exports every public item, which is the path
//! downstream crates use.

mod order;
mod requirement;

pub use order::{FloorPlacement, FloorVerdict, VersionOrder};
pub use requirement::{
    Requirement, RequirementError, compare_versions, parse_min_version, parse_requirement,
};
