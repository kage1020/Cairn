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
                       roof kind=flat mat_slot=wall overhang=1\n\n";

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

const AT_ORIGIN_FIX: &str = "two placements of one site may share no cell: each placement is \
                             written with air in every cell its body does not fill, so whichever \
                             of the two is placed second decides what those cells hold; this \
                             row's `at=origin` puts it on the world origin, which no `gap=` moves; \
                             place it `east_of=` or `north_of=` the row it should stand beside";

#[test]
fn a_row_overlapping_earlier_rows_is_reported_once_for_each() {
    let (a_row, b_row, c_row) = (
        "place id=a use=hut theme=t at=origin",
        "place id=b use=hut theme=t at=origin",
        "place id=c use=hut theme=t at=origin",
    );
    let (source, out) = lowered_site(&format!("site s:\n  {a_row}\n  {b_row}\n  {c_row}\n"));
    assert_eq!(
        overlaps(&source, &out),
        [
            (
                "E_PLACEMENT_OVERLAP",
                b_row,
                "`place id=b` spans (0, 0, 0) to (2, 2, 2) and overlaps `place id=a` from \
                 (0, 0, 0) to (2, 2, 2)",
                vec![
                    (Some(a_row), "`place id=a` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, AT_ORIGIN_FIX),
                ],
            ),
            (
                "E_PLACEMENT_OVERLAP",
                c_row,
                "`place id=c` spans (0, 0, 0) to (2, 2, 2) and overlaps `place id=a` from \
                 (0, 0, 0) to (2, 2, 2)",
                vec![
                    (Some(a_row), "`place id=a` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, AT_ORIGIN_FIX),
                ],
            ),
            (
                "E_PLACEMENT_OVERLAP",
                c_row,
                "`place id=c` spans (0, 0, 0) to (2, 2, 2) and overlaps `place id=b` from \
                 (0, 0, 0) to (2, 2, 2)",
                vec![
                    (Some(b_row), "`place id=b` spans (0, 0, 0) to (2, 2, 2)"),
                    (None, AT_ORIGIN_FIX),
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
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=d use=hut theme=t east_of=a",
            "`place id=d` spans (3, 0, 0) to (5, 2, 2) and overlaps `place id=c` from \
             (3, 0, 0) to (5, 2, 2)",
            vec![
                (
                    Some("place id=c use=hall theme=t east_of=b"),
                    "`place id=c` spans (3, 0, -3) to (5, 2, 5)",
                ),
                (
                    None,
                    "two placements of one site may share no cell: each placement is written \
                     with air in every cell its body does not fill, so whichever of the two is \
                     placed second decides what those cells hold; write `gap=3` on this row to \
                     move it, and every row placed relative to it, east of `place id=c`",
                ),
            ],
        )],
    );
}

#[test]
fn the_gap_an_east_of_overlap_names_clears_it_and_one_less_does_not() {
    assert_eq!(primaries_of(&east_chain(" gap=3")), Vec::<String>::new());
    assert_eq!(
        primaries_of(&east_chain(" gap=2")),
        [
            "`place id=d` spans (5, 0, 0) to (7, 2, 2) and overlaps `place id=c` from \
          (5, 0, 0) to (5, 2, 2)"
        ],
    );
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
    let (source, out) = lowered_site(&north_chain(" gap=1"));
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "place id=e use=hut theme=t north_of=d gap=1",
            "`place id=e` spans (6, 0, -4) to (8, 2, -2) and overlaps `place id=y` from \
             (6, 0, -3) to (8, 2, -2)",
            vec![
                (
                    Some("place id=y use=wing theme=t north_of=a"),
                    "`place id=y` spans (0, 0, -3) to (8, 2, -1)",
                ),
                (
                    None,
                    "two placements of one site may share no cell: each placement is written \
                     with air in every cell its body does not fill, so whichever of the two is \
                     placed second decides what those cells hold; write `gap=3` on this row to \
                     move it, and every row placed relative to it, north of `place id=y`",
                ),
            ],
        )],
    );
    assert_eq!(primaries_of(&north_chain(" gap=3")), Vec::<String>::new());
    assert_eq!(primaries_of(&north_chain(" gap=2")).len(), 1);
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
fn a_walkway_routed_under_the_eaves_of_a_third_placement_is_reported() {
    let (source, out) = lowered_site(
        "site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=c use=eaved theme=t east_of=a\n  \
         place id=b use=hut theme=t east_of=c\n  \
         connect a.front to b.front path=@cobblestone\n",
    );
    assert_eq!(
        overlaps(&source, &out),
        [(
            "E_PLACEMENT_OVERLAP",
            "connect a.front to b.front path=@cobblestone",
            "walkway `a.front ↔ b.front` lays 5 cells inside `place id=c`, from (3, 0, 4) to \
             (7, 0, 4)",
            vec![
                (
                    Some("place id=c use=eaved theme=t east_of=a"),
                    "`place id=c` spans (3, 0, 0) to (7, 3, 4)",
                ),
                (
                    None,
                    "a walkway may lay no cell inside a placement other than the two it joins: \
                     each placement is written with air in every cell its body does not fill, \
                     so whichever of the two is placed second decides what those cells hold, \
                     its own block or the walkway's; move `place id=c` clear of the strip",
                ),
            ],
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
