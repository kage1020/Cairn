//! Accept/reject parity with the reference parser.
//!
//! The grammar in this crate is a second implementation of the language
//! `cairn-lang-core` defines, so any source the two disagree about is a
//! bug in one of them — and by [the design doc's constraint 1][design],
//! the reference parser is the one that is right. Nothing else in the
//! crate compares them: `tests/examples.rs` checks the grammar accepts
//! every file in `examples/`, which is one direction over a corpus of
//! twelve valid files, and `test/corpus/` pins trees this grammar builds
//! without consulting the other side at all.
//!
//! [design]: ../../../docs/superpowers/specs/2026-07-23-tree-sitter-cairn-design.md
//!
//! Every fixture below is fed to both parsers and their verdicts compared.
//! The expected verdict is written out too, so a fixture that silently
//! flips in *both* parsers still fails: agreeing on the wrong answer is
//! the failure mode a differential test alone cannot see.
//!
//! Deliberate divergences live in [`RANGE_DIVERGENCES`] with the reason
//! each one is out of a grammar's reach.

use tree_sitter::Parser;

/// What both parsers should say about a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Both parsers build a tree.
    Accept,
    /// Both parsers refuse.
    Reject,
}

use Verdict::{Accept, Reject};

/// `(name, source, verdict)`.
///
/// Grouped by the construct each one probes, and written with `\n` escapes
/// rather than raw strings because leading whitespace *is* the fixture in
/// the indentation cases — a raw string would put the file's own
/// indentation into the source.
const FIXTURES: &[(&str, &str, Verdict)] = &[
    // -- directives --------------------------------------------------
    ("cairn_version", "@cairn 2026.06\n", Accept),
    // The value is kept as an opaque slice, so anything non-empty parses.
    ("cairn_word_value", "@cairn draft\n", Accept),
    ("cairn_trailing_words", "@cairn 2026.06 extra\n", Accept),
    ("cairn_no_value", "@cairn\n", Reject),
    (
        "requires_version_prefix",
        "@requires version>=1.20\n",
        Accept,
    ),
    ("requires_other_prefix", "@requires mc>=1.20\n", Accept),
    ("requires_bare", "@requires 1.20\n", Accept),
    (
        "intended_targets",
        "@intended_targets [\"1.20.4\",\"1.21.4\"]\n",
        Accept,
    ),
    ("intended_targets_empty", "@intended_targets []\n", Accept),
    (
        "intended_targets_trailing_comma",
        "@intended_targets [\"a\",]\n",
        Accept,
    ),
    // Re-lexed and required to be strings, unlike every other directive.
    (
        "intended_targets_idents",
        "@intended_targets [oak]\n",
        Reject,
    ),
    ("unknown_directive", "@nope 1\n", Reject),
    (
        "directive_after_decl",
        "theme t:\n  slot a -> @b\n@cairn 2026.06\n",
        Reject,
    ),
    // -- theme bodies ------------------------------------------------
    ("slot_material", "theme t:\n  slot floor -> @oak\n", Accept),
    ("slot_ident", "theme t:\n  slot floor -> oak\n", Accept),
    ("slot_string", "theme t:\n  slot floor -> \"oak\"\n", Accept),
    ("slot_int", "theme t:\n  slot floor -> 3\n", Accept),
    ("slot_dotted", "theme t:\n  slot a -> b.c\n", Accept),
    ("slot_no_arrow", "theme t:\n  slot a\n", Reject),
    (
        "selector_filtered",
        "theme t:\n  window[class=small] -> frame=@spruce\n",
        Accept,
    ),
    (
        "selector_empty_filter",
        "theme t:\n  window[] -> f=@oak\n",
        Accept,
    ),
    (
        "selector_no_bindings",
        "theme t:\n  window[class=a] ->\n",
        Accept,
    ),
    (
        "selector_comma_filter",
        "theme t:\n  window[a=1, b=2] -> c=1\n",
        Accept,
    ),
    // A theme row is `slot` or `<keyword>[..] ->`, with nothing between.
    (
        "selector_no_bracket",
        "theme t:\n  front -> mat=@oak\n",
        Reject,
    ),
    ("selector_no_arrow", "theme t:\n  front[side=x]\n", Reject),
    (
        "selector_dotted_tail",
        "theme t:\n  window[side=front].inside -> mat=@oak\n",
        Reject,
    ),
    ("theme_no_body", "theme empty:\n", Accept),
    // -- member commands ---------------------------------------------
    (
        "struct_member",
        "struct s size=3x3\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "member_unknown_keyword",
        "struct s size=3x3\n  gazebo mat_slot=f\n",
        Accept,
    ),
    // `level` and `room` are ordinary keywords, body or no body.
    ("level_bare", "struct s size=3x3\n  level y=0\n", Accept),
    (
        "level_with_body",
        "struct s size=3x3\n  level y=0\n    floor mat_slot=f\n",
        Accept,
    ),
    (
        "unknown_keyword_with_body",
        "struct s size=3x3\n  gazebo y=0\n    floor mat_slot=f\n",
        Accept,
    ),
    (
        "member_selector_comma",
        "struct s size=3x3\n  door[side=front, y=2]\n",
        Accept,
    ),
    (
        "member_empty_selector",
        "struct s size=3x3\n  door[] mat=@oak\n",
        Accept,
    ),
    // A bracket right after the keyword is the selector, so its contents
    // must be attributes and a bare list cannot lead.
    (
        "member_bracket_non_attr",
        "struct s size=3x3\n  door[a]\n",
        Reject,
    ),
    ("member_leading_list", "site s:\n  place [1,2]\n", Reject),
    (
        "member_list_after_selector",
        "struct s size=3x3\n  door[side=front] [1,2]\n",
        Accept,
    ),
    (
        "member_list_trailing_comma",
        "site s:\n  place items=[a,]\n",
        Accept,
    ),
    (
        "member_nested_list",
        "struct s size=3x3\n  floor a=[[1,2],[3]]\n",
        Accept,
    ),
    // The arrow sits inside the argument loop, so arguments may follow it.
    (
        "member_arrow_then_args",
        "site s:\n  place x -> out mat=@oak\n",
        Accept,
    ),
    ("member_arrow_only", "site s:\n  place -> out\n", Accept),
    (
        "member_two_arrows",
        "site s:\n  place x -> a -> b\n",
        Reject,
    ),
    (
        "member_positional",
        "site s:\n  connect a.b to c.d path=@gravel\n",
        Accept,
    ),
    // A `site` header takes no arguments, unlike `struct` and `def`.
    (
        "site_header_args",
        "site s size=3x3\n  place id=a\n",
        Reject,
    ),
    (
        "top_level_unknown",
        "widget w\n  floor mat_slot=f\n",
        Reject,
    ),
    // -- booleans are never identifiers ------------------------------
    (
        "member_value_bool",
        "struct s size=3x3\n  floor a=true\n",
        Accept,
    ),
    ("bool_as_key", "struct s size=3x3\n  floor true=1\n", Reject),
    (
        "bool_as_member_keyword",
        "struct s size=3x3\n  true a=1\n",
        Reject,
    ),
    (
        "bool_as_decl_name",
        "struct true size=3x3\n  floor a=1\n",
        Reject,
    ),
    ("bool_as_slot_name", "theme t:\n  slot true -> @a\n", Reject),
    // -- logic --------------------------------------------------------
    (
        "logic_expr",
        "struct s size=3x3\n  logic s.out = a.b and not (c.d or e)\n",
        Accept,
    ),
    // Operands resolve through `parse_dotted_ref`, which reads no literal.
    (
        "logic_bool_literal",
        "struct s size=3x3\n  logic s.out = true\n",
        Reject,
    ),
    (
        "logic_not_bool",
        "struct s size=3x3\n  logic s.out = not true\n",
        Reject,
    ),
    (
        "logic_paren_bool",
        "struct s size=3x3\n  logic s.out = (true)\n",
        Reject,
    ),
    (
        "logic_bool_lhs",
        "struct s size=3x3\n  logic true = a.b\n",
        Reject,
    ),
    // -- assertions ---------------------------------------------------
    (
        "truth_rows",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { 00 -> 1; 11 -> 0 }\n",
        Accept,
    ),
    (
        "truth_empty",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { }\n",
        Accept,
    ),
    (
        "truth_no_semicolons",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 0 -> 1 1 -> 0 }\n",
        Accept,
    ),
    (
        "truth_trailing_semicolon",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 0 -> 1; }\n",
        Accept,
    ),
    // The input side is any integer lexeme; only the output is a bit.
    (
        "truth_non_bit_input",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 2 -> 1 }\n",
        Accept,
    ),
    (
        "truth_multi_digit_output",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 0 -> 10 }\n",
        Reject,
    ),
    (
        "truth_padded_output",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 0 -> 01 }\n",
        Reject,
    ),
    (
        "truth_leading_semicolon",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { ; 0 -> 1 }\n",
        Reject,
    ),
    (
        "always_within",
        "struct s size=3x3\n  assert always(a.b -> eventually c.d within 5)\n",
        Accept,
    ),
    (
        "always_no_within",
        "struct s size=3x3\n  assert always(a.b -> eventually c.d)\n",
        Reject,
    ),
    // -- literals ------------------------------------------------------
    (
        "size_leading_zero",
        "struct s size=03x3\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "size_spaced",
        "struct s size=9 x 7\n  floor mat_slot=f\n",
        Reject,
    ),
    // A string ends at the first quote and never spans a line.
    (
        "string_escaped_quote",
        "theme t:\n  slot a -> \"x\\\"y\"\n",
        Reject,
    ),
    ("string_unterminated", "theme t:\n  slot a -> \"x\n", Reject),
    // -- indentation ----------------------------------------------------
    (
        "dedent_two_levels",
        "struct a size=3x3\n  level y=0\n    room\n      floor mat_slot=f\n  walls mat_slot=w\n  door side=front\n",
        Accept,
    ),
    // An odd count is an error wherever it appears, including where the
    // level does not change and the line is therefore not asked about.
    (
        "odd_indent_same_level",
        "theme t:\n  slot a -> @b\n   slot c -> @d\n",
        Reject,
    ),
    (
        "odd_indent_deeper",
        "struct s size=3x3\n  floor a=1\n     walls b=2\n",
        Reject,
    ),
    (
        "odd_indent_first_body_line",
        "theme t:\n   slot a -> @b\n",
        Reject,
    ),
    (
        "indent_jump",
        "struct s size=3x3\n    floor mat_slot=x\n",
        Reject,
    ),
    ("tab_indent", "theme t:\n\tslot a -> @b\n", Reject),
    ("first_line_indented", "  theme t:\n", Reject),
    ("first_line_indented_odd", " theme t:\n", Reject),
    ("first_line_indented_after_blank", "\n  theme t:\n", Reject),
    // -- layout ----------------------------------------------------------
    ("empty_source", "", Accept),
    ("blank_lines_only", "\n\n  \n", Accept),
    ("comment_only", "# hello\n", Accept),
    (
        "comment_between_members",
        "struct s size=3x3\n  # c\n  floor mat_slot=f\n",
        Accept,
    ),
    ("crlf", "theme t:\r\n  slot a -> @b\r\n", Accept),
    ("lone_cr", "theme t:\r  slot a -> @b\r", Accept),
    ("no_trailing_newline", "theme t:\n  slot a -> @b", Accept),
];

/// Fixtures the reference parser refuses on a value's *range*, which a
/// context-free grammar has no way to express: the refusal comes from
/// `NonZeroU32::new` and `str::parse::<i64>` inside
/// `cairn-lang-core::parse::Parser::parse_value`, after a token that is
/// lexically well-formed.
///
/// Listed rather than omitted so the divergence stays visible, and
/// asserted rather than skipped: each one must still parse here and still
/// be refused there. A grammar that started rejecting them would be
/// rejecting by digit count, which is the wrong shape of rule and would
/// mis-refuse a valid literal one digit longer.
const RANGE_DIVERGENCES: &[(&str, &str)] = &[
    (
        "size_zero_extent",
        "struct s size=0x3\n  floor mat_slot=f\n",
    ),
    (
        "integer_out_of_range",
        "struct s size=3x3\n  floor n=99999999999999999999\n",
    ),
];

fn grammar_accepts(parser: &mut Parser, source: &str) -> bool {
    let tree = parser.parse(source, None).expect("parse produced no tree");
    !tree.root_node().has_error()
}

fn new_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");
    parser
}

/// The two parsers agree on every fixture, and agree on the verdict
/// written down for it.
#[test]
fn both_parsers_reach_the_written_verdict() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    for (name, source, expected) in FIXTURES {
        let core = if cairn_lang_core::parse::parse(source).is_ok() {
            Accept
        } else {
            Reject
        };
        let grammar = if grammar_accepts(&mut parser, source) {
            Accept
        } else {
            Reject
        };
        if (core, grammar) != (*expected, *expected) {
            wrong.push(format!(
                "{name}: expected both to {expected:?}, core said {core:?} and the grammar said {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The listed divergences are exactly the ones that exist: each parses
/// here and is refused there.
#[test]
fn range_divergences_are_the_only_ones() {
    let mut parser = new_parser();
    for (name, source) in RANGE_DIVERGENCES {
        assert!(
            cairn_lang_core::parse::parse(source).is_err(),
            "{name}: the reference parser now accepts this, so it is no longer a divergence",
        );
        assert!(
            grammar_accepts(&mut parser, source),
            "{name}: the grammar now refuses this; if that is deliberate, move it into FIXTURES",
        );
    }
}

/// Where each member sits agrees too, not just the yes/no.
///
/// Acceptance parity alone would be satisfied by a grammar that accepts
/// the right files and builds the wrong tree for them — which is exactly
/// what a multi-level dedent used to do: the file parsed, and the member
/// after the dedent ended up a level too deep. The *deepest* nesting is
/// unchanged by that (the sibling sinks into a body that already exists),
/// so the comparison has to be per member: every keyword in document
/// order, each with the depth it sits at.
///
/// Keyword and depth are the two things both parsers name the same way;
/// node kinds and argument shapes are each parser's own business.
#[test]
fn accepted_sources_place_every_member_at_the_same_depth() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    for (name, source, expected) in FIXTURES {
        if *expected != Accept {
            continue;
        }
        let Ok(module) = cairn_lang_core::parse::parse(source) else {
            continue;
        };
        let tree = parser.parse(source, None).expect("parse produced no tree");
        let mut core = Vec::new();
        core_members(&module, &mut core);
        let mut grammar = Vec::new();
        grammar_members(tree.root_node(), source, 0, &mut grammar);
        if core != grammar {
            wrong.push(format!(
                "{name}: the reference parser places {core:?}, the grammar {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `(depth, keyword)` for every member command in the reference AST, in
/// document order. A top-level item's own members sit at depth 1.
fn core_members(module: &cairn_lang_core::ast::Module, out: &mut Vec<(usize, String)>) {
    use cairn_lang_core::ast::Item;
    for item in &module.items {
        // A theme body holds rules rather than member commands, and `Item`
        // is `#[non_exhaustive]`, so a kind added later contributes nothing
        // here either — its members would then show up on the grammar side
        // alone and fail loudly rather than pass quietly.
        if let Item::Def { body, .. } | Item::Site { body, .. } | Item::Struct { body, .. } = item {
            core_statements(body, 1, out);
        }
    }
}

fn core_statements(
    body: &[cairn_lang_core::ast::Statement],
    depth: usize,
    out: &mut Vec<(usize, String)>,
) {
    use cairn_lang_core::ast::Statement;
    for statement in body {
        if let Statement::Generic {
            keyword, children, ..
        } = statement
        {
            out.push((depth, keyword.clone()));
            core_statements(children, depth + 1, out);
        }
    }
}

/// The same list read off the tree-sitter tree.
fn grammar_members(
    node: tree_sitter::Node<'_>,
    source: &str,
    depth: usize,
    out: &mut Vec<(usize, String)>,
) {
    let inner = if node.kind() == "member_keyword" {
        let keyword = node.utf8_text(source.as_bytes()).expect("utf-8");
        // The keyword's own statement sits one level shallower than the
        // body it may open, and `struct_body` is what counts a level, so
        // the depth recorded here is the one already accumulated.
        out.push((depth, keyword.to_owned()));
        depth
    } else if matches!(node.kind(), "theme_body" | "struct_body") {
        depth + 1
    } else {
        depth
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        grammar_members(child, source, inner, out);
    }
}
