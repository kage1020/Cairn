//! Mapping from surface command keywords to [`MemberRole`] classifications.
//!
//! Kept as a single source of truth so the `keyword_allowlist` validation
//! pass in `crate::check` can share the same table without drifting.
//!
//! The roster is drawn from the surface keywords used by the four shipped
//! examples (`cottage`, `themed-tower`, `village`, `redstone-door`) and from
//! the phase-ordered evaluation table in `spec/compilation.md` §4.1
//! (massing → envelope → openings → fixtures → logic). Any keyword outside
//! this table is intentionally surfaced as [`MemberRole::Other`] so the
//! lowering step never has to reject input.

use super::member::MemberRole;

/// All M2 keywords known to the role table, in their declaration order.
///
/// Used by the `keyword_allowlist` diagnostic pass to render the
/// "expected one of ..." note attached to `E_UNKNOWN_KEYWORD`. Kept in
/// lock-step with [`role_of`] — the unit test below trips if the two
/// drift apart.
pub const KNOWN_KEYWORDS: &[&str] = &[
    "floor",
    "walls",
    "door",
    "window",
    "roof",
    "stair",
    "level",
    "pressure_plate",
    "circuit",
    "place",
    "connect",
];

/// Return the M2 known-keyword table.
///
/// Public-facing helper so external passes can render the same list this
/// module uses for classification, without duplicating the constant.
#[must_use]
pub fn known_keywords() -> &'static [&'static str] {
    KNOWN_KEYWORDS
}

/// Look up a command keyword in the M2 known-keyword table.
///
/// Returns the corresponding [`MemberRole`] for known keywords and
/// [`MemberRole::Other`] (wrapping the original keyword string) for
/// everything else. The fallback keeps the AST → IR lowering total: an
/// unknown keyword surfaces as data rather than an error and is reported by
/// the `keyword_allowlist` pass in `crate::check`.
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

#[cfg(test)]
mod tests {
    use super::{KNOWN_KEYWORDS, MemberRole, role_of};

    #[test]
    fn known_keywords_round_trip_to_concrete_roles() {
        for kw in KNOWN_KEYWORDS {
            let role = role_of(kw);
            assert!(
                !matches!(role, MemberRole::Other(_)),
                "`{kw}` should classify to a concrete MemberRole, got {role:?}",
            );
        }
    }

    #[test]
    fn unknown_keyword_falls_through_to_other() {
        assert_eq!(role_of("mystery"), MemberRole::Other("mystery".to_owned()));
    }
}
