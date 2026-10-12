//! Every shipped walkway example lowers to exactly the one walkway its `connect` row spells, with
//! no `W_WALKWAY_BLOCKED` or `W_DEFERRED_MEMBER`.

use cairn_lang_core::check::DiagnosticCode;

use crate::common::{examples, lowered_with_resolver_diagnostics, read_example};

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
fn the_table_names_every_walkway_example_shipped() {
    let on_disk: Vec<String> = examples()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.contains("walkway"))
        .collect();
    let mut in_table: Vec<String> = WALKWAY_EXAMPLES
        .iter()
        .map(|(example, _)| (*example).to_owned())
        .collect();
    in_table.sort();
    assert_eq!(in_table, on_disk);
}

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
