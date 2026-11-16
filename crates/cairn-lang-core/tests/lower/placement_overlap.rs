//! Two `place` rows of one site whose boxes share a cell, and a walkway laid inside a placement it
//! does not join, are `E_PLACEMENT_OVERLAP` (`spec/entities` "Anchor conventions").

use cairn_lang_core::block_array::BlockArrayIr;

use crate::common::{lowered, slice};

const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n\n\
                       def hut size=3x3:\n  \
                       floor mat_slot=wall\n  \
                       walls mat_slot=wall height=2\n  \
                       door id=front side=front at=center\n\n\
                       def rear size=3x3:\n  \
                       floor mat_slot=wall\n  \
                       walls mat_slot=wall height=2\n  \
                       door id=back side=back at=center\n\n\
                       def hall size=3x9:\n  \
                       floor mat_slot=wall\n  \
                       walls mat_slot=wall height=2\n\n\
                       def wing size=9x3:\n  \
                       floor mat_slot=wall\n  \
                       walls mat_slot=wall height=2\n\n\
                       def eaved size=3x3:\n  \
                       floor mat_slot=wall\n  \
                       walls mat_slot=wall height=2\n  \
                       door id=front side=front at=center\n  \
                       roof kind=flat mat_slot=wall overhang=1\n\n\
                       def yard size=9x9:\n  \
                       floor mat_slot=wall\n\n";

/// `site` after the shared defs, lowered.
fn lowered_site(site: &str) -> (String, BlockArrayIr) {
    let source = format!("{PRELUDE}{site}");
    let out = lowered(&source);
    (source, out)
}

/// A finding as its code, the text it points at, its primary, and each note as the text its span
/// points at beside its message.
type Overlap<'a> = (&'a str, &'a str, &'a str, Vec<(Option<&'a str>, &'a str)>);

/// Every finding of `out`, in report order.
fn overlaps<'a>(source: &'a str, out: &'a BlockArrayIr) -> Vec<Overlap<'a>> {
    out.diagnostics
        .iter()
        .map(|d| {
            let notes = d
                .notes
                .iter()
                .map(|n| (n.span.clone().map(|span| &source[span]), n.message.as_str()))
                .collect();
            (d.code.as_str(), slice(source, d), d.primary.as_str(), notes)
        })
        .collect()
}

/// The primaries of every finding `site` raises.
fn primaries_of(site: &str) -> Vec<String> {
    lowered_site(site)
        .1
        .diagnostics
        .into_iter()
        .map(|d| d.primary)
        .collect()
}

/// The note every placement overlap opens with, before its fix.
const SHARE_NO_CELL: &str = "two placements of one site may share no cell: each is written with \
                             air in every cell its body does not fill, so whichever of the two is \
                             placed second decides what those cells hold";

/// The note on a placement overlap whose later row is `at=origin`.
fn at_origin_fix() -> String {
    format!(
        "{SHARE_NO_CELL}; this row's `at=origin` puts it on the world origin, which no `gap=` \
         moves; place it `east_of=` or `north_of=` the row it should stand beside"
    )
}

/// The note on a placement overlap whose later row steps `toward` from another, with the `gap=`
/// that clears `place id={earlier}`.
fn gap_fix(gap: u32, toward: &str, earlier: &str) -> String {
    format!(
        "{SHARE_NO_CELL}; write `gap={gap}` on this row to move it, and every row placed relative \
         to it, {toward} of `place id={earlier}`"
    )
}

/// The note on a strip laid inside `place id={id}` by the walkway from `from` to `to`.
fn strip_fix(id: &str, from: &str, to: &str) -> String {
    format!(
        "a walkway may lay no cell inside a placement other than the two it joins: `place \
         id={id}`'s file writes air in every cell its body does not fill, so whichever of it and \
         the walkway is placed second decides what those cells hold; move `place id={id}` off the \
         route between `{from}` and `{to}`"
    )
}

#[test]
fn a_row_overlapping_earlier_rows_is_reported_once_for_each() {
    let (a_row, b_row, c_row) = (
        "place id=a use=hut theme=t at=origin",
        "place id=b use=hut theme=t at=origin",
        "place id=c use=hut theme=t at=origin",
    );
    let (source, out) = lowered_site(&format!("site s:\n  {a_row}\n  {b_row}\n  {c_row}\n"));
    let fix = at_origin_fix();
    assert_eq!(
        overlaps(&source, &out),
        [
            (
                "E_PLACEMENT_OVERLAP",
                b_row,
                "`place id=b` spans (0, 0, 0) to (2, 2, 2) and shares (0, 0, 0) to (2, 2, 2) \
                 with `place id=a`",
                vec![
                    (Some(a_row), "`place id=a` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, fix.as_str()),
                ],
            ),
            (
                "E_PLACEMENT_OVERLAP",
                c_row,
                "`place id=c` spans (0, 0, 0) to (2, 2, 2) and shares (0, 0, 0) to (2, 2, 2) \
                 with `place id=a`",
                vec![
                    (Some(a_row), "`place id=a` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, fix.as_str()),
                ],
            ),
            (
                "E_PLACEMENT_OVERLAP",
                c_row,
                "`place id=c` spans (0, 0, 0) to (2, 2, 2) and shares (0, 0, 0) to (2, 2, 2) \
                 with `place id=b`",
                vec![
                    (Some(b_row), "`place id=b` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, fix.as_str()),
                ],
            ),
        ],
    );
}

/// `d` is placed east of `a`, onto the part of the deep `c` that reaches past `b` towards `+z`.
fn east_chain(d_gap: &str) -> String {
    format!(
        "site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t north_of=a\n  \
         place id=c use=hall theme=t east_of=b\n  \
         place id=d use=hut theme=t east_of=a{d_gap}\n",
    )
}

#[test]
fn a_relative_row_that_lands_on_another_row_is_reported_with_the_gap_that_clears_it() {
    let (source, out) = lowered_site(&east_chain(""));
    let fix = gap_fix(3, "east", "c");
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=d use=hut theme=t east_of=a",
            "`place id=d` spans (3, 0, 0) to (5, 2, 2) and shares (3, 0, 0) to (5, 2, 2) with \
             `place id=c`",
            vec![
                (
                    Some("place id=c use=hall theme=t east_of=b"),
                    "`place id=c` spans (3, 0, -3) to (5, 2, 5)",
                ),
                (None, fix.as_str()),
            ],
        )],
    );
}

#[test]
fn the_gap_an_east_of_overlap_names_adds_to_the_gap_written_and_clears_it() {
    let (source, out) = lowered_site(&east_chain(" gap=2"));
    let fix = gap_fix(3, "east", "c");
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=d use=hut theme=t east_of=a gap=2",
            "`place id=d` spans (5, 0, 0) to (7, 2, 2) and shares (5, 0, 0) to (5, 2, 2) with \
             `place id=c`",
            vec![
                (
                    Some("place id=c use=hall theme=t east_of=b"),
                    "`place id=c` spans (3, 0, -3) to (5, 2, 5)",
                ),
                (None, fix.as_str()),
            ],
        )],
    );
    assert_eq!(primaries_of(&east_chain(" gap=3")), Vec::<String>::new());
}

/// `e` is placed north of `d`, onto the wide `y` placed north of `a` beside it.
fn north_chain(e_gap: &str) -> String {
    format!(
        "site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=y use=wing theme=t north_of=a\n  \
         place id=d use=hut theme=t east_of=a gap=3\n  \
         place id=e use=hut theme=t north_of=d{e_gap}\n",
    )
}

#[test]
fn a_north_of_row_that_lands_on_another_row_is_reported_with_the_gap_that_clears_it() {
    let fix = gap_fix(3, "north", "y");
    let y_spans = (
        Some("place id=y use=wing theme=t north_of=a"),
        "`place id=y` spans (0, 0, -3) to (8, 2, -1)",
    );
    let (source, out) = lowered_site(&north_chain(" gap=1"));
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=e use=hut theme=t north_of=d gap=1",
            "`place id=e` spans (6, 0, -4) to (8, 2, -2) and shares (6, 0, -3) to (8, 2, -2) \
             with `place id=y`",
            vec![y_spans, (None, fix.as_str())],
        )],
    );
    let (source, out) = lowered_site(&north_chain(" gap=2"));
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=e use=hut theme=t north_of=d gap=2",
            "`place id=e` spans (6, 0, -5) to (8, 2, -3) and shares (6, 0, -3) to (8, 2, -3) \
             with `place id=y`",
            vec![y_spans, (None, fix.as_str())],
        )],
    );
    assert_eq!(primaries_of(&north_chain(" gap=3")), Vec::<String>::new());
}

#[test]
fn boxes_that_touch_and_rows_of_two_sites_do_not_overlap() {
    assert_eq!(
        primaries_of(
            "site s:\n  \
             place id=a use=hut theme=t at=origin\n  \
             place id=b use=hut theme=t east_of=a\n  \
             place id=c use=hut theme=t north_of=a\n\n\
             site r:\n  \
             place id=p use=eaved theme=t at=origin\n",
        ),
        Vec::<String>::new(),
    );
}

#[test]
fn rows_of_a_later_site_are_compared_past_the_rows_of_an_earlier_one() {
    let (p_row, q_row) = (
        "place id=p use=hut theme=t at=origin",
        "place id=q use=hut theme=t at=origin",
    );
    let (source, out) = lowered_site(&format!(
        "site s:\n  place id=a use=hut theme=t at=origin\n\nsite r:\n  {p_row}\n  {q_row}\n",
    ));
    let fix = at_origin_fix();
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            q_row,
            "`place id=q` spans (0, 0, 0) to (2, 2, 2) and shares (0, 0, 0) to (2, 2, 2) with \
             `place id=p`",
            vec![
                (Some(p_row), "`place id=p` spans (0, 0, 0) to (2, 2, 2)"),
                (None, fix.as_str()),
            ],
        )],
    );
}

/// A walkway from `a` to `b` that rounds the floor of the eaved `c` under its eaves, with `extra`
/// rows placed before `c`.
fn under_eaves(extra: &str) -> String {
    format!(
        "site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         {extra}\
         place id=c use=eaved theme=t east_of=a\n  \
         place id=b use=hut theme=t east_of=c\n  \
         connect a.front to b.front path=@cobblestone\n",
    )
}

const UNDER_EAVES_CONNECT: &str = "connect a.front to b.front path=@cobblestone";

/// The finding on the strip [`under_eaves`] lays under `c`'s eaves, `fix` its last note.
fn strip_under_c(fix: &str) -> Overlap<'_> {
    (
        "E_PLACEMENT_OVERLAP",
        UNDER_EAVES_CONNECT,
        "walkway `a.front ↔ b.front` lays 5 cells inside `place id=c`, within (3, 0, 4) to \
         (7, 0, 4)",
        vec![
            (
                Some("place id=c use=eaved theme=t east_of=a"),
                "`place id=c` spans (3, 0, 0) to (7, 3, 4)",
            ),
            (None, fix),
        ],
    )
}

#[test]
fn a_walkway_routed_under_the_eaves_of_a_third_placement_is_reported() {
    // `o` stands off the route and comes first, so `c` is reached past a placement with no strip.
    let (source, out) = lowered_site(&under_eaves("place id=o use=hut theme=t north_of=a\n  "));
    let fix = strip_fix("c", "a.front", "b.front");
    assert_eq!(overlaps(&source, &out), [strip_under_c(&fix)]);
}

#[test]
fn a_walkway_under_the_eaves_of_two_third_placements_is_reported_for_each() {
    let (source, out) = lowered_site(
        "site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=c use=eaved theme=t east_of=a\n  \
         place id=d use=eaved theme=t east_of=c\n  \
         place id=b use=hut theme=t east_of=d\n  \
         connect a.front to b.front path=@cobblestone\n",
    );
    let (c_fix, d_fix) = (
        strip_fix("c", "a.front", "b.front"),
        strip_fix("d", "a.front", "b.front"),
    );
    assert_eq!(
        overlaps(&source, &out),
        [
            (
                "E_PLACEMENT_OVERLAP",
                UNDER_EAVES_CONNECT,
                "walkway `a.front ↔ b.front` lays 5 cells inside `place id=c`, within \
                 (3, 0, 4) to (7, 0, 4)",
                vec![
                    (
                        Some("place id=c use=eaved theme=t east_of=a"),
                        "`place id=c` spans (3, 0, 0) to (7, 3, 4)",
                    ),
                    (None, c_fix.as_str()),
                ],
            ),
            (
                "E_PLACEMENT_OVERLAP",
                UNDER_EAVES_CONNECT,
                "walkway `a.front ↔ b.front` lays 5 cells inside `place id=d`, within \
                 (8, 0, 4) to (12, 0, 4)",
                vec![
                    (
                        Some("place id=d use=eaved theme=t east_of=c"),
                        "`place id=d` spans (8, 0, 0) to (12, 3, 4)",
                    ),
                    (None, d_fix.as_str()),
                ],
            ),
        ],
    );
}

#[test]
fn floors_of_another_site_on_the_route_leave_the_finding_as_it_is() {
    // `p` floors over `a`'s port and every cell the strip lays under `c`'s eaves.
    let (source, out) = lowered_site(&format!(
        "{}\nsite r:\n  place id=p use=yard theme=t at=origin\n",
        under_eaves(""),
    ));
    let fix = strip_fix("c", "a.front", "b.front");
    assert_eq!(overlaps(&source, &out), [strip_under_c(&fix)]);
}

#[test]
fn the_cells_a_blocked_walkway_skips_are_not_compared() {
    // `c` floors over `a`'s back port, so the row falls back to the straight L through `c` and `b`.
    let (source, out) = lowered_site(
        "site s:\n  \
         place id=a use=rear theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n  \
         place id=c use=hut theme=t north_of=a gap=0\n  \
         connect a.back to b.front path=@cobblestone\n",
    );
    assert_eq!(
        overlaps(&source, &out),
        [(
            "W_WALKWAY_BLOCKED",
            "connect a.back to b.front path=@cobblestone",
            "walkway `a.back ↔ b.front` skipped 5 cells that overlapped an existing structure",
            vec![(
                None,
                "port `a.back` is buried inside the floor of placement `c`; move that door/window \
                 to an unobstructed wall or pull the placements apart",
            )],
        )],
    );
}

#[test]
fn a_walkway_through_the_eaves_of_its_own_ends_or_another_site_is_not_reported() {
    assert_eq!(
        primaries_of(
            "site s:\n  \
             place id=a use=eaved theme=t at=origin\n  \
             place id=b use=eaved theme=t east_of=a\n  \
             connect a.front to b.front path=@cobblestone\n\n\
             site r:\n  \
             place id=p use=eaved theme=t at=origin\n",
        ),
        Vec::<String>::new(),
    );
}
