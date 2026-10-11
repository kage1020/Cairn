//! Minecraft version labels: reading an `@requires` floor, and placing a label in an edition's
//! `DataVersion` order. Downstream crates reach these through [`mod@crate::resolve`].

mod order;
mod requirement;

pub use order::{FloorPlacement, FloorVerdict, VersionOrder};
pub use requirement::{
    Requirement, RequirementError, compare_versions, parse_min_version, parse_requirement,
};
