//! `E_UNEXPECTED_POSITIONAL`: everything after a command keyword is `key=value` (`spec/syntax`
//! "Lexical"), so a bare value on any line but a `connect` row is an error at the value.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{DiagnosticCode, Severity, lower, parse, resolve};

use crate::common::{PRELUDE, def_with, diagnose, of_code, one_of_code, struct_with};

fn duo_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

/// The spec's own forbidden example.
#[test]
fn the_spec_forbidden_form_is_an_error() {
    for src in [
        struct_with("window front G 2 2 2x2 mat_slot=wall"),
        def_with("window front G 2 2 2x2 mat_slot=wall"),
    ] {
        let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            d.primary,
            "`window` reads only `key=value` arguments: the 5 bare values on this line are dropped",
        );
    }
}

#[test]
fn a_dropped_equals_sign_is_reported() {
    let src = struct_with("walls mat_slot=wall height 3");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "height 3");
}

/// A malformed `connect` row is `connect_arity`'s.
#[test]
fn connect_is_exempt_well_formed_or_not() {
    for row in [
        "connect anchor.entry to peer.entry path=@gravel",
        "connect anchor.entry to peer.entry c.exit path=@gravel",
        "connect anchor.entry xxx peer.entry path=@gravel",
        "connect anchor to peer.entry path=@gravel",
    ] {
        let src = duo_with(row);
        assert!(
            of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
            "`{row}` is `connect_arity`'s, got {:#?}",
            of_code(&src, DiagnosticCode::UnexpectedPositional),
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

#[test]
fn a_positional_after_an_argument_is_still_reported() {
    let src = struct_with("window id=w side=front y=1 offset=1 size=2 x2 mat_slot=wall");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "x2");
}

#[test]
fn the_span_covers_the_whole_run() {
    let src = struct_with("window front side=front G mat_slot=wall");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "front side=front G");
}

#[test]
fn a_fully_keyed_line_is_not_reported() {
    for row in [
        "floor mat_slot=floor",
        "walls mat_slot=wall height=3",
        "window id=w side=front y=1 offset=1 size=1x1",
        "level y=0",
    ] {
        let src = struct_with(row);
        assert!(
            of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
            "`{row}` is well-formed, got {:#?}",
            of_code(&src, DiagnosticCode::UnexpectedPositional),
        );
    }
}

#[test]
fn an_unknown_keyword_is_left_to_the_allowlist() {
    let src = struct_with("frame front 2");
    assert!(
        of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::UnexpectedPositional),
    );
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownKeyword),
    );
}

/// `member_scope` and `nesting` have reported the subtree, but dedenting leaves the row as broken.
#[test]
fn a_positional_inside_a_dropped_body_is_still_reported() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n    window front G mat_slot=wall\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "front G");
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnsupportedNesting),
        "the dropped body is still reported by `nesting` — the two \
         findings have different repairs",
    );
}

#[test]
fn the_verb_agrees_with_the_count() {
    let one_value = one_of_code(
        &struct_with("roof flat mat_slot=wall"),
        DiagnosticCode::UnexpectedPositional,
    );
    assert!(
        one_value
            .primary
            .ends_with("the 1 bare value on this line is dropped"),
        "got {:?}",
        one_value.primary,
    );
    let two_values = one_of_code(
        &struct_with("roof flat gable mat_slot=wall"),
        DiagnosticCode::UnexpectedPositional,
    );
    assert!(
        two_values
            .primary
            .ends_with("the 2 bare values on this line are dropped"),
        "got {:?}",
        two_values.primary,
    );
}

#[test]
fn a_site_row_is_covered_too() {
    let src = duo_with("place extra theme=plain at=origin");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "extra");
}

/// `walls` has no default height, so a lost `=` costs the whole wall.
#[test]
fn a_dropped_equals_costs_the_whole_member() {
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

/// Each `E_UNEXPECTED_POSITIONAL` `source` raises, as the text it points at, its primary and its
/// notes.
fn positionals(source: &str) -> Vec<(&str, String, Vec<String>)> {
    of_code(source, DiagnosticCode::UnexpectedPositional)
        .into_iter()
        .map(|d| {
            let notes = d.notes.iter().map(|n| n.message.clone()).collect();
            (&source[d.span.clone()], d.primary, notes)
        })
        .collect()
}

/// A finding on a line other than a member line, with its one note.
fn finding<'a>(text: &'a str, primary: &str, note: &str) -> (&'a str, String, Vec<String>) {
    (text, primary.to_owned(), vec![note.to_owned()])
}

const KEYED_HEADER_NOTE: &str =
    "give each value the key it belongs to (`size=3x3`); a missing `=` lands here too";

#[test]
fn a_dropped_equals_sign_on_a_struct_or_def_header_is_reported() {
    for keyword in ["struct", "def"] {
        let src = format!("{PRELUDE}{keyword} s size 5x5\n  floor mat_slot=floor\n");
        assert_eq!(
            positionals(&src),
            [finding(
                "size 5x5",
                &format!(
                    "a `{keyword}` header reads only `key=value` arguments: the 2 bare values on \
                     this line are dropped"
                ),
                KEYED_HEADER_NOTE,
            )],
            "{keyword}",
        );
    }
}

#[test]
fn a_bare_value_beside_a_header_argument_is_reported() {
    let src = format!("{PRELUDE}struct s size=5x5 tall:\n  floor mat_slot=floor\n");
    assert_eq!(
        positionals(&src),
        [finding(
            "tall",
            "a `struct` header reads only `key=value` arguments: the 1 bare value on this line is \
             dropped",
            KEYED_HEADER_NOTE,
        )],
    );
}

#[test]
fn a_site_or_theme_header_reads_nothing_after_its_name() {
    let site =
        format!("{PRELUDE}site duo extra:\n  place id=anchor use=hut theme=plain at=origin\n");
    assert_eq!(
        positionals(&site),
        [finding(
            "extra",
            "a `site` header reads nothing after its name: the 1 bare value on this line is \
             dropped",
            "delete it: a `site` header is its name, with an optional `:` after it",
        )],
    );
    let theme = "theme dusk dark 2:\n  slot wall -> @stone\n";
    assert_eq!(
        positionals(theme),
        [finding(
            "dark 2",
            "a `theme` header reads nothing after its name: the 2 bare values on this line are \
             dropped",
            "delete them: a `theme` header is its name, with an optional `:` after it",
        )],
    );
}

#[test]
fn a_dropped_equals_sign_on_a_selector_rule_is_reported() {
    let src = "theme t:\n  window[side=front] -> mat_slot glass\n";
    assert_eq!(
        positionals(src),
        [finding(
            "mat_slot glass",
            "a `window[...]` rule binds only `key=value` arguments: the 2 bare values on this \
             line are dropped",
            "give each value the key it binds (`mat_slot=glass`); a missing `=` lands here too",
        )],
    );
}

#[test]
fn a_slot_row_binds_its_one_value() {
    let src = "theme t:\n  slot wall -> @stone @brick\n  slot roof -> @oak @birch @spruce\n";
    assert_eq!(
        positionals(src),
        [
            finding(
                "@brick",
                "a `slot` row binds one value: the 1 bare value after it on this line is dropped",
                "delete it, or bind it to a slot of its own on a `slot` row",
            ),
            finding(
                "@birch @spruce",
                "a `slot` row binds one value: the 2 bare values after it on this line are \
                 dropped",
                "delete them, or bind each to a slot of its own on a `slot` row",
            ),
        ],
    );
}

#[test]
fn an_assert_line_ends_at_its_form() {
    for (form, line, text, end) in [
        (
            "truth",
            "assert truth(sig.a -> sig.b) { 0 -> 1; 1 -> 0 } strict",
            "strict",
            "the `}` of its table",
        ),
        (
            "always",
            "assert always(sig.a -> eventually sig.b within 4) twice",
            "twice",
            "its `)`",
        ),
        (
            "latency",
            "assert latency(sig.a -> sig.b) <= 4 ticks",
            "ticks",
            "its bound",
        ),
    ] {
        assert_eq!(
            positionals(&struct_with(line)),
            [finding(
                text,
                &format!(
                    "an `assert {form}` line ends at {end}: the 1 bare value after it is dropped"
                ),
                "delete it",
            )],
            "{line}",
        );
    }
}

#[test]
fn a_logic_line_without_its_equals_sign_binds_nothing() {
    let line = "logic sig.lamps sig.power and not sig.day";
    let src = struct_with(line);
    assert_eq!(
        positionals(&src),
        [finding(
            line,
            "a `logic` line binds `NAME = EXPR`, and no `=` follows `sig.lamps`: the name and the \
             expression after it are bare values, and the line binds nothing",
            "write `=` between them: `logic sig.lamps = ...`",
        )],
    );
    let bound = |source: &str| lower(&parse(source).expect("parse")).structs[0].logic.len();
    assert_eq!(bound(&src), 0, "the line binds nothing");
    assert_eq!(
        bound(&struct_with("logic sig.lamps = sig.power and not sig.day")),
        1,
    );
}

#[test]
fn a_well_formed_header_rule_or_assertion_is_not_reported() {
    let src = format!(
        "{PRELUDE}theme t:\n  slot wall -> @stone\n  window[side=front] -> mat_slot=glass\n\n\
         site duo:\n  place id=anchor use=hut theme=plain at=origin\n\n\
         struct s size=5x5:\n  floor mat_slot=floor\n  logic sig.lamps = sig.power and sig.day\n  \
         assert truth(sig.a -> sig.b) {{ 0 -> 1; 1 -> 0 }}\n  \
         assert always(sig.a -> eventually sig.b within 4)\n  \
         assert latency(sig.a -> sig.b) <= 4\n"
    );
    assert_eq!(positionals(&src), []);
}

/// A line under a guard or a member is checked where it stands, whatever else it earns there.
#[test]
fn a_bare_value_on_a_nested_line_is_still_reported() {
    let src = struct_with(
        "@edition java\n    assert latency(sig.a -> sig.b) <= 4 ticks\n  \
         walls mat_slot=wall height=3\n    logic sig.a sig.b",
    );
    assert_eq!(
        of_code(&src, DiagnosticCode::UnexpectedPositional)
            .iter()
            .map(|d| &src[d.span.clone()])
            .collect::<Vec<_>>(),
        ["ticks", "logic sig.a sig.b"],
    );
}

/// The slips are findings rather than parse failures, so every one in the file is reported.
#[test]
fn the_parse_goes_on_past_a_bare_value_on_any_line() {
    let src = "theme t dark:\n  slot wall -> @stone\n\n\
               struct s size 5x5\n  floor mat_slot=wall\n  walls mat_slot=wall height 3\n  \
               logic sig.a sig.b\n";
    assert_eq!(
        of_code(src, DiagnosticCode::UnexpectedPositional)
            .iter()
            .map(|d| &src[d.span.clone()])
            .collect::<Vec<_>>(),
        ["dark", "size 5x5", "height 3", "logic sig.a sig.b"],
    );
}
