//! The address the Intent IR gives each member of a `struct` or `def` body (`spec/syntax` "IDs,
//! classes, addresses").

use cairn_lang_core::intent::{AddressSegment, MemberAddress, ScopeKind};
use cairn_lang_core::{lower, parse};

/// Each member line's first line of text with its address, in the order
/// `IntentModule::addressed_members` lists them.
fn addresses(src: &str) -> Vec<(String, String)> {
    let module = parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    lower(&module)
        .addressed_members()
        .into_iter()
        .map(|(_, _, member)| {
            let line = src[member.span.clone()].lines().next().unwrap_or_default();
            let address = member.address.as_ref().map(ToString::to_string);
            (line.trim().to_owned(), address.unwrap_or_default())
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

#[test]
fn members_of_one_role_on_one_side_count_along_the_wall() {
    let src = "struct s size=9x7\n  \
               window side=front offset=6\n  \
               window side=front offset=2\n  \
               window side=back offset=1\n  \
               window side=front offset=4\n  \
               walls height=3\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("window side=front offset=6", "window@front[2]"),
            ("window side=front offset=2", "window@front[0]"),
            ("window side=back offset=1", "window@back[0]"),
            ("window side=front offset=4", "window@front[1]"),
            ("walls height=3", "walls[0]"),
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
    assert_eq!(written, {
        let mut expected = pairs(&[
            ("window side=front offset=6", "window@front[2]"),
            ("window side=front offset=2 y=3", "window@front[1]"),
            ("window side=front offset=2 y=1", "window@front[0]"),
            ("door side=front at=left", "door@front[0]"),
            ("door side=front at=right", "door@front[1]"),
            ("chest at=inside.back offset=2", "chest@inside.back[0]"),
            ("fill from=1,1,1 to=3,1,3", "fill[1]"),
            ("fill from=0,0,0 to=2,0,2", "fill[0]"),
        ]);
        expected.sort();
        expected
    },);
}

#[test]
fn appending_a_member_moves_no_address_before_it() {
    let before = "struct s size=9x7\n  \
                  walls height=3\n  \
                  window side=front offset=2\n  \
                  door side=front at=center\n";
    let after = format!(
        "{before}  window side=front offset=7\n  window side=back offset=0\n  \
         door side=back at=left\n  roof kind=flat\n"
    );
    let kept = sorted_addresses(before);
    let grown = sorted_addresses(&after);
    for pair in &kept {
        assert!(grown.contains(pair), "{pair:?} moved: {grown:?}");
    }
    assert_eq!(grown.len(), kept.len() + 4);
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
               door side=front at=5\n  \
               door side=front at=center\n  \
               door side=front at=3\n  \
               door side=front at=left\n  \
               door side=left at=center\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("door side=front at=right", "door@front[4]"),
            ("door side=front at=5", "door@front[3]"),
            ("door side=front at=center", "door@front[2]"),
            ("door side=front at=3", "door@front[1]"),
            ("door side=front at=left", "door@front[0]"),
            ("door side=left at=center", "door@left[0]"),
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
fn lines_at_one_place_are_ordered_by_what_they_write() {
    let first = "struct s size=9x7\n  \
                 window class=b side=front offset=2\n  \
                 window class=a side=front offset=2\n  \
                 window id=z side=front offset=2\n";
    let expected = pairs(&[
        ("window class=a side=front offset=2", "window@front[0]"),
        ("window class=b side=front offset=2", "window@front[1]"),
        ("window id=z side=front offset=2", "window@front[2]"),
    ]);
    assert_eq!(sorted_addresses(first), expected);
    let swapped = "struct s size=9x7\n  \
                   window id=z side=front offset=2\n  \
                   window class=a side=front offset=2\n  \
                   window class=b side=front offset=2\n";
    assert_eq!(sorted_addresses(swapped), expected);
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
               raw_block mat=minecraft:stone at=2,2,2\n  \
               @edition java\n    \
               raw_block mat=minecraft:glass at=1,1,1\n";
    assert_eq!(
        addresses(src),
        pairs(&[
            ("raw_block mat=minecraft:stone at=2,2,2", "raw_block[1]"),
            ("raw_block mat=minecraft:glass at=1,1,1", "raw_block[0]"),
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
    let listed: Vec<(ScopeKind, &str, Option<String>)> = ir
        .addressed_members()
        .into_iter()
        .map(|(kind, name, member)| (kind, name, member.address.as_ref().map(ToString::to_string)))
        .collect();
    assert_eq!(
        listed,
        [
            (ScopeKind::Struct, "s", Some("floor[0]".to_owned())),
            (ScopeKind::Def, "porch", Some("floor[0]".to_owned())),
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
fn an_address_holds_each_step_from_the_body_down() {
    let src = "struct s size=9x7\n  level y=0\n    door side=front at=center\n";
    let module = parse(src).expect("parses");
    let ir = lower(&module);
    let door = &ir.structs[0].members[0].children.members[0];
    let level = AddressSegment {
        role: "level".to_owned(),
        side: None,
        index: 0,
    };
    let step = AddressSegment {
        role: "door".to_owned(),
        side: Some("front".to_owned()),
        index: 0,
    };
    let expected = MemberAddress::default().child(level).child(step);
    assert_eq!(door.address.as_ref(), Some(&expected));
    assert_eq!(expected.segments().len(), 2);
    assert_eq!(expected.to_string(), "level[0].door@front[0]");
}
