//! Mapping from surface command keywords to [`MemberRole`] classifications.
//!
//! Kept as a single source of truth so the M2-PR2 `keyword_allowlist`
//! validation pass can share the same table without drifting.
//!
//! The roster is drawn from the surface keywords used by the four shipped
//! examples (`cottage`, `themed-tower`, `village`, `redstone-door`) and from
//! the phase-ordered evaluation table in `spec/compilation.md` §4.1
//! (massing → envelope → openings → fixtures → logic). Any keyword outside
//! this table is intentionally surfaced as [`MemberRole::Other`] so the
//! lowering step never has to reject input.

use super::member::MemberRole;

/// Look up a command keyword in the M2 known-keyword table.
///
/// Returns the corresponding [`MemberRole`] for known keywords and
/// [`MemberRole::Other`] (wrapping the original keyword string) for
/// everything else. The fallback keeps the AST → IR lowering total: an
/// unknown keyword surfaces as data rather than an error and is reported by
/// the validation passes added in M2-PR2.
#[must_use]
pub fn role_of(keyword: &str) -> MemberRole {
    match keyword {
        "floor" => MemberRole::Floor,
        "walls" => MemberRole::Walls,
        "door" => MemberRole::Door,
        "window" => MemberRole::Window,
        "roof" => MemberRole::Roof,
        "stair" => MemberRole::Stair,
        "level" => MemberRole::Level,
        "pressure_plate" => MemberRole::PressurePlate,
        "circuit" => MemberRole::Circuit,
        "place" => MemberRole::Place,
        "connect" => MemberRole::Connect,
        other => MemberRole::Other(other.to_owned()),
    }
}
