//! Top-level `edit` lines applied to the Intent IR before lowering, and the members their changes
//! ripple into (`spec/components-editing-sites` "Editing model").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::findings_without_target;
use cairn_lang_core::intent::{IntentModule, Member};
use cairn_lang_core::{Diagnostic, lower, parse};

use crate::common::{Finding, findings, lowered_with, notes, slice};
use crate::connections::Rules;

/// `source` parsed and lowered to the Intent IR, its edits applied.
fn edited(source: &str) -> IntentModule {
    lower(&parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}")))
}

/// Every member of every `struct` and `def` body, at any depth, a member before its children.
fn members(ir: &IntentModule) -> Vec<&Member> {
    fn walk<'a>(members: &'a [Member], out: &mut Vec<&'a Member>) {
        for member in members {
            out.push(member);
            walk(&member.children.members, out);
        }
    }
    let mut out = Vec::new();
    for s in &ir.structs {
        walk(&s.members, &mut out);
    }
    for d in &ir.defs {
        walk(&d.members, &mut out);
    }
    out
}

/// The member whose line starts with `line`.
fn member<'a>(source: &str, ir: &'a IntentModule, line: &str) -> &'a Member {
    members(ir)
        .into_iter()
        .find(|m| source[m.span.clone()].starts_with(line))
        .unwrap_or_else(|| panic!("no member line starts with {line:?}"))
}

/// What each `key=` of `member` holds, as the text written, and where it went: `state`,
/// `argument`, or the hoisted `class` and `mat_slot`.
fn fields<'a>(source: &'a str, member: &'a Member) -> Vec<(&'static str, &'a str, &'a str)> {
    let mut out: Vec<(&'static str, &str, &str)> = Vec::new();
    if let Some(class) = &member.class {
        out.push(("class", "class", class.as_str()));
    }
    if let Some(slot) = &member.mat_slot {
        out.push(("mat_slot", "mat_slot", &source[slot.span.clone()]));
    }
    for (key, value) in member.arguments.iter() {
        out.push(("argument", key.as_str(), &source[value.span.clone()]));
    }
    for (key, value) in member.intent_state.iter() {
        out.push(("state", key.as_str(), &source[value.span.clone()]));
    }
    out
}

/// The first line of the text each of `ir`'s edits selected, per edit.
fn selected<'a>(source: &'a str, ir: &IntentModule) -> Vec<Vec<&'a str>> {
    ir.edits
        .iter()
        .map(|edit| {
            edit.selected
                .iter()
                .map(|span| source[span.clone()].lines().next().unwrap_or_default())
                .collect()
        })
        .collect()
}

#[test]
fn an_edit_writes_each_key_where_its_line_would_have_put_it() {
    let source = "struct s size=9x7\n  \
                  walls height=3\n  \
                  door id=entry side=front at=center\n  \
                  window class=small side=front offset=1 y=1\n\
                  edit door[id=entry] set hinge=right side=back\n\
                  edit window@front[0] set class=vent mat_slot=glass y=2\n\
                  edit window@front[0] set class=5\n";
    let ir = edited(source);
    assert_eq!(
        fields(source, member(source, &ir, "door")),
        [
            ("argument", "side", "back"),
            ("argument", "at", "center"),
            ("state", "hinge", "right"),
        ],
    );
    // A label-shaped `class=` is the member's class; any other value stays an argument.
    assert_eq!(
        fields(source, member(source, &ir, "window")),
        [
            ("mat_slot", "mat_slot", "glass"),
            ("argument", "side", "front"),
            ("argument", "offset", "1"),
            ("argument", "y", "2"),
            ("argument", "class", "5"),
        ],
    );
    let hinge = &member(source, &ir, "door").intent_state["hinge"];
    assert!(hinge.span.start > source.find("edit door").expect("edit line"));
}

#[test]
fn each_edit_selects_among_the_members_as_written_and_the_later_one_wins() {
    let source = "struct s size=9x7\n  \
                  window class=a side=front offset=1 y=1\n  \
                  window class=b side=front offset=5 y=1\n\
                  edit window[class=a] set class=b y=2\n\
                  edit window[class=b] set y=3\n\
                  edit window@front[0] set y=4\n";
    let ir = edited(source);
    assert_eq!(
        selected(source, &ir),
        [
            vec!["window class=a side=front offset=1 y=1"],
            vec!["window class=b side=front offset=5 y=1"],
            vec!["window class=a side=front offset=1 y=1"],
        ],
    );
    assert_eq!(
        fields(source, member(source, &ir, "window class=a")),
        [
            ("class", "class", "b"),
            ("argument", "side", "front"),
            ("argument", "offset", "1"),
            ("argument", "y", "4"),
        ],
    );
    assert_eq!(
        fields(source, member(source, &ir, "window class=b"))[3],
        ("argument", "y", "3"),
    );
}

#[test]
fn a_level_attribute_selects_the_members_under_a_level_of_that_id() {
    let source = "struct s size=9x7\n  \
                  window side=front offset=1 y=1\n  \
                  level id=floor2 y=4\n    \
                  window side=front offset=2 y=1\n    \
                  window side=front offset=3 y=1\n  \
                  level y=8\n    \
                  window side=front offset=4 y=1\n\
                  edit window[level=floor2] set shape=arch\n\
                  edit window[level=\"floor2\"][offset=3] set y=2\n\
                  edit window[level=floor3] set y=2\n";
    let ir = edited(source);
    assert_eq!(
        selected(source, &ir),
        [
            vec![
                "window side=front offset=2 y=1",
                "window side=front offset=3 y=1",
            ],
            vec!["window side=front offset=3 y=1"],
            vec![],
        ],
    );
}

#[test]
fn an_edit_reaches_def_bodies_and_neither_site_rows_nor_actuator_patches() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  def hut size=5x5:\n  \
                  walls mat_slot=wall height=3\n  \
                  door id=entry side=front at=center\n  \
                  door[id=entry] opened_by=sig.open\n\n\
                  site v:\n  \
                  place id=a use=hut theme=t at=origin\n\n\
                  edit door[id=entry] set hinge=right\n\
                  edit place[id=a] set gap=2\n";
    let ir = edited(source);
    assert_eq!(
        selected(source, &ir),
        [vec!["door id=entry side=front at=center"], vec![]],
    );
    let place = &ir.sites[0].placements[0];
    assert!(place.arguments.get("gap").is_none());
}

#[test]
fn an_edit_moves_no_address_and_selects_by_the_written_one() {
    let source = "struct s size=9x7\n  \
                  door id=entry side=front at=center\n  \
                  door side=back at=left\n\
                  edit door[id=entry] set side=back\n\
                  edit door@back[1] set hinge=right\n\
                  edit door@front[0] set at=right\n";
    let ir = edited(source);
    let addresses: Vec<String> = members(&ir)
        .iter()
        .map(|m| {
            m.address
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(addresses, ["door@front[0]", "door@back[0]"]);
    assert_eq!(
        selected(source, &ir),
        [
            vec!["door id=entry side=front at=center"],
            vec![],
            vec!["door id=entry side=front at=center"],
        ],
    );
}

#[test]
fn an_edit_leaves_out_the_keys_it_may_not_set() {
    let source = "struct s size=9x7\n  \
                  window side=front offset=1 y=1\n\
                  edit window@front[0] set id=w shap=arch y=2\n";
    let ir = edited(source);
    let window = member(source, &ir, "window");
    assert_eq!(window.id, None);
    assert_eq!(
        fields(source, window),
        [
            ("argument", "side", "front"),
            ("argument", "offset", "1"),
            ("argument", "y", "2"),
        ],
    );
}

#[test]
fn a_member_an_edit_leaves_unbuildable_is_refused_at_its_line_with_the_edit_noted() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  walls mat_slot=wall height=3\n  \
                  door id=entry side=front at=center\n\
                  edit door[id=entry] set at=middle\n";
    let out = lowered_with(source, &Rules);
    let found = findings(source, &out);
    let expected: Vec<Finding<'_>> = vec![(
        DiagnosticCode::InvalidMember,
        "door id=entry side=front at=center",
        "door has `at=middle`, which is not one of center, left, right",
        vec![
            "name the door's column with `at=center`, `at=left` or `at=right`, or with `offset=N` \
             counted from 0 at the wall's left end seen from outside; `offset=` takes a column \
             between the wall's corners, from 1 to 5",
            "this edit changes this member",
        ],
    )];
    assert_eq!(found, expected);
    let edit = out.diagnostics[0].notes[1]
        .span
        .clone()
        .map(|span| &source[span]);
    assert_eq!(edit, Some("edit door[id=entry] set at=middle"));
}

#[test]
fn the_intent_ir_lists_each_edit_with_its_target_as_written() {
    let source = "struct s size=9x7\n  window side=front offset=1 y=1\n\
                  edit window@front[0] set y=2\n";
    let json = serde_json::to_value(edited(source)).expect("serializes");
    assert_eq!(
        json["edits"][0]["target"],
        serde_json::json!({ "kind": "Address", "address": "window@front[0]" }),
    );
    assert_eq!(
        json["edits"][0]["set"],
        serde_json::json!({ "y": { "kind": "Int", "value": 2 } }),
    );
    let without = serde_json::to_value(edited("struct s size=9x7\n  floor\n")).expect("serializes");
    assert!(without.get("edits").is_none(), "{without}");
}

/// The last note of a `W_EDIT_RIPPLE` on the member at `address`.
fn advice(address: &str) -> String {
    format!(
        "a member's resolved state follows the members around it, as a roof sits on its walls and \
         a chest pairs with the chest beside it: if the change is meant, the build can stand as it \
         is; if not, change the edit so it leaves what `{address}` follows as it was, or write the \
         state `{address}` should keep on its own line, where its keyword takes it"
    )
}

/// `source`'s findings lowered with no target for either edition, as `cairn check` reports them
/// without `--target`.
fn without_target(source: &str) -> Vec<Diagnostic> {
    findings_without_target(&edited(source), None, &|_, _| true)
}

/// A 7x5 struct with 3-row walls under a flat roof one voxel past them, `members` after the roof
/// and `edits` after the struct.
fn storeroom(members: &str, edits: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  {members}\n\n{edits}"
    )
}

#[test]
fn an_edit_that_parts_two_chests_ripples_into_the_one_it_left() {
    let source = storeroom(
        "chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1",
        "edit chest@inside.back[1] set offset=4\n",
    );
    let out = lowered_with(&source, &Rules);
    let expected: Vec<Finding<'_>> = vec![(
        DiagnosticCode::EditRipple,
        "chest at=inside.back offset=1 y=1",
        "no edit selects `chest@inside.back[0]`, and the edits change its resolved state at 1 cell",
        vec![
            "first at (6, 1, 2) of `struct::s`, which holds `minecraft:chest[facing=south,type=left]` \
             as written and `minecraft:chest[facing=south,type=single]` with the edits",
            "this edit changes a member of the same body",
            "a member's resolved state follows the members around it, as a roof sits on its \
             walls and a chest pairs with the chest beside it: if the change is meant, the build \
             can stand as it is; if not, change the edit so it leaves what `chest@inside.back[0]` \
             follows as it was, or write the state `chest@inside.back[0]` should keep on its own \
             line, where its keyword takes it",
        ],
    )];
    assert_eq!(findings(&source, &out), expected);
    let edit = &out.diagnostics[0].notes[1];
    assert_eq!(
        edit.span.clone().map(|span| &source[span]),
        Some("edit chest@inside.back[1] set offset=4"),
    );
}

#[test]
fn an_edit_that_changes_only_the_members_it_selects_ripples_into_none() {
    let chests = "chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1";
    for edits in [
        // Both chests move together, and nothing else meets them.
        "edit chest[at=inside.back] set y=2\n",
        // The roof's wider overhang shifts every other member by the same step.
        "edit roof[0] set overhang=2\n",
        "edit walls[0] set mat_slot=wall\n",
    ] {
        let source = storeroom(chests, edits);
        let ir = edited(&source);
        assert!(
            ir.edits.iter().all(|edit| !edit.selected.is_empty()),
            "{edits}"
        );
        let out = lowered_with(&source, &Rules);
        assert_eq!(findings(&source, &out), [], "{edits}");
    }
    // The wider overhang widens the box, so the comparison runs on shifted cells.
    let widths: Vec<u32> = ["", "edit roof[0] set overhang=2\n"]
        .into_iter()
        .map(|edits| {
            lowered_with(&storeroom(chests, edits), &Rules).structures["struct::s"]
                .dims
                .x
        })
        .collect();
    assert_eq!(widths, [9, 11]);
}

#[test]
fn with_no_target_only_a_ripple_in_where_blocks_stand_is_seen() {
    let source = storeroom("", "edit walls[0] set height=4\n").replace("overhang=1", "overhang=0");
    let out = without_target(&source);
    let found: Vec<Finding<'_>> = out
        .iter()
        .map(|d| (d.code, slice(&source, d), d.primary.as_str(), notes(d)))
        .collect();
    let expected: Vec<Finding<'_>> = vec![(
        DiagnosticCode::EditRipple,
        "roof kind=flat mat_slot=wall overhang=0",
        "no edit selects `roof[0]`, and the edits change its resolved state at 50 cells",
        vec![
            "first at (1, 4, 1) of `struct::s`, which holds `minecraft:cobblestone` as written and \
             none of its blocks with the edits",
            "this edit changes a member of the same body",
            "a member's resolved state follows the members around it, as a roof sits on its \
             walls and a chest pairs with the chest beside it: if the change is meant, the build \
             can stand as it is; if not, change the edit so it leaves what `roof[0]` follows as \
             it was, or write the state `roof[0]` should keep on its own line, where its keyword \
             takes it",
        ],
    )];
    assert_eq!(found, expected);
    // A chest pairs by the pack's tables, which a lowering with no target reads none of.
    let source = storeroom(
        "chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1",
        "edit chest@inside.back[1] set offset=4\n",
    );
    assert_eq!(without_target(&source), []);
}

#[test]
fn an_edit_selects_every_member_it_reaches_in_each_body_wherever_it_stands() {
    let source = "edit window[] set y=2\n\n\
                  struct a size=9x7\n  \
                  window side=front offset=1 y=1\n  \
                  level y=4\n    \
                  window side=front offset=2 y=1\n  \
                  @edition java\n    \
                  raw_block mat=minecraft:stone at=1,1,1\n\n\
                  struct b size=9x7\n  \
                  window side=front offset=3 y=1\n\n\
                  edit level[0].window@front[0] set y=2\n\
                  edit window@front[0] set y=3\n\
                  edit raw_block[0] set at=2,1,1\n";
    let ir = edited(source);
    assert_eq!(
        selected(source, &ir),
        [
            vec![
                "window side=front offset=1 y=1",
                "window side=front offset=2 y=1",
                "window side=front offset=3 y=1",
            ],
            vec!["window side=front offset=2 y=1"],
            vec![
                "window side=front offset=1 y=1",
                "window side=front offset=3 y=1",
            ],
            vec!["raw_block mat=minecraft:stone at=1,1,1"],
        ],
    );
    // A window under a `level` carries the level's step, so the bare address names none.
    let nested = "struct a size=9x7\n  level y=4\n    window side=front offset=2 y=1\n\
                  edit window@front[0] set y=2\n";
    assert_eq!(selected(nested, &edited(nested)), [Vec::<&str>::new()]);
}

#[test]
fn a_bracketed_member_line_is_selected_and_an_actuator_patch_is_not() {
    let source = "struct s size=7x5\n  \
                  window[class=a] side=front offset=2 y=1 size=1x1\n  \
                  window side=back offset=2 y=1 size=1x1\n  \
                  door id=entry side=left at=center\n  \
                  door[id=entry] opened_by=sig.open\n\
                  edit window[] set y=2\n\
                  edit window@front[0] set y=2\n\
                  edit door[] set hinge=right\n";
    let ir = edited(source);
    assert_eq!(
        selected(source, &ir),
        [
            vec![
                "window[class=a] side=front offset=2 y=1 size=1x1",
                "window side=back offset=2 y=1 size=1x1",
            ],
            vec!["window[class=a] side=front offset=2 y=1 size=1x1"],
            vec!["door id=entry side=left at=center"],
        ],
    );
}

#[test]
fn a_value_that_is_no_label_empties_the_field_its_key_names() {
    let source = "struct s size=9x7\n  \
                  window class=small mat_slot=glass side=front offset=1 y=1\n  \
                  window class=5 side=front offset=5 y=1\n\
                  edit window@front[0] set class=5 mat_slot=5\n\
                  edit window@front[1] set class=vent\n";
    let ir = edited(source);
    assert_eq!(
        fields(source, member(source, &ir, "window class=small")),
        [
            ("argument", "side", "front"),
            ("argument", "offset", "1"),
            ("argument", "y", "1"),
            ("argument", "class", "5"),
            ("argument", "mat_slot", "5"),
        ],
    );
    assert_eq!(
        fields(source, member(source, &ir, "window class=5")),
        [
            ("class", "class", "vent"),
            ("argument", "side", "front"),
            ("argument", "offset", "5"),
            ("argument", "y", "1"),
        ],
    );
}

#[test]
fn a_finding_on_a_member_two_edits_change_notes_each_edit() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  walls mat_slot=wall height=3\n  \
                  door id=entry side=front at=center\n\
                  edit door[id=entry] set at=middle\n\
                  edit door@front[0] set hinge=right\n";
    let out = lowered_with(source, &Rules);
    let edits: Vec<Vec<Option<&str>>> = out
        .diagnostics
        .iter()
        .map(|d| {
            d.notes
                .iter()
                .filter(|note| note.message == "this edit changes this member")
                .map(|note| note.span.clone().map(|span| &source[span]))
                .collect()
        })
        .collect();
    assert_eq!(
        edits,
        [vec![
            Some("edit door[id=entry] set at=middle"),
            Some("edit door@front[0] set hinge=right"),
        ]],
    );
}

/// Two chests side by side against the back wall, the first of them under a `level` of
/// `id=up` at `y=0`, then `edits`.
fn paired_across_a_level(edits: &str) -> String {
    storeroom(
        "chest at=inside.back offset=1 y=1\n  level id=up y=0\n    chest at=inside.back offset=2 y=1",
        edits,
    )
}

#[test]
fn a_ripple_notes_an_edit_of_a_member_that_holds_no_cell() {
    let source = paired_across_a_level("edit level[id=up] set y=1\n");
    let out = lowered_with(&source, &Rules);
    let advice = advice("chest@inside.back[0]");
    let expected: Vec<Finding<'_>> = vec![(
        DiagnosticCode::EditRipple,
        "chest at=inside.back offset=1 y=1",
        "no edit selects `chest@inside.back[0]`, and the edits change its resolved state at 1 cell",
        vec![
            "first at (6, 1, 2) of `struct::s`, which holds \
             `minecraft:chest[facing=south,type=left]` as written and \
             `minecraft:chest[facing=south,type=single]` with the edits",
            "this edit changes a member of the same body",
            &advice,
        ],
    )];
    assert_eq!(findings(&source, &out), expected);
    let edit = &out.diagnostics[0].notes[1];
    assert_eq!(edit.message, "this edit changes a member of the same body");
    assert_eq!(
        edit.span.clone().map(|span| &source[span]),
        Some("edit level[id=up] set y=1"),
    );
}

#[test]
fn the_members_under_a_selected_member_move_with_it_and_ripple_into_none() {
    let source = "theme t:\n  slot wall -> @cobblestone\n  slot glass -> @glass\n\n\
                  struct s size=7x5\n  walls mat_slot=wall height=6\n  \
                  level id=up y=2\n    \
                  window side=front offset=2 y=1 size=1x1 mat_slot=glass\n\
                  edit level[id=up] set y=3\n";
    let out = lowered_with(source, &Rules);
    assert_eq!(findings(source, &out), []);
    assert_eq!(without_target(source), []);
}

#[test]
fn a_def_placed_twice_ripples_once_in_each_structure() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  def hut size=7x5:\n  walls mat_slot=wall height=3\n  \
                  roof kind=flat mat_slot=wall overhang=1\n  \
                  chest at=inside.back offset=1 y=1\n  \
                  chest at=inside.back offset=2 y=1\n\n\
                  site v:\n  \
                  place id=a use=hut theme=t at=origin\n  \
                  place id=b use=hut theme=t east_of=a\n\n\
                  edit chest@inside.back[1] set offset=4\n";
    let out = lowered_with(source, &Rules);
    let found: Vec<(DiagnosticCode, &str, &str)> = out
        .diagnostics
        .iter()
        .map(|d| (d.code, slice(source, d), d.notes[0].message.as_str()))
        .collect();
    let chest = "chest at=inside.back offset=1 y=1";
    assert_eq!(
        found,
        [
            (
                DiagnosticCode::EditRipple,
                chest,
                "first at (6, 1, 2) of `site::v::a`, which holds \
                 `minecraft:chest[facing=south,type=left]` as written and \
                 `minecraft:chest[facing=south,type=single]` with the edits",
            ),
            (
                DiagnosticCode::EditRipple,
                chest,
                "first at (6, 1, 2) of `site::v::b`, which holds \
                 `minecraft:chest[facing=south,type=left]` as written and \
                 `minecraft:chest[facing=south,type=single]` with the edits",
            ),
        ],
    );
}

#[test]
fn a_member_an_edit_leaves_unbuildable_ripples_from_the_build_as_written() {
    let source = storeroom(
        "pressure_plate id=plate at=front.outside offset=3 y=0",
        "edit roof[0] set overhang=0\n",
    );
    let out = lowered_with(&source, &Rules);
    let advice = advice("pressure_plate@front.outside[0]");
    let expected: Vec<Finding<'_>> = vec![
        (
            DiagnosticCode::InvalidMember,
            "pressure_plate id=plate at=front.outside offset=3 y=0",
            "pressure_plate `at=front.outside` at y=0 has no exterior voxel to sit on",
            vec![
                "no roof on this struct draws an overhang, so it has no voxel outside its walls",
                "give a `roof` in the struct's body an `overhang=` of at least 1, or use \
                 `at=inside.front` with an `offset=` from 1 to 5",
            ],
        ),
        (
            DiagnosticCode::EditRipple,
            "pressure_plate id=plate at=front.outside offset=3 y=0",
            "no edit selects `pressure_plate@front.outside[0]`, and the edits change its resolved \
             state at 1 cell",
            vec![
                "first at (3, 0, 5) of `struct::s`, which holds `minecraft:oak_pressure_plate` as \
                 written and none of its blocks with the edits",
                "this edit changes a member of the same body",
                &advice,
            ],
        ),
    ];
    assert_eq!(findings(&source, &out), expected);
}

#[test]
fn a_ripple_notes_only_the_edits_of_its_own_body() {
    let source = format!(
        "{}\nstruct b size=5x5\n  floor mat_slot=wall\n\nedit floor[0] set mat_slot=wall\n",
        storeroom(
            "chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1",
            "edit chest@inside.back[1] set offset=4\n",
        ),
    );
    let out = lowered_with(&source, &Rules);
    let notes: Vec<Vec<Option<&str>>> = out
        .diagnostics
        .iter()
        .map(|d| {
            d.notes
                .iter()
                .filter_map(|note| note.span.clone())
                .map(|span| Some(&source[span]))
                .collect()
        })
        .collect();
    assert_eq!(
        notes,
        [vec![Some("edit chest@inside.back[1] set offset=4")]]
    );
}

#[test]
fn with_no_target_two_stand_ins_hold_the_same_block() {
    let source = format!(
        "{}\nedit door[id=front] set hinge=right\n",
        include_str!("../../../../examples/redstone-door.crn"),
    );
    assert_eq!(without_target(&source), []);
}
