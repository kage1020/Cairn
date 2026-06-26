//! AST → Intent IR lowering acceptance tests.
//!
//! Two layers:
//! 1. Insta snapshots over every shipped example, fixing the IR shape so any
//!    behavioural drift in `lower` shows up as a snapshot diff.
//! 2. Unit tests that pin down the hoisting rules (`id` / `class` /
//!    `mat_slot`, size, nested children, unknown-keyword fallback) without
//!    depending on a snapshot file.

use std::num::NonZeroU32;

use cairn_lang_core::ast::ValueKind;
use cairn_lang_core::intent::{MemberRole, SemanticLevel};
use cairn_lang_core::{lower, parse};

fn lower_source(src: &str) -> cairn_lang_core::IntentModule {
    let module = parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    lower(&module)
}

fn lower_example(filename: &str) -> cairn_lang_core::IntentModule {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(filename);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    lower_source(&source)
}

#[test]
fn lowers_cottage() {
    insta::assert_yaml_snapshot!(lower_example("cottage.crn"));
}

#[test]
fn lowers_themed_tower() {
    insta::assert_yaml_snapshot!(lower_example("themed-tower.crn"));
}

#[test]
fn lowers_village() {
    insta::assert_yaml_snapshot!(lower_example("village.crn"));
}

#[test]
fn lowers_redstone_door() {
    insta::assert_yaml_snapshot!(lower_example("redstone-door.crn"));
}

#[test]
fn m2_lower_always_returns_grouped_semantic_level() {
    let ir = lower_source("struct s size=1x1\n  floor\n");
    assert_eq!(ir.semantic_level, SemanticLevel::Grouped);
}

#[test]
fn struct_size_hoists_out_of_header_args() {
    let ir = lower_source("struct cottage size=9x7\n  floor\n");
    let s = ir
        .structs
        .first()
        .expect("one struct expected after lowering");
    let size = s
        .size
        .as_ref()
        .expect("size header should hoist to StructIr.size");
    assert_eq!(size.w, NonZeroU32::new(9).unwrap());
    assert_eq!(size.h, NonZeroU32::new(7).unwrap());
    assert!(
        !s.args.contains_key("size"),
        "size must not leak into the residual header args map"
    );
}

#[test]
fn struct_body_classifies_floor_keyword_as_role_floor() {
    let ir = lower_source("struct s size=1x1\n  floor\n");
    let members = &ir.structs[0].members;
    assert_eq!(members.len(), 1, "expected exactly one body member");
    assert!(matches!(members[0].role, MemberRole::Floor));
}

#[test]
fn unknown_keyword_lands_in_member_role_other_without_failing() {
    let ir = lower_source("struct s size=1x1\n  mystery_block foo=1\n");
    let members = &ir.structs[0].members;
    assert_eq!(members.len(), 1);
    match &members[0].role {
        MemberRole::Other(keyword) => assert_eq!(keyword, "mystery_block"),
        other => panic!("unknown keyword should fall back to Other, got {other:?}"),
    }
}

#[test]
fn id_class_and_mat_slot_hoist_into_typed_fields() {
    let ir = lower_source(
        "struct s size=1x1\n  walls id=outer_wall class=outer mat_slot=wall height=4\n",
    );
    let member = &ir.structs[0].members[0];
    assert_eq!(member.id.as_deref(), Some("outer_wall"));
    assert_eq!(member.class.as_deref(), Some("outer"));
    assert_eq!(member.mat_slot.as_deref(), Some("wall"));
    assert!(
        !member.intent_state.fields.contains_key("id")
            && !member.intent_state.fields.contains_key("class")
            && !member.intent_state.fields.contains_key("mat_slot"),
        "hoisted keys must not also remain in intent_state"
    );
    assert!(
        member.intent_state.fields.contains_key("height"),
        "non-hoisted keys must remain in intent_state"
    );
}

#[test]
fn site_place_use_is_preserved_in_intent_state() {
    let ir = lower_source("site hamlet\n  place use=cottage\n");
    let placement = ir
        .sites
        .first()
        .expect("site")
        .placements
        .first()
        .expect("placement");
    assert!(matches!(placement.role, MemberRole::Place));
    let use_value = placement
        .intent_state
        .fields
        .get("use")
        .expect("use= must survive lowering");
    assert!(
        matches!(&use_value.value.kind, ValueKind::Ident(s) if s == "cottage"),
        "use= should preserve its identifier payload, got {use_value:?}",
    );
}

#[test]
fn level_block_recursively_lowers_its_children() {
    let ir =
        lower_source("struct tower size=5x5\n  level y=0\n    floor\n    walls mat_slot=stone\n");
    let level = &ir.structs[0].members[0];
    assert!(matches!(level.role, MemberRole::Level));
    assert_eq!(
        level.children.members.len(),
        2,
        "two nested members expected"
    );
    assert!(matches!(level.children.members[0].role, MemberRole::Floor));
    assert!(matches!(level.children.members[1].role, MemberRole::Walls));
    assert!(
        level.children.logic.is_empty() && level.children.asserts.is_empty(),
        "no logic / assert in this fixture"
    );
}

#[test]
fn level_block_keeps_nested_logic_and_asserts() {
    // Regression for the PR #20 review: a nested `logic` or `assert` used
    // to either panic via `unreachable!()` or be silently dropped because
    // `Member.children` was `Vec<Member>`. The MemberBody shape preserves
    // all three statement flavours.
    let src = "\
struct gate size=3x3
  level y=0
    pressure_plate id=plate at=front.outside -> sig.step
    logic sig.open = sig.step
    assert always(sig.step -> eventually sig.open within 2)
";
    let ir = lower_source(src);
    let level = &ir.structs[0].members[0];
    assert!(matches!(level.role, MemberRole::Level));
    assert_eq!(level.children.members.len(), 1);
    assert_eq!(level.children.logic.len(), 1);
    assert_eq!(level.children.asserts.len(), 1);
}

#[test]
fn duplicate_size_does_not_leak_into_residual_args() {
    // Regression for the PR #20 review: a repeated `size=` used to land in
    // `StructIr::args`, contradicting that field's documented "everything
    // except size" contract.
    let ir = lower_source("struct s size=4x4 size=5x5\n  floor\n");
    let s = &ir.structs[0];
    let size = s.size.as_ref().expect("first size= still hoists");
    assert_eq!(size.w.get(), 4);
    assert_eq!(size.h.get(), 4);
    assert!(
        !s.args.contains_key("size"),
        "duplicate size= must not leak into residual args"
    );
}

#[test]
fn token_and_dotref_are_not_hoisted_into_label_fields() {
    // Regression: hoist_label used to accept Token and DotRef silently,
    // swallowing diagnostics the `check::type_mismatch` pass needs to
    // report.
    let ir = lower_source("struct s size=1x1\n  walls id=@oak class=foo.bar mat_slot=wall\n");
    let member = &ir.structs[0].members[0];
    assert!(
        member.id.is_none(),
        "@token must not coerce into a string id"
    );
    assert!(
        member.class.is_none(),
        "dotted refs must not coerce into a string class"
    );
    assert_eq!(
        member.mat_slot.as_deref(),
        Some("wall"),
        "plain idents still hoist"
    );
    assert!(
        member.intent_state.contains_key("id") && member.intent_state.contains_key("class"),
        "unhoisted id/class values must stay in intent_state for the PR2 mismatch pass"
    );
}

#[test]
fn pressure_plate_binding_arrow_is_kept_separate_from_intent_state() {
    let ir = lower_source(
        "struct gate size=3x3\n  pressure_plate id=plate at=front.outside -> sig.step\n",
    );
    let member = &ir.structs[0].members[0];
    assert!(matches!(member.role, MemberRole::PressurePlate));
    assert_eq!(member.id.as_deref(), Some("plate"));
    assert!(member.binding.is_some(), "-> binding must survive lowering");
    assert!(
        !member.intent_state.fields.contains_key("->"),
        "the arrow tail must never become a synthetic intent_state key"
    );
}
