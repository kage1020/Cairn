//! A door or window directly under a `level` is a port on that level's base row, and a walkway
//! joins two ports on one row, round every cell the placements of its site take up on that row
//! (`spec/components-editing-sites` "Ports and `connect`").

use cairn_lang_core::block_array::{BlockArrayIr, Footprint};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{findings, lowered};

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

/// Every walkway `out` laid, as its key, origin and footprint.
fn walkways(out: &BlockArrayIr) -> Vec<(&str, (i32, i32, i32), Footprint)> {
    out.walkways
        .iter()
        .map(|(key, w)| (key.as_str(), w.origin, w.footprint))
        .collect()
}

/// The one walkway `out` laid, as its origin and footprint, or `None` when it laid none.
fn walkway(out: &BlockArrayIr) -> Option<((i32, i32, i32), Footprint)> {
    assert!(out.walkways.len() <= 1, "{:?}", out.walkways.keys());
    out.walkways
        .values()
        .next()
        .map(|w| (w.origin, w.footprint))
}

/// Each note of every finding of `out` on the `connect` row, as the text it points at.
fn connect_note_spans<'a>(source: &'a str, out: &BlockArrayIr) -> Vec<Option<&'a str>> {
    out.diagnostics
        .iter()
        .filter(|d| source[d.span.clone()].starts_with("connect"))
        .flat_map(|d| &d.notes)
        .map(|n| n.span.clone().map(|span| &source[span]))
        .collect()
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

const CONNECT: &str = "connect a.e to b.e path=@cobblestone";

/// The note a walkway skipped over a port the openings pass refused ends with.
const OWN_LINE: &str = "; the member says so on its own line too";

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
fn a_walkway_routes_round_the_room_of_a_placement_on_its_row() {
    let (_, raised) = across_a_ring("  level y=2\n    door id=e side=front at=center");
    assert_eq!(raised.diagnostics, []);
    assert_eq!(
        walkway(&raised),
        Some(((1, 2, -1), Footprint { x: 9, z: 5 }))
    );
    let (_, ground) = across_a_ring("  door id=e side=front at=center");
    assert_eq!(ground.diagnostics, []);
    assert_eq!(
        walkway(&ground),
        Some(((1, 0, -1), Footprint { x: 9, z: 5 }))
    );
}

/// Two huts east of each other, joined from the west wall of each by doors under `level y=1`,
/// with `windows` in the body.
fn west_to_west(windows: &str) -> (String, BlockArrayIr) {
    lowered_site(
        &format!(
            "def hut size=5x5:\n  \
             floor mat_slot=wall\n  \
             walls mat_slot=wall height=4\n\
             {windows}  \
             level y=1\n    \
             door id=e side=left at=center\n\n"
        ),
        "  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@cobblestone\n",
    )
}

#[test]
fn a_walkway_goes_round_a_placement_it_joins_rather_than_through_its_room() {
    // The east and west windows face each other across `a`'s room, on the walkway's row.
    for windows in [
        "  window side=left offset=2 y=1 size=1x1\n  window side=right offset=2 y=1 size=1x1\n",
        "  window side=left offset=1 y=1 size=1x1\n  window side=right offset=3 y=1 size=1x1\n",
        "",
    ] {
        let (_, out) = west_to_west(windows);
        assert_eq!(out.diagnostics, [], "{windows}");
        assert_eq!(
            walkway(&out),
            Some(((-1, 1, 2), Footprint { x: 10, z: 4 })),
            "{windows}",
        );
    }
    let (_, floorless) = lowered_site(
        "def hut size=5x5:\n  \
         walls mat_slot=wall height=4\n  \
         door id=e side=left at=center\n\n",
        "  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@cobblestone\n",
    );
    assert_eq!(floorless.diagnostics, []);
    assert_eq!(
        walkway(&floorless),
        Some(((-1, 0, 2), Footprint { x: 10, z: 4 }))
    );
}

#[test]
fn a_walkway_bridges_over_a_placement_whose_top_is_below_its_row() {
    let (_, out) = lowered_site(
        "def shed size=3x9:\n  \
         floor mat_slot=wall\n  \
         walls mat_slot=wall height=1\n\n\
         def tower size=3x3:\n  \
         floor mat_slot=wall\n  \
         walls mat_slot=wall height=6\n  \
         level y=4\n    \
         door id=e side=front at=center\n\n",
        "  place id=a use=tower theme=t at=origin\n  \
         place id=p use=shed theme=t east_of=a gap=1\n  \
         place id=b use=tower theme=t east_of=p gap=1\n  \
         connect a.e to b.e path=@cobblestone\n",
    );
    assert_eq!(out.diagnostics, []);
    assert_eq!(walkway(&out), Some(((1, 4, 3), Footprint { x: 9, z: 1 })));
}

#[test]
fn walkways_of_one_site_on_two_rows_are_each_laid_on_their_own_row() {
    let (_, out) = lowered_site(
        "def tower size=5x5:\n  \
         walls mat_slot=wall height=6\n  \
         door id=dn side=front at=center\n  \
         level y=3\n    \
         door id=up side=front at=center\n\n",
        "  place id=a use=tower theme=t at=origin\n  \
         place id=b use=tower theme=t east_of=a gap=4\n  \
         connect a.up to b.up path=@cobblestone\n  \
         connect a.dn to b.dn path=@cobblestone\n",
    );
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        walkways(&out),
        [
            (
                "walkway::s::a.up__b.up",
                (2, 3, 5),
                Footprint { x: 10, z: 1 }
            ),
            (
                "walkway::s::a.dn__b.dn",
                (2, 0, 5),
                Footprint { x: 10, z: 1 }
            ),
        ],
    );
}

#[test]
fn a_walkway_under_the_eaves_of_a_third_placement_on_a_raised_row_is_reported() {
    let (source, out) = lowered_site(
        "def tower size=3x3:\n  \
         floor mat_slot=wall\n  \
         walls mat_slot=wall height=5\n  \
         level y=3\n    \
         door id=e side=front at=center\n\n\
         def eaved size=3x3:\n  \
         floor mat_slot=wall\n  \
         walls mat_slot=wall height=5\n  \
         roof kind=flat mat_slot=wall overhang=1\n\n",
        "  place id=a use=tower theme=t at=origin\n  \
         place id=c use=eaved theme=t east_of=a\n  \
         place id=b use=tower theme=t east_of=c\n  \
         connect a.e to b.e path=@cobblestone\n",
    );
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::PlacementOverlap,
            CONNECT,
            "walkway `a.e ↔ b.e` lays 5 cells inside `place id=c`, within (3, 3, 4) to (7, 3, 4)",
            vec![
                "`place id=c` spans (3, 0, 0) to (7, 6, 4)",
                "a walkway may lay no cell inside a placement other than the two it joins: \
                 `place id=c`'s file writes air in every cell its body does not fill, so \
                 whichever of it and the walkway is placed second decides what those cells \
                 hold; move `place id=c` off the route between `a.e` and `b.e`",
            ],
        )],
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
            CONNECT,
            "walkway `a.e ↔ b.e` was skipped because its ports stand on two rows, `a.e` at y=2 \
             and `b.e` at y=0",
            vec![
                "a walkway is a flat strip on the row its two ports share and does not climb; \
                 join two openings whose `level`s have one `y=`, a member of the body counting \
                 as `y=0`"
            ],
        )],
    );
    assert_eq!(walkway(&out), None);
}

#[test]
fn a_door_under_a_raised_level_is_judged_against_the_rows_above_that_level() {
    let (source, out) = front_to_front("  level y=4\n    door id=e side=front at=center");
    let refused = "is a door that opens at y=5, the row above the base row of its `level`, which \
                   is not inside any wall course (the walls occupy y=1..=4)";
    let member = (
        DiagnosticCode::InvalidMember,
        "door id=e side=front at=center",
        "door opens at y=5, which is not inside any wall course (the walls occupy y=1..=4)",
        vec![
            "a door opens one row above the base of its `level`, or of the body outside one; move \
             it under a `level` whose walls hold that row and the one above, or add walls there",
        ],
    );
    assert_eq!(
        findings(&source, &out),
        [
            member,
            (
                DiagnosticCode::DeferredMember,
                CONNECT,
                "walkway `a.e ↔ b.e` was skipped because ports `a.e` and `b.e` could not be placed",
                vec![
                    format!("`a.e` {refused}{OWN_LINE}").as_str(),
                    format!("`b.e` {refused}{OWN_LINE}").as_str(),
                ],
            ),
        ],
    );
}

#[test]
fn a_port_under_a_level_whose_y_does_not_read_points_at_that_level() {
    let (source, out) = front_to_front("  level y=up\n    door id=e side=front at=center");
    let unread = "is under a `level` whose `y=` does not read, so none of its members is built \
                  — the `E_UNSUPPORTED_NESTING` on that `level`'s members says why";
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::DeferredMember,
            CONNECT,
            "walkway `a.e ↔ b.e` was skipped because ports `a.e` and `b.e` could not be placed",
            vec![
                format!("`a.e` {unread}").as_str(),
                format!("`b.e` {unread}").as_str(),
            ],
        )],
    );
    assert_eq!(
        connect_note_spans(&source, &out),
        [Some("level y=up"), Some("level y=up")]
    );
}

#[test]
fn a_window_under_a_raised_level_is_judged_against_the_rows_above_that_level() {
    let (source, out) =
        front_to_front("  level y=4\n    window id=e side=front offset=1 y=1 size=3x1");
    let refused = "is a window whose rows y=5..=5 are not all inside one wall course (the walls \
                   occupy y=1..=4)";
    let member = (
        DiagnosticCode::InvalidMember,
        "window id=e side=front offset=1 y=1 size=3x1",
        "window rows y=5..=5 are not all inside one wall course (size=3x1; the walls occupy \
         y=1..=4)",
        vec![
            "move the window with `y=`, shorten it with `size=`, or raise the walls' `height=`, so \
             one course holds every row it cuts",
        ],
    );
    assert_eq!(
        findings(&source, &out),
        [
            member,
            (
                DiagnosticCode::DeferredMember,
                CONNECT,
                "walkway `a.e ↔ b.e` was skipped because ports `a.e` and `b.e` could not be placed",
                vec![
                    format!("`a.e` {refused}{OWN_LINE}").as_str(),
                    format!("`b.e` {refused}{OWN_LINE}").as_str(),
                ],
            ),
        ],
    );
}

#[test]
fn an_opening_under_a_level_past_the_highest_row_is_refused_on_its_line_and_as_a_port() {
    let (source, out) = lowered_site(
        "def hut size=5x5:\n  \
         walls mat_slot=wall height=4\n  \
         level y=4294967295\n    \
         door id=e side=front at=center\n  \
         level y=4294967295\n    \
         window id=w side=back offset=1 y=1 size=1x1\n\n",
        "  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@cobblestone\n  \
         connect a.w to b.w path=@cobblestone\n",
    );
    let door = "is a door under `level y=4294967295`, which opens past the highest row a build can \
                address (the walls occupy y=1..=4)";
    let window = "is a window under `level y=4294967295` whose rows from y=1 run past the highest \
                  row a build can address (the walls occupy y=1..=4)";
    assert_eq!(
        findings(&source, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "door id=e side=front at=center",
                "door under `level y=4294967295` opens past the highest row a build can address \
                 (the walls occupy y=1..=4)",
                vec![
                    "a door opens one row above the base of its `level`; lower the `level` with `y=`"
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "window id=w side=back offset=1 y=1 size=1x1",
                "window rows from y=1 under `level y=4294967295` run past the highest row a build \
                 can address (size=1x1; the walls occupy y=1..=4)",
                vec!["lower the window or its `level` with `y=`, or shorten it with `size=`"],
            ),
            (
                DiagnosticCode::DeferredMember,
                CONNECT,
                "walkway `a.e ↔ b.e` was skipped because ports `a.e` and `b.e` could not be placed",
                vec![
                    format!("`a.e` {door}{OWN_LINE}").as_str(),
                    format!("`b.e` {door}{OWN_LINE}").as_str(),
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                "connect a.w to b.w path=@cobblestone",
                "walkway `a.w ↔ b.w` was skipped because ports `a.w` and `b.w` could not be placed",
                vec![
                    format!("`a.w` {window}{OWN_LINE}").as_str(),
                    format!("`b.w` {window}{OWN_LINE}").as_str(),
                ],
            ),
        ],
    );
}
