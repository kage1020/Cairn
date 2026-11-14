//! A door or window directly under a `level` is a port on that level's base row, and a walkway
//! joins two ports on one row (`spec/components-editing-sites` "Ports and `connect`").

use cairn_lang_core::block_array::{BlockArrayIr, Footprint};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{findings, lowered, slice};

const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n\n\
                       def ring size=3x9:\n  \
                       walls mat_slot=wall height=4\n\n";

/// `defs` and `site` after the shared prelude, lowered.
fn lowered_site(defs: &str, site: &str) -> (String, BlockArrayIr) {
    let source = format!("{PRELUDE}{defs}site s:\n{site}");
    let out = lowered(&source);
    (source, out)
}

/// The one walkway `out` laid, as its origin and footprint, or `None` when it laid none.
fn walkway(out: &BlockArrayIr) -> Option<((i32, i32, i32), Footprint)> {
    assert!(out.walkways.len() <= 1, "{:?}", out.walkways.keys());
    out.walkways
        .values()
        .next()
        .map(|w| (w.origin, w.footprint))
}

/// Two huts east of each other, joined front to front, with each door at `door`.
fn front_to_front(door: &str) -> (String, BlockArrayIr) {
    lowered_site(
        &format!(
            "def hut size=5x5:\n  \
             walls mat_slot=wall height=4\n\
             {door}\n\n"
        ),
        "  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@cobblestone\n",
    )
}

#[test]
fn a_door_under_a_ground_level_is_the_port_the_same_door_in_the_body_is() {
    let (_, in_level) = front_to_front("  level y=0\n    door id=e side=front at=center");
    let (_, in_body) = front_to_front("  door id=e side=front at=center");
    assert_eq!(in_level.diagnostics, []);
    assert_eq!(
        walkway(&in_level),
        Some(((2, 0, 5), Footprint { x: 10, z: 1 }))
    );
    assert_eq!(walkway(&in_level), walkway(&in_body));
}

#[test]
fn a_window_under_a_raised_level_is_a_port_on_that_levels_base_row() {
    let (_, out) = front_to_front("  level y=1\n    window id=e side=front offset=1 y=1 size=3x1");
    assert_eq!(out.diagnostics, []);
    assert_eq!(walkway(&out), Some(((2, 1, 5), Footprint { x: 10, z: 1 })));
}

/// `a` and `b` joined across `p`, walls with no floor that reach past their front walls, by doors
/// at `door`.
fn across_a_ring(door: &str) -> (String, BlockArrayIr) {
    lowered_site(
        &format!(
            "def hut size=3x3:\n  \
             floor mat_slot=wall\n  \
             walls mat_slot=wall height=4\n\
             {door}\n\n"
        ),
        "  place id=a use=hut theme=t at=origin\n  \
         place id=p use=ring theme=t east_of=a gap=1\n  \
         place id=b use=hut theme=t east_of=p gap=1\n  \
         connect a.e to b.e path=@cobblestone\n",
    )
}

#[test]
fn a_walkway_routes_around_what_the_placements_hold_on_its_own_row() {
    let (_, raised) = across_a_ring("  level y=2\n    door id=e side=front at=center");
    assert_eq!(raised.diagnostics, []);
    assert_eq!(
        walkway(&raised),
        Some(((1, 2, -1), Footprint { x: 9, z: 5 }))
    );
    let (_, ground) = across_a_ring("  door id=e side=front at=center");
    assert_eq!(
        ground
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.primary.as_str()))
            .collect::<Vec<_>>(),
        [(
            "E_PLACEMENT_OVERLAP",
            "walkway `a.e ↔ b.e` lays 3 cells inside `place id=p`, from (4, 0, 3) to (6, 0, 3)",
        )],
    );
    assert_eq!(
        walkway(&ground),
        Some(((1, 0, 3), Footprint { x: 9, z: 1 }))
    );
}

#[test]
fn ports_on_two_rows_are_not_joined() {
    let (source, out) = lowered_site(
        "def low size=5x5:\n  \
         walls mat_slot=wall height=4\n  \
         door id=e side=front at=center\n\n\
         def high size=5x5:\n  \
         walls mat_slot=wall height=4\n  \
         level y=2\n    \
         door id=e side=front at=center\n\n",
        "  place id=a use=high theme=t at=origin\n  \
         place id=b use=low theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@cobblestone\n",
    );
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::DeferredMember,
            "connect a.e to b.e path=@cobblestone",
            "walkway `a.e ↔ b.e` was skipped because its ports stand on two rows, `a.e` at y=2 \
             and `b.e` at y=0",
            vec![
                "a walkway is a flat strip on the row its two ports share and does not climb; \
                 join two openings that open onto one row, both in their `def` body or both \
                 under a `level` of one `y=`"
            ],
        )],
    );
    assert_eq!(walkway(&out), None);
}

#[test]
fn a_door_under_a_raised_level_is_judged_against_the_rows_above_that_level() {
    let (source, out) = front_to_front("  level y=4\n    door id=e side=front at=center");
    let walkway_findings: Vec<(&str, Vec<String>)> = out
        .diagnostics
        .iter()
        .filter(|d| slice(&source, d).starts_with("connect"))
        .map(|d| {
            (
                d.primary.as_str(),
                d.notes.iter().map(|n| n.message.clone()).collect(),
            )
        })
        .collect();
    let refused = "is a door that opens at y=5, the row above the floor slab, which is not inside \
                   any wall course (the walls occupy y=1..=4); the member says so on its own line \
                   too";
    assert_eq!(
        walkway_findings,
        [(
            "walkway `a.e ↔ b.e` was skipped because ports `a.e` and `b.e` could not be placed",
            vec![format!("`a.e` {refused}"), format!("`b.e` {refused}")],
        )],
    );
}

#[test]
fn a_port_under_a_level_whose_y_does_not_read_points_at_that_level() {
    let (source, out) = front_to_front("  level y=up\n    door id=e side=front at=center");
    let notes: Vec<(Option<&str>, String)> = out
        .diagnostics
        .iter()
        .filter(|d| slice(&source, d).starts_with("connect"))
        .flat_map(|d| &d.notes)
        .map(|n| (n.span.clone().map(|s| &source[s]), n.message.clone()))
        .collect();
    let unread = "is under a `level` whose `y=` does not read, so none of its members is built \
                  — the finding on that `level` line says why";
    assert_eq!(
        notes,
        [
            (Some("level y=up"), format!("`a.e` {unread}")),
            (Some("level y=up"), format!("`b.e` {unread}")),
        ],
    );
}

#[test]
fn a_window_under_a_raised_level_is_judged_against_the_rows_above_that_level() {
    let (source, out) =
        front_to_front("  level y=4\n    window id=e side=front offset=1 y=1 size=3x1");
    let notes: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| slice(&source, d).starts_with("connect"))
        .flat_map(|d| &d.notes)
        .map(|n| n.message.clone())
        .collect();
    let refused = "is a window whose rows y=5..=5 are not all inside one wall course (the walls \
                   occupy y=1..=4); the member says so on its own line too";
    assert_eq!(
        notes,
        [format!("`a.e` {refused}"), format!("`b.e` {refused}")]
    );
}
