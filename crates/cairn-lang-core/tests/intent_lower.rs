//! AST → Intent IR lowering acceptance tests.
//!
//! Two layers:
//! 1. Insta snapshots over every shipped example, fixing the IR shape so any
//!    behavioural drift in `lower` shows up as a snapshot diff.
//! 2. Unit tests that pin down the hoisting rules (`id` / `class` /
//!    `mat_slot`, size, nested children, unknown-keyword fallback) without
//!    depending on a snapshot file.

use std::fmt::Write as _;
use std::num::NonZeroU32;

use cairn_lang_core::ast::ValueKind;
use cairn_lang_core::intent::{MemberRole, SemanticLevel};
use cairn_lang_core::{lower, parse};

mod common;
use common::examples;

fn lower_source(src: &str) -> cairn_lang_core::IntentModule {
    let module = parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    lower(&module)
}

/// One snapshot per shipped example, named `lowers_<stem>` so the fixture
/// a diff belongs to is in its file name.
#[test]
fn every_shipped_example_lowers_to_its_snapshot() {
    for (name, source) in examples() {
        let stem = name.trim_end_matches(".crn").replace('-', "_");
        insta::assert_yaml_snapshot!(format!("lowers_{stem}"), lower_source(&source));
    }
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
    // Regression: a nested `logic` or `assert` used to either panic via
    // `unreachable!()` or be silently dropped because `Member.children`
    // was `Vec<Member>`. The MemberBody shape preserves all three
    // statement flavours.
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
    // Regression: a repeated `size=` used to land in `StructIr::args`,
    // contradicting that field's documented "everything except size"
    // contract.
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
        "unhoisted id/class values must stay in intent_state for the selector mismatch pass"
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

/// One `circuit` line that reserves nothing: where it is written, and
/// what [`cairn_lang_core::circuit_lines`] says about it.
struct RejectedLine {
    /// `size=` header of the scope, or empty for none.
    size: &'static str,
    /// How many `level` blocks the line is nested under.
    levels: usize,
    line: &'static str,
    is: fn(&cairn_lang_core::CircuitRegionDefect) -> bool,
    /// The defect's `Display` form, the reason clause a pass prints.
    reason: &'static str,
}

/// `keyword s{size}` with a `floor` and `line` at the given depth of
/// `level` nesting.
fn scope_with_line(keyword: &str, size: &str, levels: usize, line: &str) -> String {
    let mut src = format!("{keyword} s{size}\n  floor\n");
    let mut indent = String::from("  ");
    for y in 0..levels {
        let _ = writeln!(src, "{indent}level y={y}");
        indent.push_str("  ");
    }
    let _ = writeln!(src, "{indent}{line}");
    src
}

/// One case per [`cairn_lang_core::CircuitRegionDefect`] variant, and
/// a second `level` deep for the one that is about nesting.
fn rejected_lines() -> [RejectedLine; 10] {
    use cairn_lang_core::CircuitRegionDefect as Defect;

    let sized = " size=5x5";
    [
        RejectedLine {
            size: sized,
            levels: 1,
            line: "circuit region=floor void=2",
            is: |d| matches!(d, Defect::NestedUnderLevel { .. }),
            reason: "it is written under a `level`, and only a `circuit` line at the scope's top level is read as a reservation",
        },
        RejectedLine {
            size: sized,
            levels: 2,
            line: "circuit region=floor void=2",
            is: |d| matches!(d, Defect::NestedUnderLevel { .. }),
            reason: "it is written under a `level`, and only a `circuit` line at the scope's top level is read as a reservation",
        },
        RejectedLine {
            size: "",
            levels: 0,
            line: "circuit region=floor void=2",
            is: |d| matches!(d, Defect::NoSize { .. }),
            reason: "the enclosing scope has no `size=WxH` header for it to reserve within",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit void=2",
            is: |d| matches!(d, Defect::RegionMissing { .. }),
            reason: "it has no `region=`",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=3 void=2",
            is: |d| {
                matches!(
                    d,
                    Defect::RegionNotLabel {
                        found: "integer",
                        ..
                    }
                )
            },
            reason: "its `region=` must be an identifier or string label, got integer",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=\"\" void=2",
            is: |d| matches!(d, Defect::RegionEmpty { .. }),
            reason: "its `region=` is an empty label",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor",
            is: |d| matches!(d, Defect::VoidMissing { .. }),
            reason: "it has no `void=`",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=deep",
            is: |d| {
                matches!(
                    d,
                    Defect::VoidNotInteger {
                        found: "identifier",
                        ..
                    }
                )
            },
            reason: "its `void=` must be an integer, got identifier",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=0",
            is: |d| matches!(d, Defect::VoidBelowOne { value: 0, .. }),
            reason: "its `void=0` reserves no service layer",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=4294967296",
            is: |d| {
                matches!(
                    d,
                    Defect::VoidTooLarge {
                        value: 4_294_967_296,
                        ..
                    }
                )
            },
            reason: "its `void=4294967296` is over the limit of 4294967295",
        },
    ]
}

/// Every `circuit` line that reserves nothing comes back as rejected,
/// with the reason it does not, the sentence that reason prints as, and
/// its own line as the span, so a pass that finds a scope with no
/// reservation can point at the line that was meant to be one.
///
/// Each case runs under a `struct` and under a `def`: the two are walked
/// by separate loops, and a table that put `size=` on one keyword only
/// would exercise `NoSize` on one and every other defect on the other.
#[test]
fn each_circuit_line_that_reserves_nothing_is_handed_back_with_its_reason() {
    use cairn_lang_core::{ScopeKind, circuit_lines, circuit_regions};

    for case in &rejected_lines() {
        for (keyword, kind) in [("struct", ScopeKind::Struct), ("def", ScopeKind::Def)] {
            let src = scope_with_line(keyword, case.size, case.levels, case.line);
            let ir = lower_source(&src);

            assert!(
                circuit_regions(&ir).is_empty(),
                "reserves nothing:\n{src}{:?}",
                circuit_regions(&ir),
            );
            let lines = circuit_lines(&ir);
            let [Err(rejected)] = lines.as_slice() else {
                panic!("handed back once, as rejected:\n{src}{lines:?}");
            };
            assert!((case.is)(&rejected.defect), "{src}{:?}", rejected.defect);
            assert_eq!(rejected.defect.to_string(), case.reason, "{src}");
            assert_eq!(rejected.scope_kind, kind, "{src}");
            assert_eq!(rejected.scope_name, "s", "{src}");
            assert_eq!(
                &src[rejected.span.clone()],
                case.line,
                "the span is the line:\n{src}"
            );
        }
    }
}

/// A usable `circuit` line is a reservation and is not handed back as a
/// rejected one, while a rejected line beside it still is, each in its
/// place in source order.
#[test]
fn a_usable_circuit_line_is_not_handed_back_as_rejected() {
    use cairn_lang_core::{CircuitRegionDefect, circuit_lines, circuit_regions};

    let ir = lower_source(
        "struct s size=5x5\n  floor\n  circuit region=floor void=0\n  circuit region=floor void=2\n",
    );
    let regions = circuit_regions(&ir);
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0].void, 2);
    let lines = circuit_lines(&ir);
    let [Err(rejected), Ok(region)] = lines.as_slice() else {
        panic!("the rejected line, then the usable one: {lines:?}");
    };
    assert!(
        matches!(
            rejected.defect,
            CircuitRegionDefect::VoidBelowOne { value: 0, .. }
        ),
        "{:?}",
        rejected.defect,
    );
    assert_eq!(region, &regions[0]);
}
