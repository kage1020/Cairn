//! Acceptance tests for the `positional` pass of
//! `cairn_lang_core::check`.
//!
//! `spec/syntax.md` §5.1 requires `key=value` for everything after the
//! command keyword and prints `window front G 2 2 2x2` as the forbidden
//! form. Nothing enforced it: the parser appends any token that is not
//! `key=`, `-> binding`, or `[selector]` to the statement's `positional`
//! list, and every reader except `connect`'s ignores that list. The
//! author got the member they wrote minus the arguments they meant, with
//! `cairn check` reporting nothing.
//!
//! The shape that makes this more than a style rule is a dropped `=`.
//! `walls mat_slot=wall height 3` is one keystroke from correct, and
//! there is no default to fall back on: `wall_height` refuses a member
//! with no positive `height=`, so the wall is not built at all. Before
//! this pass, `cairn check` exited 0 on that line and only `cairn lower`
//! said anything — about the missing `height=`, never about the two bare
//! values that explain it.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse, resolve};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn positional_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::UnexpectedPositional)
        .collect()
}

fn one(source: &str) -> Diagnostic {
    let mut found = positional_only(source);
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

fn struct_with(row: &str) -> String {
    format!("{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  {row}\n")
}

/// `run` walks `ir.structs` and `ir.defs` in separate loops, so a
/// struct-only suite leaves the `def` loop unexecuted.
fn def_with(row: &str) -> String {
    format!("{PRELUDE}def lodge size=5x5:\n  floor mat_slot=floor\n  {row}\n")
}

fn duo_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

/// The spec's own forbidden example.
#[test]
fn po_1_the_spec_forbidden_form_is_an_error() {
    for src in [
        struct_with("window front G 2 2 2x2 mat_slot=wall"),
        def_with("window front G 2 2 2x2 mat_slot=wall"),
    ] {
        let d = one(&src);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            d.primary,
            "`window` reads only `key=value` arguments: the 5 bare values on this line are dropped",
        );
    }
}

/// A dropped `=` lands in the same list. This is the shape that changes
/// a build rather than only offending the style rule: `height 3` reads as
/// two bare values, and `walls` with no `height=` is not built at all —
/// there is no default to fall back on.
#[test]
fn po_2_a_dropped_equals_sign_is_reported() {
    let src = struct_with("walls mat_slot=wall height 3");
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "height 3");
}

/// `connect FROM.PORT to TO.PORT` is the one form that reads
/// positionals, so this pass never looks at it — including when the row
/// is malformed, which is `connect_arity`'s finding and carries advice
/// about the endpoint shape rather than about `key=value`.
#[test]
fn po_3_connect_is_exempt_well_formed_or_not() {
    for row in [
        "connect anchor.entry to peer.entry path=@gravel",
        "connect anchor.entry to peer.entry c.exit path=@gravel",
        "connect anchor.entry xxx peer.entry path=@gravel",
        "connect anchor to peer.entry path=@gravel",
    ] {
        let src = duo_with(row);
        assert!(
            positional_only(&src).is_empty(),
            "`{row}` is `connect_arity`'s, got {:#?}",
            positional_only(&src),
        );
    }
    let over_arity = duo_with("connect anchor.entry to peer.entry c.exit path=@gravel");
    assert!(
        diagnose(&over_arity)
            .iter()
            .any(|d| d.code == DiagnosticCode::ConnectArity),
        "the arity pass still owns the over-arity row",
    );
}

/// The parser appends to `positional` whenever the next token is not
/// `key=`, so a bare value can sit *after* an argument. The AST field's
/// own doc used to say positionals are consumed before the first
/// `key=value`; the loop never had that ordering.
#[test]
fn po_4_a_positional_after_an_argument_is_still_reported() {
    let src = struct_with("window id=w side=front y=1 offset=1 size=2 x2 mat_slot=wall");
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "x2");
}

/// The span reaches from the first bare value to the last, so a run
/// split by an argument underlines the whole run. The argument caught
/// in between is part of the line the author rewrites.
#[test]
fn po_5_the_span_covers_the_whole_run() {
    let src = struct_with("window front side=front G mat_slot=wall");
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "front side=front G");
}

/// Negative space: a line whose arguments are all keyed says nothing.
#[test]
fn po_6_a_fully_keyed_line_is_not_reported() {
    for row in [
        "floor mat_slot=floor",
        "walls mat_slot=wall height=3",
        "window id=w side=front y=1 offset=1 size=1x1",
        "level y=0",
    ] {
        let src = struct_with(row);
        assert!(
            positional_only(&src).is_empty(),
            "`{row}` is well-formed, got {:#?}",
            positional_only(&src),
        );
    }
}

/// An unknown keyword is `keyword_allowlist`'s finding: there is no
/// reader whose argument form the bare values could be measured against,
/// and the repair is the word.
#[test]
fn po_7_an_unknown_keyword_is_left_to_the_allowlist() {
    let src = struct_with("frame front 2");
    assert!(
        positional_only(&src).is_empty(),
        "got {:#?}",
        positional_only(&src),
    );
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownKeyword),
    );
}

/// Where a line sits does not change whether its own values are read,
/// so this pass keeps descending through a subtree `member_scope` and
/// `nesting` have already reported: dedenting the row leaves it just as
/// broken.
#[test]
fn po_8_a_positional_inside_a_dropped_body_is_still_reported() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n    window front G mat_slot=wall\n"
    );
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "front G");
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnsupportedNesting),
        "the dropped body is still reported by `nesting` — the two \
         findings have different repairs",
    );
}

/// Singular and plural both appear in the message, and only a rendered
/// string shows that the verb agrees with the count.
#[test]
fn po_9_the_verb_agrees_with_the_count() {
    let one_value = one(&struct_with("roof flat mat_slot=wall"));
    assert!(
        one_value
            .primary
            .ends_with("the 1 bare value on this line is dropped"),
        "got {:?}",
        one_value.primary,
    );
    let two_values = one(&struct_with("roof flat gable mat_slot=wall"));
    assert!(
        two_values
            .primary
            .ends_with("the 2 bare values on this line are dropped"),
        "got {:?}",
        two_values.primary,
    );
}

/// A `place` row takes the same treatment as a geometry row: `place hut`
/// (the `use=` key forgotten) is a plausible typo and drops the whole
/// placement.
#[test]
fn po_10_a_site_row_is_covered_too() {
    let src = duo_with("place extra theme=plain at=origin");
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "extra");
}

/// The premise the message is written on, measured on the build: a
/// `walls` line whose `height=` lost its `=` is not built shorter, it is
/// not built at all. Without this the doc comments above are a claim
/// about lowering that no test in this file exercises.
#[test]
fn po_11_a_dropped_equals_costs_the_whole_member() {
    let solid = |source: &str| {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        let built = lower_to_block_array(&ir, &resolution, None);
        built.structures["struct::s"]
            .voxels
            .iter()
            .filter(|c| c.0 != 0)
            .count()
    };
    let keyed = solid(&struct_with("walls mat_slot=wall height=3"));
    let dropped = solid(&struct_with("walls mat_slot=wall height 3"));
    let no_walls = solid(&struct_with("floor mat_slot=floor"));
    assert!(
        keyed > dropped,
        "the wall should be missing: keyed={keyed} dropped={dropped}",
    );
    assert_eq!(
        dropped, no_walls,
        "missing entirely, not shortened — there is no default height",
    );
}
