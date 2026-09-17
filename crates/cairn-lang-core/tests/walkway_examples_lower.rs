//! The contract every shipped walkway example shares: it lowers to exactly
//! one walkway, under the key its `connect` row spells, with neither a
//! `W_WALKWAY_BLOCKED` nor a `W_DEFERRED_MEMBER` along the way.
//!
//! Each fixture exists to pin one piece of geometry — the L's elbow, the
//! corner-anchored doors, the window port — and `l_walkway_lower`,
//! `at_side_walkway_lower` and `window_walkway_lower` keep those
//! assertions. What they have in common is here, as one table, so a
//! fourth walkway example is one row rather than three copied tests.
//!
//! The diagnostics are read the way the CLI reports them: resolver
//! findings first, then the block-array pass's. Every fixture's geometry
//! is chosen so its strip clears every placement footprint, which makes
//! the absence of `W_WALKWAY_BLOCKED` the integration-level proxy for
//! `blocked_count == 0`; and every port is a door or window that places,
//! so nothing cascades into `W_DEFERRED_MEMBER` from `carve_door` or
//! `port_world_position`.

use cairn_lang_core::check::DiagnosticCode;

mod common;
use common::{lowered_with_resolver_diagnostics, read_example};

/// `(example, the one walkway key it must lower to)`.
const WALKWAY_EXAMPLES: &[(&str, &str)] = &[
    (
        "l-walkway.crn",
        "walkway::duo::home1.entry__home3.side_entry",
    ),
    (
        "at-side-walkway.crn",
        "walkway::duo::west.east_corner__east.west_corner",
    ),
    (
        "window-walkway.crn",
        "walkway::pair::home1.entry__home2.front",
    ),
];

#[test]
fn every_walkway_example_lowers_one_unblocked_walkway_under_its_key() {
    for (example, key) in WALKWAY_EXAMPLES {
        let out = lowered_with_resolver_diagnostics(&read_example(example));
        assert_eq!(
            out.walkways.len(),
            1,
            "{example}: expected exactly one walkway, got {:?}",
            out.walkways.keys().collect::<Vec<_>>(),
        );
        assert!(
            out.walkways.contains_key(*key),
            "{example}: missing walkway under key `{key}`, keys = {:?}",
            out.walkways.keys().collect::<Vec<_>>(),
        );
        let blocked: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
            .collect();
        assert!(
            blocked.is_empty(),
            "{example}: walkway must not collide with any placement, got {blocked:#?}",
        );
        let deferred: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert!(
            deferred.is_empty(),
            "{example}: no port or door may defer, got {deferred:#?}",
        );
    }
}
