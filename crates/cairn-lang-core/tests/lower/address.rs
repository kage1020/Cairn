//! The address the Intent IR gives each member of a `struct` or `def` body (`spec/syntax` "IDs,
//! classes, addresses").

use cairn_lang_core::intent::ScopeKind;
use cairn_lang_core::{lower, parse};

/// Each member line's first line of text with its address, in the order
/// `IntentModule::addressed_members` lists them.
fn addresses(src: &str) -> Vec<(String, String)> {
    let module = parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    lower(&module)
        .addressed_members()
        .into_iter()
        .map(|found| {
            let line = src[found.member.span.clone()]
                .lines()
                .next()
                .unwrap_or_default();
            (line.trim().to_owned(), found.address.to_string())
        })
        .collect()
}

/// [`addresses`], sorted by line, for comparing two sources that write the same lines.
fn sorted_addresses(src: &str) -> Vec<(String, String)> {
    let mut found = addresses(src);
    found.sort();
    found
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(line, address)| ((*line).to_owned(), (*address).to_owned()))
        .collect()
}

/// [`pairs`], sorted, to compare with [`sorted_addresses`].
fn sorted_pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut expected = pairs(expected);
    expected.sort();
    expected
}

#[test]
fn members_of_one_role_on_one_side_count_along_the_wall() {
    let src = "struct s size=12x7\n  \
               window side=front offset=6\n  \
               window side=front offset=10\n  \
               window side=back offset=1\n  \
               window side=front offset=2\n  \
               window side=back\n  \
               walls height=3\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("window side=front offset=6", "window@front[1]"),
            ("window side=front offset=10", "window@front[2]"),
            ("window side=back offset=1", "window@back[1]"),
            ("window side=front offset=2", "window@front[0]"),
            ("window side=back", "window@back[0]"),
            ("walls height=3", "walls[0]"),
        ]),
    );
}

#[test]
fn members_at_one_column_count_up_the_wall() {
    let src = "struct s size=9x12\n  \
               window side=front offset=2 y=10\n  \
               window side=front offset=2\n  \
               window side=front offset=2 y=9\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("window side=front offset=2 y=10", "window@front[2]"),
            ("window side=front offset=2", "window@front[0]"),
            ("window side=front offset=2 y=9", "window@front[1]"),
        ]),
    );
}

#[test]
fn reordering_the_lines_moves_no_address() {
    let lines = [
        "window side=front offset=6",
        "window side=front offset=2 y=3",
        "window side=front offset=2 y=1",
        "door side=front at=left",
        "door side=front at=right",
        "chest at=inside.back offset=2",
        "fill from=1,1,1 to=3,1,3",
        "fill from=0,0,0 to=2,0,2",
    ];
    let body = |order: &[usize]| {
        let mut src = "struct s size=9x7\n".to_owned();
        for &i in order {
            src.push_str("  ");
            src.push_str(lines[i]);
            src.push('\n');
        }
        src
    };
    let written = sorted_addresses(&body(&[0, 1, 2, 3, 4, 5, 6, 7]));
    assert_eq!(written, sorted_addresses(&body(&[7, 6, 5, 4, 3, 2, 1, 0])));
    assert_eq!(written, sorted_addresses(&body(&[2, 5, 0, 7, 3, 1, 6, 4])));
    assert_eq!(
        written,
        sorted_pairs(&[
            ("window side=front offset=6", "window@front[2]"),
            ("window side=front offset=2 y=3", "window@front[1]"),
            ("window side=front offset=2 y=1", "window@front[0]"),
            ("door side=front at=left", "door@front[0]"),
            ("door side=front at=right", "door@front[1]"),
            ("chest at=inside.back offset=2", "chest@inside.back[0]"),
            ("fill from=1,1,1 to=3,1,3", "fill[1]"),
            ("fill from=0,0,0 to=2,0,2", "fill[0]"),
        ]),
    );

    let front = "  level y=0\n    window side=front offset=1 y=1 size=1x1 mat_slot=trim\n";
    let back = "  level y=0\n    window side=back offset=2 y=1 size=1x1 mat_slot=trim\n";
    let levels = |first: &str, second: &str| {
        format!("struct s size=9x7\n  walls mat_slot=wall height=4\n{first}{second}")
    };
    let expected = sorted_pairs(&[
        ("walls mat_slot=wall height=4", "walls[0]"),
        ("level y=0", "level[0]"),
        ("level y=0", "level[1]"),
        (
            "window side=front offset=1 y=1 size=1x1 mat_slot=trim",
            "level[1].window@front[0]",
        ),
        (
            "window side=back offset=2 y=1 size=1x1 mat_slot=trim",
            "level[0].window@back[0]",
        ),
    ]);
    assert_eq!(sorted_addresses(&levels(front, back)), expected);
    assert_eq!(sorted_addresses(&levels(back, front)), expected);
}

#[test]
fn appending_a_member_moves_no_address_before_it() {
    let before = "struct s size=9x7\n  \
                  walls height=3\n  \
                  window side=front offset=2\n  \
                  door side=front at=center\n";
    assert_eq!(
        addresses(before),
        pairs(&[
            ("walls height=3", "walls[0]"),
            ("window side=front offset=2", "window@front[0]"),
            ("door side=front at=center", "door@front[0]"),
        ]),
    );
    let after = format!(
        "{before}  window side=front offset=7\n  window side=back offset=0\n  \
         door side=back at=left\n  roof kind=flat\n"
    );
    assert_eq!(
        addresses(&after),
        pairs(&[
            ("walls height=3", "walls[0]"),
            ("window side=front offset=2", "window@front[0]"),
            ("door side=front at=center", "door@front[0]"),
            ("window side=front offset=7", "window@front[1]"),
            ("window side=back offset=0", "window@back[0]"),
            ("door side=back at=left", "door@back[0]"),
            ("roof kind=flat", "roof[0]"),
        ]),
    );
}

#[test]
fn a_member_added_ahead_of_its_kind_on_its_wall_renumbers_those_after_it() {
    let before = "struct s size=9x7\n  window side=front offset=4\n";
    assert_eq!(
        addresses(before),
        pairs(&[("window side=front offset=4", "window@front[0]")]),
    );
    let after = format!("{before}  window side=front offset=1\n");
    assert_eq!(
        addresses(&after),
        pairs(&[
            ("window side=front offset=4", "window@front[1]"),
            ("window side=front offset=1", "window@front[0]"),
        ]),
    );
}

#[test]
fn a_door_at_a_named_anchor_counts_by_the_column_it_names() {
    // On a 9-wide front `center` is column 4, so the door at 5 comes after it and the one at 3
    // before.
    let src = "struct s size=9x7\n  \
               door side=front at=right\n  \
               door side=front offset=5\n  \
               door side=front at=center\n  \
               door side=front offset=3\n  \
               door side=front at=left\n  \
               door side=left at=center\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door side=front at=right", "door@front[4]"),
            ("door side=front offset=5", "door@front[3]"),
            ("door side=front at=center", "door@front[2]"),
            ("door side=front offset=3", "door@front[1]"),
            ("door side=front at=left", "door@front[0]"),
            ("door side=left at=center", "door@left[0]"),
        ]),
    );
}

#[test]
fn an_anchor_is_measured_along_the_wall_it_stands_on() {
    // `center` on the 8-wide front is column 4, past the door at 3. On the 7-deep left wall it is
    // column 3 and `right` column 6, level with the doors there, and an anchor's line sorts first
    // by what it writes.
    let src = "struct s size=8x7\n  \
               door side=front at=center\n  \
               door side=front offset=3\n  \
               door side=left offset=6\n  \
               door side=left at=right\n  \
               door side=left offset=3\n  \
               door side=left at=center\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door side=front at=center", "door@front[1]"),
            ("door side=front offset=3", "door@front[0]"),
            ("door side=left offset=6", "door@left[3]"),
            ("door side=left at=right", "door@left[2]"),
            ("door side=left offset=3", "door@left[1]"),
            ("door side=left at=center", "door@left[0]"),
        ]),
    );
}

#[test]
fn a_numeric_at_names_no_column_and_sorts_after_every_one() {
    let src = "struct s size=9x7\n  \
               door side=front at=5\n  \
               door side=front at=right\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door side=front at=5", "door@front[1]"),
            ("door side=front at=right", "door@front[0]"),
        ]),
    );
}

#[test]
fn a_value_no_step_reads_sorts_after_every_number() {
    let src = "def porch\n  \
               door side=front at=center\n  \
               door side=front at=left\n  \
               window side=front offset=two\n  \
               window side=front offset=4\n  \
               window side=back y=high\n  \
               window side=back y=3\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door side=front at=center", "door@front[1]"),
            ("door side=front at=left", "door@front[0]"),
            ("window side=front offset=two", "window@front[1]"),
            ("window side=front offset=4", "window@front[0]"),
            ("window side=back y=high", "window@back[1]"),
            ("window side=back y=3", "window@back[0]"),
        ]),
    );
}

#[test]
fn cells_count_by_y_then_z_then_x() {
    let src = "struct s size=9x7\n  \
               raw_block mat=minecraft:stone at=0,1,0\n  \
               raw_block mat=minecraft:stone at=0,0,1\n  \
               raw_block mat=minecraft:stone at=1,0,0\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("raw_block mat=minecraft:stone at=0,1,0", "raw_block[2]"),
            ("raw_block mat=minecraft:stone at=0,0,1", "raw_block[1]"),
            ("raw_block mat=minecraft:stone at=1,0,0", "raw_block[0]"),
        ]),
    );
}

#[test]
fn boxes_count_by_their_low_corner_then_their_high_corner_whichever_end_is_written_first() {
    let src = "struct s size=9x7\n  \
               fill from=1,0,1 to=2,0,2\n  \
               fill from=3,0,3 to=0,0,0\n  \
               fill from=0,0,0 to=5,0,5\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("fill from=1,0,1 to=2,0,2", "fill[2]"),
            ("fill from=3,0,3 to=0,0,0", "fill[0]"),
            ("fill from=0,0,0 to=5,0,5", "fill[1]"),
        ]),
    );
}

#[test]
fn a_member_under_a_level_is_addressed_through_it() {
    let src = "struct s size=9x7\n  \
               level id=upper y=5\n    \
               window side=front offset=3\n    \
               window side=front offset=1\n  \
               level id=ground y=0\n    \
               window side=front offset=1\n  \
               window side=front offset=1\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("level id=upper y=5", "level[1]"),
            ("window side=front offset=3", "level[1].window@front[1]"),
            ("window side=front offset=1", "level[1].window@front[0]"),
            ("level id=ground y=0", "level[0]"),
            ("window side=front offset=1", "level[0].window@front[0]"),
            ("window side=front offset=1", "window@front[0]"),
        ]),
    );
}

#[test]
fn the_wall_a_line_names_is_its_side_as_written() {
    let src = "struct s size=9x7\n  \
               lever at=inside.front offset=2 y=1\n  \
               button at=front.outside offset=5 y=1\n  \
               painting side=inside.front offset=1 y=2\n  \
               chest at=inside.back offset=3\n  \
               chest at=inside.back offset=1\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            (
                "lever at=inside.front offset=2 y=1",
                "lever@inside.front[0]"
            ),
            (
                "button at=front.outside offset=5 y=1",
                "button@front.outside[0]"
            ),
            (
                "painting side=inside.front offset=1 y=2",
                "painting@inside.front[0]"
            ),
            ("chest at=inside.back offset=3", "chest@inside.back[1]"),
            ("chest at=inside.back offset=1", "chest@inside.back[0]"),
        ]),
    );
}

#[test]
fn lines_at_one_place_are_ordered_by_their_pairs_as_text() {
    let src = "struct s size=9x7\n  \
               walls height=9 mat_slot=wall\n  \
               walls height=10 mat_slot=wall\n  \
               walls height=9 mat_slot=wall frame=oak\n  \
               walls height=9 mat_slot=wall y=0\n  \
               walls height=9 mat_slot=wall frame=\"oak\"\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("walls height=9 mat_slot=wall", "walls[3]"),
            ("walls height=10 mat_slot=wall", "walls[2]"),
            ("walls height=9 mat_slot=wall frame=oak", "walls[1]"),
            ("walls height=9 mat_slot=wall y=0", "walls[4]"),
            ("walls height=9 mat_slot=wall frame=\"oak\"", "walls[0]"),
        ]),
    );
}

#[test]
fn a_number_is_compared_by_its_one_spelling() {
    let src = "struct s size=9x7\n  \
               walls height=9 mat_slot=wall scale=02.50\n  \
               walls height=9 mat_slot=wall scale=1.5\n  \
               walls height=9 mat_slot=wall scale=2.5\n  \
               walls height=9 mat_slot=wall scale=2.0\n  \
               walls height=9 mat_slot=wall scale=2\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("walls height=9 mat_slot=wall scale=02.50", "walls[3]"),
            ("walls height=9 mat_slot=wall scale=1.5", "walls[0]"),
            ("walls height=9 mat_slot=wall scale=2.5", "walls[4]"),
            ("walls height=9 mat_slot=wall scale=2.0", "walls[1]"),
            ("walls height=9 mat_slot=wall scale=2", "walls[2]"),
        ]),
    );
}

#[test]
fn lines_with_the_same_pairs_are_ordered_by_bare_values_then_binding_then_guard() {
    let stone = "raw_block mat=minecraft:stone at=1,1,1";
    let src = format!(
        "struct s size=5x5\n  \
         {stone} -> sig.b\n  \
         {stone} b\n  \
         {stone} -> sig.a\n  \
         {stone} a\n  \
         @edition java\n    \
         {stone}\n  \
         {stone}\n"
    );
    let lines = [
        format!("{stone} -> sig.b"),
        format!("{stone} b"),
        format!("{stone} -> sig.a"),
        format!("{stone} a"),
    ];
    assert_eq!(
        addresses(&src),
        pairs(&[
            (&lines[0], "raw_block[3]"),
            (&lines[1], "raw_block[5]"),
            (&lines[2], "raw_block[2]"),
            (&lines[3], "raw_block[4]"),
            (stone, "raw_block[1]"),
            (stone, "raw_block[0]"),
        ]),
    );
}

#[test]
fn adding_an_id_a_class_or_brackets_renumbers_nothing() {
    let body = |tagged: &str| {
        format!(
            "struct s size=9x7\n  \
             walls mat_slot=wall height=4\n  \
             {tagged} side=front offset=1 y=1 size=1x1 mat_slot=wall\n  \
             window side=front offset=1 y=1 size=1x1 mat_slot=trim\n"
        )
    };
    for tagged in ["window", "window id=w", "window class=c", "window[class=c]"] {
        let line = format!("{tagged} side=front offset=1 y=1 size=1x1 mat_slot=wall");
        assert_eq!(
            addresses(&body(tagged)),
            pairs(&[
                ("walls mat_slot=wall height=4", "walls[0]"),
                (&line, "window@front[1]"),
                (
                    "window side=front offset=1 y=1 size=1x1 mat_slot=trim",
                    "window@front[0]",
                ),
            ]),
            "{tagged}",
        );
    }
}

#[test]
fn members_that_tie_at_every_step_keep_the_order_their_lines_are_written_in() {
    let src = "struct s size=9x7\n  \
               window id=a side=front offset=1\n  \
               window id=b side=front offset=1\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("window id=a side=front offset=1", "window@front[0]"),
            ("window id=b side=front offset=1", "window@front[1]"),
        ]),
    );
    let swapped = "struct s size=9x7\n  \
                   window id=b side=front offset=1\n  \
                   window id=a side=front offset=1\n";
    assert_eq!(
        addresses(swapped),
        pairs(&[
            ("window id=b side=front offset=1", "window@front[0]"),
            ("window id=a side=front offset=1", "window@front[1]"),
        ]),
    );
}

#[test]
fn a_member_with_an_id_still_takes_its_place_in_the_count() {
    let src = "struct s size=9x7\n  \
               window id=main side=front offset=4\n  \
               window side=front offset=6\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("window id=main side=front offset=4", "window@front[0]"),
            ("window side=front offset=6", "window@front[1]"),
        ]),
    );
}

#[test]
fn a_member_under_an_edition_guard_is_counted_with_its_siblings() {
    let src = "struct s size=5x5\n  \
               @edition java\n    \
               raw_block mat=minecraft:glass at=2,2,2\n  \
               raw_block mat=minecraft:stone at=1,1,1\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("raw_block mat=minecraft:glass at=2,2,2", "raw_block[1]"),
            ("raw_block mat=minecraft:stone at=1,1,1", "raw_block[0]"),
        ]),
    );
}

#[test]
fn a_def_body_is_addressed_as_a_struct_body_is_and_a_site_row_is_not() {
    let src = "def porch size=3x2\n  \
               floor\n\n\
               struct s size=9x7\n  \
               floor\n\n\
               site v:\n  \
               place id=a use=s at=origin\n";
    let module = parse(src).expect("parses");
    let ir = lower(&module);
    let listed: Vec<(ScopeKind, &str, String)> = ir
        .addressed_members()
        .into_iter()
        .map(|found| {
            (
                found.scope_kind,
                found.scope_name,
                found.address.to_string(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (ScopeKind::Struct, "s", "floor[0]".to_owned()),
            (ScopeKind::Def, "porch", "floor[0]".to_owned()),
        ],
    );
    assert_eq!(ir.sites[0].placements[0].address, None);
}

#[test]
fn an_actuator_patch_takes_no_address_and_takes_no_place_in_the_count() {
    let src = "struct s size=9x7\n  \
               door[id=back] opened_by=sig.a\n  \
               door id=front side=front at=center\n  \
               door id=back side=back at=center\n";
    let module = parse(src).expect("parses");
    let ir = lower(&module);
    let members: Vec<Option<String>> = ir.structs[0]
        .members
        .iter()
        .map(|member| member.address.as_ref().map(ToString::to_string))
        .collect();
    assert_eq!(
        members,
        [
            None,
            Some("door@front[0]".to_owned()),
            Some("door@back[0]".to_owned()),
        ],
    );
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door id=front side=front at=center", "door@front[0]"),
            ("door id=back side=back at=center", "door@back[0]"),
        ]),
    );
}

#[test]
fn a_line_with_brackets_that_patches_no_actuator_is_addressed_as_any_other() {
    let src = "struct s size=9x7\n  \
               walls mat_slot=wall height=4\n  \
               window[class=outer] side=front offset=1 y=1 size=1x1 mat_slot=trim\n  \
               window side=front offset=3 y=1 size=1x1 mat_slot=trim\n  \
               level[class=a] y=0\n    \
               window side=back offset=2 y=1 size=1x1 mat_slot=trim\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("walls mat_slot=wall height=4", "walls[0]"),
            (
                "window[class=outer] side=front offset=1 y=1 size=1x1 mat_slot=trim",
                "window@front[0]",
            ),
            (
                "window side=front offset=3 y=1 size=1x1 mat_slot=trim",
                "window@front[1]",
            ),
            ("level[class=a] y=0", "level[0]"),
            (
                "window side=back offset=2 y=1 size=1x1 mat_slot=trim",
                "level[0].window@back[0]",
            ),
        ]),
    );
}

#[test]
fn an_address_holds_each_step_from_the_body_down() {
    let src = "struct s size=9x7\n  level y=0\n    door side=front at=center\n";
    let module = parse(src).expect("parses");
    let ir = lower(&module);
    let door = &ir.structs[0].members[0].children.members[0];
    let address = door.address.as_ref().expect("the door is addressed");
    let steps: Vec<(&str, Option<&str>, u32)> = address
        .segments()
        .iter()
        .map(|step| (step.role(), step.side(), step.index()))
        .collect();
    assert_eq!(steps, [("level", None, 0), ("door", Some("front"), 0)]);
    assert_eq!(address.to_string(), "level[0].door@front[0]");
}
