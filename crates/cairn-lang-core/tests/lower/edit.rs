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
        "door has `at=middle`, which is not one of center, left, right — use `at=center | left | \
         right`",
        vec!["this edit changes this member"],
    )];
    assert_eq!(found, expected);
    let edit = out.diagnostics[0].notes[0]
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
        let out = lowered_with(&source, &Rules);
        assert_eq!(findings(&source, &out), [], "{edits}");
    }
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
