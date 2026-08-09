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
//! Divergences that remain live in [`KNOWN_DIVERGENCES`], each with the
//! reason it is still there. That list is asserted, not skipped: a
//! divergence that gets fixed fails the test that holds it, so the list
//! shrinks deliberately rather than drifting.

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
    // Both sides of a row hold bits and nothing else.
    (
        "truth_non_bit_input",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 2 -> 1 }\n",
        Reject,
    ),
    // One bit per input signal, counted against the list left of the
    // arrow.
    (
        "truth_row_matches_the_input_arity",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { 01 -> 1 }\n",
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
    // Both refuse this, but not for the reason it looks like: a
    // declaration header refuses *any* trailing bare token, so
    // `size=2x2 junk` fails here identically. The size rule is not
    // consulted, which is why the real case lives in
    // `KNOWN_DIVERGENCES` — see `size_with_a_third_extent`.
    (
        "a_header_refuses_a_trailing_token",
        "struct s size=2x2x9\n  floor a=1\n",
        Reject,
    ),
    (
        "a_header_refuses_a_trailing_token_whatever_it_is",
        "struct s size=2x2 junk\n  floor a=1\n",
        Reject,
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
    // A lone `\r` ends a line for the reference lexer but not for
    // `get_column()`, which restarts only at `\n`. Every line after the
    // first is therefore measured against a running column, and the blank
    // and comment-only lines below are what carry the scanner across a
    // second break before it measures — the case where the base column it
    // measures from has to have moved with it.
    (
        "lone_cr_body_after_blank_line",
        "struct a size=3x3\r\r  floor m=1\r",
        Accept,
    ),
    (
        "lone_cr_body_after_comment_line",
        "theme t:\r  # c\r  slot a -> @b\r",
        Accept,
    ),
    (
        "lone_cr_nested_body_after_blank_line",
        "struct s size=3x3\r  a x=1\r\r    b x=2\r",
        Accept,
    ),
    (
        "lone_cr_multi_level_dedent",
        "struct a size=3x3\r  level y=0\r    room\r      floor mat_slot=f\r  walls mat_slot=w\r",
        Accept,
    ),
    // A file of nothing but spaces holds no line for them to indent.
    ("spaces_with_no_line_break", "  ", Accept),
    ("blank_line_then_spaces", "\n  ", Accept),
    // Three levels closed by one line, landing at level 0.
    (
        "dedent_three_levels_to_zero",
        "struct s size=3x3\n  a x=1\n    b x=2\n      c x=3\nstruct t size=3x3\n  e x=5\n",
        Accept,
    ),
    // A jump of more than one level, inside a body. The scanner can
    // decline the INDENT, but declining alone lets the `/ +/` extra eat
    // the spaces and the line lands as a sibling one level short — so the
    // refusal has to come from the newline in front of it.
    (
        "indent_jump_inside_a_body",
        "struct s size=3x3\n  level y=0\n      room\n",
        Reject,
    ),
    (
        "indent_jump_below_a_nested_body",
        "struct s size=3x3\n  level y=0\n    room\n        f m=1\n",
        Reject,
    ),
    // The keyword redesign's boundary: any identifier opens a statement
    // except the two the reference parser dispatches elsewhere.
    (
        "member_logic_needs_its_own_shape",
        "struct s size=3x3\n  logic a=1\n",
        Reject,
    ),
    (
        "member_assert_needs_its_own_shape",
        "struct s size=3x3\n  assert a=1\n",
        Reject,
    ),
    (
        "member_named_theme",
        "struct s size=3x3\n  theme a=1\n",
        Accept,
    ),
    ("member_named_and", "struct s size=3x3\n  and a=1\n", Accept),
    (
        "member_keyword_is_one_word",
        "struct s size=3x3\n  a.b x=1\n",
        Reject,
    ),
    (
        "theme_body_takes_no_member_row",
        "theme t:\n  floor mat_slot=f\n",
        Reject,
    ),
    // A directive name is one whole word, so a longer one is not that
    // directive with a value glued to it.
    ("directive_name_with_a_suffix", "@cairnx 2026.06\n", Reject),
    ("directive_name_glued_to_value", "@cairn2026.06\n", Reject),
    ("directive_name_after_a_space", "@ cairn 2026.06\n", Accept),
    // Commas are separator noise in a bracketed filter, anywhere and in
    // any number — `parse_arg_list_until` skips one wherever it finds it.
    (
        "filter_leading_comma",
        "struct s size=3x3\n  door[,a=1]\n",
        Accept,
    ),
    (
        "filter_trailing_comma",
        "struct s size=3x3\n  door[a=1,]\n",
        Accept,
    ),
    (
        "filter_doubled_comma",
        "struct s size=3x3\n  door[a=1,,b=2]\n",
        Accept,
    ),
    (
        "filter_only_a_comma",
        "struct s size=3x3\n  door[,]\n",
        Accept,
    ),
    (
        "theme_filter_leading_comma",
        "theme t:\n  window[,a=1] -> c=1\n",
        Accept,
    ),
];

/// `(name, source, what the grammar says)` for every source the two
/// parsers still disagree about — the reference parser's verdict being
/// the opposite of the one recorded here.
///
/// Both directions appear, so this is the full inventory and not only the
/// half that is comfortable. Each entry is asserted rather than skipped:
/// the divergence must still exist, in the direction stated. Fixing one
/// therefore fails this test, which is the point — the list is meant to
/// shrink on purpose.
const KNOWN_DIVERGENCES: &[(&str, &str, Verdict)] = &[
    // -- value ranges, which no grammar can express -------------------
    //
    // `NonZeroU32::new` in `parse_value` refuses a zero extent after the
    // token is built, and `str::parse::<i64>` refuses an integer past
    // `i64` while building one — the latter in the lexer (`scan_number`),
    // as `LexError::InvalidInt`, so it is a lex error rather than a value
    // check on a finished token. A grammar can only
    // approximate either by digit count, which would mis-refuse a valid
    // literal one digit longer — and a narrower `size_literal` pattern
    // collides with `integer` at the same position, so every bare `1` in
    // value position would start a size literal and fail for want of an
    // `x`.
    (
        "size_zero_extent",
        "struct s size=0x3\n  floor mat_slot=f\n",
        Accept,
    ),
    // -- a size literal that runs into a word --------------------------
    //
    // Where the rule is actually consulted — a member's arguments, or a
    // value — the grammar takes what `scan_number` refuses. `Size` holds
    // two extents; a run continuing past the second is a literal the
    // token cannot carry.
    //
    // Expressing that here needs negative lookahead after the height,
    // which tree-sitter's regex engine rejects outright ("look-around ...
    // is not supported"). Widening the height token to swallow the tail
    // would move the error inside `size_literal` rather than ending the
    // literal at it, which is a different tree, not a refusal.
    (
        "size_with_a_third_extent",
        "struct s size=3x3\n  floor mat_slot=f size=2x2x9\n",
        Accept,
    ),
    (
        "size_running_into_a_word",
        "struct s size=3x3\n  floor mat_slot=f size=2x2y\n",
        Accept,
    ),
    // -- a truth row wider than `i64` -----------------------------------
    //
    // A row's pattern reaches the parser as an `Int` token, so `lex.rs`
    // parses it as `i64` on the way through and a 20-bit row of ones
    // overflows. The digits are pattern data rather than a number — the
    // parser keeps the lexeme precisely because `01` and `1` differ — so
    // the ceiling is an artefact of the token type, not a rule. This
    // grammar's `bit_pattern` has no such ceiling and accepts the row.
    (
        "truth_row_wider_than_i64",
        "struct s size=3x3\n  assert truth(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t -> z) { 11111111111111111111 -> 1 }\n",
        Accept,
    ),
    // -- a truth row of the wrong width --------------------------------
    //
    // `parse_assert_truth` compares a row's width against the number of
    // signals left of the arrow. This grammar would have to relate two
    // repetitions in different halves of one rule, which a context-free
    // grammar cannot; the digits themselves are checked on both sides.
    (
        "truth_row_wider_than_the_input_list",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 00 -> 1 }\n",
        Accept,
    ),
    (
        "integer_out_of_range",
        "struct s size=3x3\n  floor n=99999999999999999999\n",
        Accept,
    ),
    // -- a declaration with no body, followed by more file ------------
    //
    // A body absorbs the blank and comment-only lines behind it through
    // `body()`'s trailing `repeat1($._newline)`. A declaration without
    // one has nothing to do that, and the obvious repair — letting the
    // bodyless branch consume them itself — puts two rules in contention
    // for the same newlines and breaks multi-level dedent instead
    // (measured: it re-breaks the very bug this crate's depth test
    // pins). It needs the newline handling reworked rather than patched.
    (
        "bodyless_decl_then_blank_line",
        "theme a:\n\nstruct s size=3x3\n  floor a=1\n",
        Reject,
    ),
    (
        "bodyless_decl_then_comment_line",
        "theme a:\n# c\nstruct s size=3x3\n  floor a=1\n",
        Reject,
    ),
    // -- whitespace before a line break ------------------------------
    //
    // tree-sitter consults the external scanner *before* it skips extras,
    // so a line ending in a space reaches the NEWLINE branch with the
    // space in `lookahead` rather than the break, and no branch there can
    // consume one.
    //
    // Skipping the run is not the repair it appears to be: at the start of
    // a line that same run is the line's indentation, the two are told
    // apart only by reading to the end of the run, and reading it moves
    // the lexer past an indent the branches below still have to measure.
    // Doing it anyway re-breaks multi-level dedent — measured, not
    // assumed. The repair is a way to inspect a run without consuming it.
    (
        "trailing_space_on_a_header",
        "theme t: \n  slot a -> @b\n",
        Reject,
    ),
    (
        "trailing_space_on_a_body_row",
        "theme t:\n  slot a -> @b \n",
        Reject,
    ),
    (
        "blank_line_of_only_spaces",
        "theme t:\n  slot a -> @b\n  \n  slot c -> @d\n",
        Reject,
    ),
    // -- indentation the scanner is never asked about -----------------
    //
    // Both are lines the reference lexer refuses on their indentation and
    // this grammar takes. `_file_start` covers the same class at the top
    // of a file, but only there: after a directive, and inside a body
    // that cannot nest, neither INDENT nor DEDENT is valid, so the
    // scanner is not consulted and the `/ +/` extra eats the leading
    // spaces. Closing them needs a token that is valid wherever a line
    // may begin, which `_file_start` is the single-position sketch of.
    (
        "indent_under_a_slot_row",
        "theme t:\n  slot a -> @b\n    slot c -> @d\n",
        Accept,
    ),
    ("indent_after_a_directive", "@cairn 1\n  theme t:\n", Accept),
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

/// The scanner keeps its hands off the input during error recovery.
///
/// `_error_sentinel` is in no rule, so tree-sitter marks it valid only
/// once it has abandoned the parse and is offering every external token
/// at once. The generated table makes that reachable rather than
/// theoretical — `ts_external_scanner_states[1]` marks all five valid,
/// `FILE_START` included, and that branch rewrites `line_start_column`
/// with wherever the lexer happens to stand.
///
/// The property this asserts is that the *rest of the file* survives the
/// broken line: a source whose second row is malformed and whose fourth
/// is not must still place that fourth row where its indentation says.
/// Without the guard the scanner answers mid-recovery, and the layout
/// state it leaves behind describes a line the parse already discarded.
///
/// It is asserted this way because a coarser test cannot see it: 546
/// malformed sources were swept with and without the guard and *no
/// accept/reject verdict* changed, so no fixture in `FIXTURES` can pin
/// it. 208 of 300 recovery trees did change, which is where the risk
/// lives.
#[test]
fn a_broken_line_does_not_displace_the_lines_after_it() {
    let mut parser = new_parser();
    // An unterminated list runs the parser off the end of its line and on
    // into the next declaration, which is where recovery starts asking the
    // scanner about lines it has already given up on.
    let source = "struct s size=3x3\n  floor a=[1,\nstruct t size=3x3\n  d x=2\n";
    let tree = parser.parse(source, None).expect("parse produced no tree");
    assert!(
        tree.root_node().has_error(),
        "the fixture is supposed to be malformed; it no longer is",
    );
    let mut placed = Vec::new();
    grammar_members(tree.root_node(), source, 0, &mut placed);
    assert_eq!(
        placed,
        vec![(1usize, "floor".to_owned())],
        "recovery placed a member the source does not have there",
    );
}

/// Every listed divergence still diverges, in the direction listed.
///
/// This does not claim the list is complete — nothing here can, since the
/// space of sources is infinite. It claims the opposite and more useful
/// thing: that none of these has been quietly fixed or quietly flipped.
/// A fix fails this test and is meant to, so the entry is removed by the
/// change that earns it.
#[test]
fn listed_divergences_still_diverge() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    for (name, source, grammar_says) in KNOWN_DIVERGENCES {
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
        if grammar != *grammar_says || core == grammar {
            wrong.push(format!(
                "{name}: listed as grammar={grammar_says:?} against the reference parser, but core said {core:?} and the grammar said {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a listed divergence changed. If you fixed one, delete its entry and add the source to FIXTURES:\n{}",
        wrong.join("\n"),
    );
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
        // The fixture is an `Accept`, so an error tree is already a
        // failure of the test above; walking one here would compare a
        // subsequence salvaged from recovery and could match by accident.
        assert!(
            !tree.root_node().has_error(),
            "{name}: the grammar rejected an Accept fixture",
        );
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
    } else if node.kind() == "struct_body" {
        // Only `struct_body` counts a level: a theme body holds rules
        // rather than member commands, so it contributes nothing to walk
        // and `core_members` skips it on the other side too.
        depth + 1
    } else {
        depth
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        grammar_members(child, source, inner, out);
    }
}
