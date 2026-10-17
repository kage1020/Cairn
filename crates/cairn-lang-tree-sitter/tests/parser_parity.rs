//! Accept/reject parity with the reference parser, which is by design the one that is right.
//! The divergences that remain live in [`KNOWN_DIVERGENCES`] and [`DEPTH_DIVERGENCES`], asserted
//! rather than skipped.

use std::fmt::Write as _;

use cairn_lang_core::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH};
use tree_sitter::Parser;

/// What both parsers should say about a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Accept,
    Reject,
}

use Verdict::{Accept, Reject};

/// `(name, source, verdict)`, in `\n` escapes: a raw string would add the file's own indentation.
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
        "requires_prerelease",
        "@requires version>=1.21.4-rc1\n",
        Accept,
    ),
    // The reference parser keeps what it cannot lex, an unterminated `"` included, in the value.
    ("cairn_unlexable_value", "@cairn 2026.6+build\n", Accept),
    (
        "cairn_unterminated_quote",
        "@cairn 2026.6 \"draft\n",
        Accept,
    ),
    // A tab is no part of a value, even inside an unterminated quote.
    (
        "cairn_tab_in_unterminated_quote",
        "@cairn 2026.6 \"dr\taft\n",
        Reject,
    ),
    (
        "requires_unlexable_operator",
        "@requires version~=1.21\n",
        Accept,
    ),
    (
        "requires_edition_scope",
        "@requires java version>=1.21.4\n",
        Accept,
    ),
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
    (
        "slot_state_literal",
        "theme t:\n  slot floor -> @oak_log[axis=x]\n",
        Accept,
    ),
    (
        "slot_state_literal_pairs",
        "theme t:\n  slot s -> @oak_stairs[half=top, facing=north]\n",
        Accept,
    ),
    (
        "slot_state_literal_number_and_bool",
        "theme t:\n  slot c -> @cake[bites=3,lit=true]\n",
        Accept,
    ),
    (
        "slot_state_literal_interior_spaces",
        "theme t:\n  slot s -> @oak_stairs[ half = top , facing = north ]\n",
        Accept,
    ),
    (
        "arg_state_literal",
        "struct s size=3x3\n  floor mat_slot=f\n  thing mat=@oak_log[axis=x]\n",
        Accept,
    ),
    // After a space the `[` is not the token's, and a slot takes one value.
    (
        "slot_state_literal_after_a_space",
        "theme t:\n  slot floor -> @oak_log [axis=x]\n",
        Reject,
    ),
    // A dotted token is abstract and takes no state literal: a touching `[` is whatever follows.
    (
        "abstract_state_literal",
        "theme t:\n  slot floor -> @floor.wood[axis=x]\n",
        Reject,
    ),
    (
        "abstract_token_then_positional_touching",
        "struct s size=3x3\n  thing mat=@a.b[1]\n",
        Accept,
    ),
    (
        "abstract_token_positional_then_list_touching",
        "struct s size=3x3\n  thing @a.b[1]\n",
        Accept,
    ),
    (
        "value_list_abstract_token_then_list_touching",
        "struct s size=3x3\n  thing mat=[@a.b[c]]\n",
        Accept,
    ),
    (
        "place_abstract_theme_then_positional_touching",
        "site s:\n  place id=p use=d theme=@a.b[1]\n",
        Accept,
    ),
    (
        "deep_abstract_token_then_positional_touching",
        "struct s size=3x3\n  thing mat=@a.b.c[1]\n",
        Accept,
    ),
    (
        "state_literal_empty",
        "theme t:\n  slot f -> @oak_log[]\n",
        Reject,
    ),
    (
        "state_literal_unterminated",
        "theme t:\n  slot f -> @oak_log[axis=x\n",
        Reject,
    ),
    (
        "state_literal_no_value",
        "theme t:\n  slot f -> @oak_log[axis=]\n",
        Reject,
    ),
    (
        "state_literal_trailing_comma",
        "theme t:\n  slot f -> @oak_log[axis=x,]\n",
        Reject,
    ),
    (
        "state_literal_double_comma",
        "theme t:\n  slot f -> @oak_stairs[half=top,,facing=north]\n",
        Reject,
    ),
    // Minecraft's syntax, not a Cairn list: the comma is not optional.
    (
        "state_literal_no_comma",
        "theme t:\n  slot f -> @oak_stairs[half=top facing=north]\n",
        Reject,
    ),
    (
        "state_literal_string_value",
        "theme t:\n  slot f -> @oak_log[axis=\"x\"]\n",
        Reject,
    ),
    // Touching, the `[` opens `@a`'s state literal, where `b` is no `key=value`.
    (
        "value_list_token_then_list_touching",
        "struct s size=3x3\n  thing mat=[@a[b]]\n",
        Reject,
    ),
    (
        "value_list_token_then_list_apart",
        "struct s size=3x3\n  thing mat=[@a [b]]\n",
        Accept,
    ),
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
    // -- member-level version floors ----------------------------------
    // `spec/versioning-editions` "Fail-loud and minimum-version inference" gives only `def` and
    // `theme` one. Its expression is opaque, so these pin where the line may stand.
    (
        "def_member_requires",
        "def d size=2x2:\n  requires version>=1.21\n",
        Accept,
    ),
    (
        "def_member_requires_scoped",
        "def d size=2x2:\n  requires java version>=1.21.4\n",
        Accept,
    ),
    // An unreadable expression is `E_INVALID_REQUIRES` from `check`, not a syntax error.
    (
        "def_member_requires_nonsense",
        "def d size=2x2:\n  requires a=1\n",
        Accept,
    ),
    (
        "def_member_requires_unlexable",
        "def d size=2x2:\n  requires version~=1.21\n",
        Accept,
    ),
    (
        "def_member_requires_no_value",
        "def d size=2x2:\n  requires\n",
        Reject,
    ),
    (
        "theme_member_requires",
        "theme t:\n  requires version>=1.21\n  slot floor -> @oak\n",
        Accept,
    ),
    // The floor of a `struct` or a `site` is the file's, spelled `@requires`.
    (
        "struct_member_requires",
        "struct s size=3x3\n  requires version>=1.21\n",
        Reject,
    ),
    (
        "site_member_requires",
        "site p:\n  requires version>=1.21\n",
        Reject,
    ),
    (
        "member_child_requires",
        "def d size=2x2:\n  walls height=3\n    requires version>=1.21\n",
        Reject,
    ),
    // Where no floor may stand, `requires` is an ordinary member keyword.
    (
        "struct_member_named_requires",
        "struct s size=3x3\n  requires a=1\n",
        Accept,
    ),
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
    ("level_bare", "struct s size=3x3\n  level y=0\n", Accept),
    (
        "level_with_body",
        "struct s size=3x3\n  level y=0\n    floor mat_slot=f\n",
        Accept,
    ),
    // Both accept it either way; only the depth comparison sees where the rows land.
    (
        "blank_line_before_a_grouping_member",
        "struct s size=3x3\n  a x=1\n\n  b y=0\n    c x=1\n    d x=2\n\n  e y=5\n    f x=1\n",
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
    // A `[` right after the keyword is the selector, which holds attributes only.
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
    // `and` and `or` are operators where a signal may start, though a dotted name may end in one.
    (
        "logic_operator_as_operand",
        "struct s size=3x3\n  logic s.out = a.b and or\n",
        Reject,
    ),
    (
        "logic_operator_as_whole_value",
        "struct s size=3x3\n  logic s.out = and\n",
        Reject,
    ),
    (
        "logic_operator_after_operator",
        "struct s size=3x3\n  logic s.out = a.b or and and c\n",
        Reject,
    ),
    (
        "logic_operator_after_not",
        "struct s size=3x3\n  logic s.out = not or\n",
        Reject,
    ),
    (
        "logic_operator_heads_a_dotted_name",
        "struct s size=3x3\n  logic s.out = a.b and or.x\n",
        Reject,
    ),
    (
        "logic_operator_ends_a_dotted_name",
        "struct s size=3x3\n  logic s.out = a.b and c.or\n",
        Accept,
    ),
    // `not` is a keyword where an operand may start, though a dotted name may end in it.
    (
        "logic_not_heads_a_dotted_name",
        "struct s size=3x3\n  logic s.out = a.b and not.x\n",
        Reject,
    ),
    (
        "logic_not_ends_a_dotted_name",
        "struct s size=3x3\n  logic s.out = a.b and c.not\n",
        Accept,
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
    (
        "truth_non_bit_input",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 2 -> 1 }\n",
        Reject,
    ),
    (
        "truth_row_matches_the_input_arity",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { 01 -> 1 }\n",
        Accept,
    ),
    // Twenty ones, past `i64`, are still a pattern.
    (
        "truth_row_wider_than_i64",
        "struct s size=3x3\n  assert truth(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t -> z) { 11111111111111111111 -> 1 }\n",
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
    // Unspaced, `101` is one integer, which the reference lexer refuses as an output.
    (
        "truth_output_run_into_the_next_row",
        "struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.c) { 00->101->0 }\n",
        Reject,
    ),
    (
        "truth_two_digit_output_run_into_the_next_row",
        "struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.c) { 00->11->0 }\n",
        Reject,
    ),
    (
        "truth_output_ended_by_a_semicolon",
        "struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.c) { 00->1;01->0 }\n",
        Accept,
    ),
    (
        "truth_output_ended_by_the_brace",
        "struct s size=3x3\n  assert truth(sig.a -> sig.c) { 0->1}\n",
        Accept,
    ),
    // Read as the rows `00->1` and `-0->0`.
    (
        "truth_output_followed_by_a_dont_care_row",
        "struct s size=3x3\n  assert truth(sig.a, sig.b -> sig.c) { 00->1-0->0 }\n",
        Accept,
    ),
    (
        "truth_leading_semicolon",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { ; 0 -> 1 }\n",
        Reject,
    ),
    // `bit_pattern` is external because a longest-match `[01-]` token would take the arrow's `-`
    // in `0-->1` and `11--> 1`.
    (
        "truth_dont_care_low",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { 0-->1 }\n",
        Accept,
    ),
    (
        "truth_dont_care_high",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { -0->1 }\n",
        Accept,
    ),
    (
        "truth_dont_care_between_bits",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 0-1->1 }\n",
        Accept,
    ),
    (
        "truth_dont_care_run_together_with_the_arrow",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 11--> 1 }\n",
        Accept,
    ),
    (
        "truth_dont_care_spaced_from_the_arrow",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 11- -> 1 }\n",
        Accept,
    ),
    (
        "truth_all_dont_care",
        "struct s size=3x3\n  assert truth(a.b, c.d -> e.f) { --->1 }\n",
        Accept,
    ),
    // Acceptance cannot tell a `-` output from a `0`; `both_parsers_read_the_same_row` can.
    (
        "truth_dash_output",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 000 -> - }\n",
        Accept,
    ),
    (
        "truth_dash_output_run_together",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 000->- }\n",
        Accept,
    ),
    (
        "truth_dash_output_under_a_dont_care_pattern",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 00--> - }\n",
        Accept,
    ),
    // The reference lexer's `skip_spaces` refuses a tab, so the scanner must not skip one.
    (
        "truth_tab_before_pattern",
        "struct s size=3x3\n  assert truth(a -> z) {\t1 -> 0 }\n",
        Reject,
    ),
    (
        "truth_tab_before_a_later_row",
        "struct s size=3x3\n  assert truth(a -> z) { 1 -> 0;\t0 -> 1 }\n",
        Reject,
    ),
    (
        "truth_tab_before_output",
        "struct s size=3x3\n  assert truth(a -> z) { 1 ->\t0 }\n",
        Reject,
    ),
    // A space ends a pattern, leaving the `1` a token the row has no place for.
    (
        "truth_dont_care_split_by_a_space",
        "struct s size=3x3\n  assert truth(a, b, c -> z) { 0- 1->1 }\n",
        Reject,
    ),
    // A line break separates rows, and a line inside the braces carries no indentation.
    (
        "truth_row_per_line",
        "struct s size=3x3\n  assert truth(sig.a, sig.b, sig.enable -> sig.out) {\n    001 -> 0\n    011 -> 1\n    101 -> 1\n    111 -> 1\n  }\n",
        Accept,
    ),
    (
        "truth_row_per_line_crlf",
        "struct s size=3x3\r\n  assert truth(a -> z) {\r\n    0 -> 1\r\n    1 -> 0\r\n  }\r\n  floor mat_slot=f\r\n",
        Accept,
    ),
    (
        "truth_row_per_line_lone_cr",
        "struct s size=3x3\r  assert truth(a -> z) {\r    0 -> 1\r    1 -> 0\r  }\r  floor mat_slot=f\r",
        Accept,
    ),
    (
        "truth_rows_at_any_depth",
        "struct s size=3x3\n  level y=1\n    assert truth(a -> z) {\n0 -> 1\n   1 -> 0\n          }\n  floor mat_slot=f\nstruct t size=3x3\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "truth_closing_brace_at_column_zero",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n}\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "truth_blank_and_comment_lines_between_rows",
        "struct s size=3x3\n  assert truth(a -> z) {  # rows\n\n    0 -> 1  # low\n   \n# between\n    1 -> 0\n\n  }\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "truth_semicolon_ending_a_line",
        "struct s size=3x3\n  assert truth(a -> z) { 0 -> 1;\n    1 -> 0; }\n",
        Accept,
    ),
    (
        "truth_rows_on_the_brace_lines",
        "struct s size=3x3\n  assert truth(a -> z) { 0 -> 1\n    1 -> 0 }\n",
        Accept,
    ),
    (
        "truth_empty_across_lines",
        "struct s size=3x3\n  assert truth(a -> z) {\n\n  }\n",
        Accept,
    ),
    (
        "truth_table_ending_the_file",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n  }",
        Accept,
    ),
    (
        "truth_row_split_after_the_arrow",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 ->\n    1\n  }\n",
        Reject,
    ),
    (
        "truth_row_split_before_the_arrow",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0\n    -> 1\n  }\n",
        Reject,
    ),
    (
        "truth_semicolon_opening_a_line",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n    ; 1 -> 0\n  }\n",
        Reject,
    ),
    (
        "truth_brace_on_a_line_of_its_own",
        "struct s size=3x3\n  assert truth(a -> z)\n  {\n    0 -> 1\n  }\n",
        Reject,
    ),
    (
        "truth_unclosed_before_a_member",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n  floor mat_slot=f\n",
        Reject,
    ),
    (
        "truth_unclosed_at_end_of_input",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n",
        Reject,
    ),
    (
        "truth_tab_opening_a_row_line",
        "struct s size=3x3\n  assert truth(a -> z) {\n\t0 -> 1\n  }\n",
        Reject,
    ),
    (
        "truth_member_after_the_closing_brace",
        "struct s size=3x3\n  assert truth(a -> z) {\n    0 -> 1\n  } floor mat_slot=f\n",
        Reject,
    ),
    // Raw text: a `{` there opens no table, and the lines below it keep their layout.
    (
        "cairn_brace_in_value",
        "@cairn 2026.06 {\nstruct s size=3x3\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "requires_brace_in_value",
        "def d size=2x2:\n  requires version>={\n  floor mat_slot=f\n",
        Accept,
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
        "a_dash_in_a_value_is_refused",
        "theme t:\n  slot a-b -> @c\n",
        Reject,
    ),
    // A header refuses any trailing token; `size_with_a_third_extent` is where the size rule is.
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
    // Refused left of the separator, which is scanned before any run of spaces.
    (
        "size_space_before_x",
        "struct s size=9 x7\n  floor mat_slot=f\n",
        Reject,
    ),
    // No size at all: an `x` with no digit behind it ends the number in both parsers, so the `x`
    // and the `2` are positional values.
    (
        "size_separator_before_a_space",
        "struct s size=3x3\n  floor size=2x 2\n",
        Accept,
    ),
    (
        "size_separator_before_a_space_crlf",
        "struct s size=3x3\r\n  floor size=2x 2\r\n",
        Accept,
    ),
    (
        "size_separator_at_end_of_line",
        "struct s size=3x3\n  floor size=2x\n",
        Accept,
    ),
    (
        "size_separator_at_end_of_file",
        "struct s size=3x3\n  floor size=2x",
        Accept,
    ),
    (
        "size_separator_before_a_letter",
        "struct s size=3x3\n  floor size=2xx\n",
        Accept,
    ),
    (
        "size_separator_before_a_space_and_a_word",
        "struct s size=3x3\n  floor size=2x f\n",
        Accept,
    ),
    (
        "size_separator_before_a_space_in_a_list",
        "struct s size=3x3\n  floor a=[2x 2]\n",
        Accept,
    ),
    (
        "size_separator_before_a_dash",
        "struct s size=3x3\n  floor size=2x-1\n",
        Reject,
    ),
    // A header refuses any trailing bare token, as it refuses `size=2x2 junk`.
    (
        "size_separator_before_a_space_in_a_header",
        "struct s size=2x 2\n  floor a=1\n",
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
    // An odd count is refused even on a line whose level does not change.
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
    // The same refusals on a line the scanner reaches through its blank-line loop.
    (
        "odd_indent_after_a_blank_line",
        "struct s size=3x3\n\n   floor a=1\n",
        Reject,
    ),
    (
        "odd_indent_after_a_comment_line",
        "struct s size=3x3\n# c\n   floor a=1\n",
        Reject,
    ),
    (
        "indent_jump_after_a_blank_line",
        "struct s size=3x3\n\n      floor a=1\n",
        Reject,
    ),
    (
        "tab_indent_after_a_blank_line",
        "struct s size=3x3\n\n\tfloor a=1\n",
        Reject,
    ),
    // A legal level, refused by the grammar: a slot row opens no body.
    (
        "indent_under_a_slot_row",
        "theme t:\n  slot a -> @b\n    slot c -> @d\n",
        Reject,
    ),
    ("indent_after_a_directive", "@cairn 1\n  theme t:\n", Reject),
    (
        "indent_under_a_nested_slot_row",
        "theme t:\n  slot a -> @b\n      slot c -> @d\n",
        Reject,
    ),
    (
        "indent_under_a_member_row_is_a_body",
        "struct s size=3x3\n  floor a=1\n    walls b=2\n",
        Accept,
    ),
    ("first_line_indented", "  theme t:\n", Reject),
    ("first_line_indented_odd", " theme t:\n", Reject),
    ("first_line_indented_after_blank", "\n  theme t:\n", Reject),
    // -- layout ----------------------------------------------------------
    ("empty_source", "", Accept),
    (
        "bodyless_decl_then_blank_line",
        "theme a:\n\nstruct s size=3x3\n  floor a=1\n",
        Accept,
    ),
    (
        "bodyless_decl_then_comment_line",
        "theme a:\n# c\nstruct s size=3x3\n  floor a=1\n",
        Accept,
    ),
    (
        "bodyless_decl_then_decl",
        "theme a:\nstruct s size=3x3\n  floor a=1\n",
        Accept,
    ),
    (
        "bodyless_decl_then_blank_line_at_eof",
        "theme a:\n\n",
        Accept,
    ),
    (
        "bodyless_decl_then_comment_line_at_eof",
        "theme a:\n# c\n",
        Accept,
    ),
    (
        "decl_whose_body_is_only_a_comment",
        "struct s size=3x3\n  # note\n",
        Accept,
    ),
    (
        "bodyless_decl_then_blank_and_comment_lines_at_eof",
        "theme a:\n\n# c\n\n",
        Accept,
    ),
    (
        "decl_with_body_then_comment_line_at_eof",
        "theme a:\n  slot x -> @y\n# c\n",
        Accept,
    ),
    (
        "lone_cr_bodyless_decl_then_blank_line_at_eof",
        "theme a:\r\r",
        Accept,
    ),
    (
        "crlf_decl_whose_body_is_only_a_comment",
        "struct s size=3x3\r\n  # note\r\n",
        Accept,
    ),
    // Non-empty bodies, so the EOF DEDENT loop and the depth comparison have something to see.
    (
        "struct_member_then_comment_at_eof",
        "struct s size=3x3\n  floor a=1\n\n# c\n",
        Accept,
    ),
    (
        "nested_body_then_comment_at_eof",
        "struct s size=3x3\n  level y=1\n    floor a=1\n\n# c\n",
        Accept,
    ),
    (
        "bodyless_decl_then_comment_with_no_final_break",
        "theme a:\n# c",
        Accept,
    ),
    (
        "bodyless_decl_then_spaces_only_line_at_eof",
        "theme a:\n  \n",
        Accept,
    ),
    (
        "decl_with_body_then_blank_line",
        "theme a:\n  slot x -> @y\n\nstruct s size=3x3\n  floor a=1\n",
        Accept,
    ),
    (
        "decl_with_body_then_comment_line",
        "theme a:\n  slot x -> @y\n# c\nstruct s size=3x3\n  floor a=1\n",
        Accept,
    ),
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
    // `get_column()` restarts only at `\n`, so after a lone `\r` it cannot give a line's indent.
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
    // -- whitespace before a line break --------------------------------
    // Not corpus cases: a trailing space is invisible in review, and `.gitattributes` turns a
    // fixture's lone `\r` into `\n`.
    (
        "trailing_space_on_a_header",
        "theme t: \n  slot a -> @b\n",
        Accept,
    ),
    (
        "trailing_space_on_a_body_row",
        "theme t:\n  slot a -> @b \n",
        Accept,
    ),
    (
        "trailing_space_before_a_body",
        "struct s size=3x3 \n  floor a=1\n",
        Accept,
    ),
    (
        "trailing_space_at_end_of_file",
        "theme t:\n  slot a -> @b   ",
        Accept,
    ),
    (
        "blank_line_of_only_spaces",
        "theme t:\n  slot a -> @b\n  \n  slot c -> @d\n",
        Accept,
    ),
    // A blank line's spaces are discarded with it, so an odd count is no error.
    (
        "blank_line_of_odd_spaces",
        "theme t:\n  slot a -> @b\n   \n  slot c -> @d\n",
        Accept,
    ),
    (
        "blank_line_of_spaces_before_a_body",
        "struct s size=3x3\n  \n  floor a=1\n",
        Accept,
    ),
    (
        "blank_line_of_spaces_before_a_dedent",
        "struct s size=3x3\n  a x=1\n  \nstruct t size=3x3\n  b x=2\n",
        Accept,
    ),
    (
        "trailing_space_crlf",
        "theme t: \r\n  slot a -> @b\r\n",
        Accept,
    ),
    (
        "trailing_space_lone_cr",
        "theme t: \r  slot a -> @b\r",
        Accept,
    ),
    (
        "lone_cr_blank_line_of_only_spaces",
        "theme t:\r  slot a -> @b\r  \r  slot c -> @d\r",
        Accept,
    ),
    (
        "lone_cr_blank_line_of_spaces_before_a_body",
        "struct s size=3x3\r  \r  floor a=1\r",
        Accept,
    ),
    // A file of nothing but spaces holds no line for them to indent.
    ("spaces_with_no_line_break", "  ", Accept),
    ("blank_line_then_spaces", "\n  ", Accept),
    (
        "dedent_three_levels_to_zero",
        "struct s size=3x3\n  a x=1\n    b x=2\n      c x=3\nstruct t size=3x3\n  e x=5\n",
        Accept,
    ),
    // The scanner withholds `_line_start` here too, or the `/ +/` extra would land the line as a
    // sibling one level short.
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
    // Any identifier opens a statement except `logic` and `assert`, which have their own shapes.
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
    // A directive name is a whole word, not a prefix with a value glued on.
    ("directive_name_with_a_suffix", "@cairnx 2026.06\n", Reject),
    ("directive_name_glued_to_value", "@cairn2026.06\n", Reject),
    ("directive_name_after_a_space", "@ cairn 2026.06\n", Accept),
    // `parse_arg_list_until` skips a comma wherever it finds one in a bracketed filter.
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

/// `(name, source, what the grammar says)` where the reference parser says the opposite.
const KNOWN_DIVERGENCES: &[(&str, &str, Verdict)] = &[
    // -- value ranges, which no grammar can express -------------------
    // `parse_value` refuses a zero extent and an integer past `i64`; a grammar could only count
    // digits, and a narrower `size_literal` would collide with `integer`.
    (
        "size_zero_extent",
        "struct s size=0x3\n  floor mat_slot=f\n",
        Accept,
    ),
    (
        "integer_out_of_range",
        "struct s size=3x3\n  floor n=99999999999999999999\n",
        Accept,
    ),
    // -- a size literal that runs into a word --------------------------
    // Ending the literal before the tail needs negative lookahead, which tree-sitter rejects.
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
    // -- a truth row of the wrong width --------------------------------
    // Relating a row's width to the input count is beyond a context-free grammar.
    (
        "truth_row_wider_than_the_input_list",
        "struct s size=3x3\n  assert truth(a.b -> c.d) { 00 -> 1 }\n",
        Accept,
    ),
    // -- a state literal naming one property twice ----------------------
    // Comparing two property names' text is beyond a context-free grammar.
    (
        "state_literal_repeated_property",
        "theme t:\n  slot f -> @oak_log[axis=x,axis=y]\n",
        Accept,
    ),
];

/// A source of the given depth.
type Shape = fn(usize) -> String;

/// `(name, shape, depth)` one step past a core depth bound, where the grammar accepts and core
/// refuses. Core spends one descent counter on bodies, lists, parentheses and `not` alike, which
/// the grammar cannot share: its rules keep no count, and the scanner bounds indentation alone,
/// at `MAX_INDENT_DEPTH`. One step shallower, both accept.
const DEPTH_DIVERGENCES: &[(&str, Shape, usize)] = &[
    (
        "blocks_past_the_nesting_bound",
        nested_blocks,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "list_past_the_nesting_bound",
        nested_list,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "parens_past_the_nesting_bound",
        nested_parens,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "not_chain_past_the_nesting_bound",
        chained_not,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "blocks_and_list_past_the_nesting_bound",
        list_in_blocks,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "blocks_and_parens_past_the_nesting_bound",
        parens_in_blocks,
        MAX_NESTING_DEPTH + 1,
    ),
    (
        "or_chain_past_the_expression_bound",
        flat_or_chain,
        MAX_EXPR_DEPTH + 1,
    ),
    (
        "and_chain_past_the_expression_bound",
        flat_and_chain,
        MAX_EXPR_DEPTH + 1,
    ),
    (
        "nots_around_an_or_chain_past_the_expression_bound",
        nots_around_an_or_chain,
        MAX_EXPR_DEPTH + 1,
    ),
];

/// One indented `level` block per level.
fn nested_blocks(depth: usize) -> String {
    let mut source = String::from("struct a size=1x1\n");
    for level in 1..=depth {
        source.push_str(&"  ".repeat(level));
        source.push_str("level y=0\n");
    }
    source
}

/// `mat=[[[ … x … ]]]`.
fn nested_list(depth: usize) -> String {
    format!("struct a size=1x1\n  {}\n", list_line(depth))
}

/// `window mat=[[[ … x … ]]]`, the list `depth` deep.
fn list_line(depth: usize) -> String {
    format!("window mat={}x{}", "[".repeat(depth), "]".repeat(depth))
}

/// `logic sig.out = ((( … sig.a … )))`.
fn nested_parens(depth: usize) -> String {
    format!("struct a size=1x1\n  {}\n", parens_line(depth))
}

/// `logic sig.out = ((( … sig.a … )))`, the parentheses `depth` deep.
fn parens_line(depth: usize) -> String {
    format!(
        "logic sig.out = {}sig.a{}",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// `logic sig.out = not not … sig.a`.
fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

/// Half of `depth` in `level` blocks, the rest in a list inside the deepest one.
fn list_in_blocks(depth: usize) -> String {
    in_blocks(depth, list_line)
}

/// Half of `depth` in `level` blocks, the rest in parentheses inside the deepest one.
fn parens_in_blocks(depth: usize) -> String {
    in_blocks(depth, parens_line)
}

/// Half of `depth` in `level` blocks, and the line `inner` makes of the rest inside the deepest.
fn in_blocks(depth: usize, inner: fn(usize) -> String) -> String {
    let blocks = depth / 2;
    let mut source = nested_blocks(blocks);
    let indent = "  ".repeat(blocks + 1);
    writeln!(source, "{indent}{}", inner(depth - blocks))
        .expect("a String never fails to take a write");
    source
}

/// `sig.a0 or sig.a1 or …`, which deepens core's boolean tree without descent.
fn flat_or_chain(terms: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}\n",
        chain("or", terms)
    )
}

/// `sig.a0 and sig.a1 and …`, which deepens core's boolean tree without descent.
fn flat_and_chain(terms: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}\n",
        chain("and", terms)
    )
}

/// The `not`s around the chain in [`nots_around_an_or_chain`], short of the descent bound.
const NOTS: usize = 60;

/// `not not … (sig.a0 or sig.a1 or …)`: `NOTS` levels of `not` over an `or` chain, `depth` deep
/// in all.
fn nots_around_an_or_chain(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}({})\n",
        "not ".repeat(NOTS),
        chain("or", depth - NOTS),
    )
}

/// `terms` distinct signals joined by `op`.
fn chain(op: &str, terms: usize) -> String {
    (0..terms)
        .map(|i| format!("sig.a{i}"))
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

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

/// Each [`DEPTH_DIVERGENCES`] shape at its bound, where both parsers accept it.
fn generated_at_the_bounds() -> impl Iterator<Item = (String, String)> {
    DEPTH_DIVERGENCES
        .iter()
        .map(|&(name, shape, depth)| (format!("{name}, one step shallower"), shape(depth - 1)))
}

#[test]
fn both_parsers_reach_the_expected_verdict() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    let written = FIXTURES
        .iter()
        .map(|&(name, source, verdict)| (name.to_owned(), source.to_owned(), verdict));
    let generated = generated_at_the_bounds().map(|(name, source)| (name, source, Accept));
    for (name, source, expected) in written.chain(generated) {
        let source = source.as_str();
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
        if (core, grammar) != (expected, expected) {
            wrong.push(format!(
                "{name}: expected both to {expected:?}, core said {core:?} and the grammar said {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// 131072 spaces is level 65536, which 16 bits would wrap to level 0; 131074, to level 1.
#[test]
fn an_indent_past_sixteen_bits_of_levels_is_refused() {
    let mut parser = new_parser();
    for (spaces, line) in [(131_072, "struct t size=3x3"), (131_074, "floor a=1")] {
        let source = format!("struct s size=3x3\n{}{line}\n", " ".repeat(spaces));
        assert!(
            cairn_lang_core::parse::parse(&source).is_err(),
            "core accepted {spaces} spaces",
        );
        assert!(
            !grammar_accepts(&mut parser, &source),
            "the grammar accepted {spaces} spaces before `{line}`",
        );
    }
}

/// `_error_sentinel` is valid only in error recovery, where a scanner that answered would rewrite
/// its layout state for a line the parse has discarded.
#[test]
fn a_broken_line_does_not_displace_the_lines_after_it() {
    let mut parser = new_parser();
    // An unterminated list runs on into the next declaration, where recovery starts.
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

/// A tab counts as zero leading spaces, so a scanner that answered would close bodies the author
/// never closed. The second source reaches the tab through the blank-line loop, which a comment
/// line enters and a blank line does not.
#[test]
fn a_tab_indented_line_does_not_close_the_body_around_it() {
    let mut parser = new_parser();
    let cases = [
        (
            "struct s size=3x3\n  a x=1\n\tb x=2\n  c x=3\n",
            vec![
                (1usize, "a".to_owned()),
                (1usize, "b".to_owned()),
                (1usize, "c".to_owned()),
            ],
        ),
        (
            "struct s size=3x3\n  a x=1\n    b x=2\n# c\n\td x=4\n    e x=5\n",
            vec![
                (1usize, "a".to_owned()),
                (2usize, "b".to_owned()),
                (2usize, "d".to_owned()),
                (2usize, "e".to_owned()),
            ],
        ),
    ];
    for (source, expected) in cases {
        let tree = parser.parse(source, None).expect("parse produced no tree");
        assert!(
            tree.root_node().has_error(),
            "the fixture is supposed to be malformed; it no longer is: {source:?}",
        );
        let mut placed = Vec::new();
        grammar_members(tree.root_node(), source, 0, &mut placed);
        assert_eq!(
            placed, expected,
            "a tab closed the body the rows after it are written in: {source:?}",
        );
    }
}

/// The scanner must not withhold the NEWLINE in front of an illegally indented line.
#[test]
fn a_bad_indent_errors_on_its_own_row() {
    let mut parser = new_parser();
    for ending in ["\n", "\r\n", "\r"] {
        // With the break withheld, the whole file would be one `ERROR`.
        let source = "theme t:\n    walls b=2\n".replace('\n', ending);
        let tree = parser.parse(&source, None).expect("parse produced no tree");
        let root = tree.root_node();
        assert!(
            root.has_error(),
            "the fixture is supposed to be malformed; it no longer is: {source:?}",
        );
        let header = root.child(0).expect("source_file has a first child");
        assert_eq!(
            header.kind(),
            "theme_decl",
            "the header in front of an over-indented line went down with it: {source:?} {}",
            root.to_sexp(),
        );
        assert!(
            !header.has_error(),
            "the header kept its node but took the error inside it: {source:?} {}",
            root.to_sexp(),
        );

        // Keywords only: the depth recovery gives an illegally indented row is not pinned.
        let source =
            "struct s size=3x3\n  floor a=1\n      walls b=2\n  door x=1\n".replace('\n', ending);
        let tree = parser.parse(&source, None).expect("parse produced no tree");
        assert!(
            tree.root_node().has_error(),
            "the fixture is supposed to be malformed; it no longer is: {source:?}",
        );
        let mut placed = Vec::new();
        grammar_members(tree.root_node(), &source, 0, &mut placed);
        let keywords: Vec<_> = placed.into_iter().map(|(_, keyword)| keyword).collect();
        assert_eq!(
            keywords,
            ["floor", "walls", "door"],
            "an over-indented row was folded into the row in front of it: {source:?}",
        );

        let source = "struct s size=3x3\n  floor a=1\n\n   walls b=2\n".replace('\n', ending);
        let tree = parser.parse(&source, None).expect("parse produced no tree");
        assert!(
            tree.root_node().has_error(),
            "the fixture is supposed to be malformed; it no longer is: {source:?}",
        );
        let mut statements = 0usize;
        count_member_statements(tree.root_node(), &mut statements);
        assert_eq!(
            statements,
            2,
            "a row next to an odd-indented one lost its statement: {source:?} {}",
            tree.root_node().to_sexp(),
        );
    }
}

/// Every `member_stmt` in the tree, inside an `ERROR` or not.
fn count_member_statements(node: tree_sitter::Node<'_>, count: &mut usize) {
    if node.kind() == "member_stmt" {
        *count += 1;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count_member_statements(child, count);
    }
}

#[test]
fn listed_divergences_still_diverge() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    let written = KNOWN_DIVERGENCES
        .iter()
        .map(|&(name, source, verdict)| (name, source.to_owned(), verdict));
    let generated = DEPTH_DIVERGENCES
        .iter()
        .map(|&(name, shape, depth)| (name, shape(depth), Accept));
    for (name, source, grammar_says) in written.chain(generated) {
        let source = source.as_str();
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
        if grammar != grammar_says || core == grammar {
            wrong.push(format!(
                "{name}: listed as grammar={grammar_says:?} against the reference parser, but core said {core:?} and the grammar said {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a listed divergence changed. If you fixed one, delete its entry: a written source moves to \
         FIXTURES, and a generated shape needs both verdicts asserted at its bound and one step \
         past it:\n{}",
        wrong.join("\n"),
    );
}

/// Compared per member: a sibling left a level too deep leaves the deepest nesting unchanged.
#[test]
fn accepted_sources_place_every_member_at_the_same_depth() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    let written = FIXTURES
        .iter()
        .filter(|&&(_, _, verdict)| verdict == Accept)
        .map(|&(name, source, _)| (name.to_owned(), source.to_owned()));
    for (name, source) in written.chain(generated_at_the_bounds()) {
        let source = source.as_str();
        let Ok(module) = cairn_lang_core::parse::parse(source) else {
            continue;
        };
        let tree = parser.parse(source, None).expect("parse produced no tree");
        // A tree salvaged by recovery could match by accident.
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

/// The corpus writes `(bit_pattern)` with no text, so a grammar reading `11--` for `11-`, or `0`
/// for a `-` output, would pass every other check.
#[test]
fn both_parsers_read_the_same_row() {
    let mut parser = new_parser();
    let mut wrong = Vec::new();
    let mut compared = 0;
    for (name, source, expected) in FIXTURES {
        if *expected != Accept {
            continue;
        }
        let Ok(module) = cairn_lang_core::parse::parse(source) else {
            continue;
        };
        let tree = parser.parse(source, None).expect("parse produced no tree");
        let core = core_rows(&module);
        if core.is_empty() {
            continue;
        }
        let grammar = grammar_rows(tree.root_node(), source);
        compared += 1;
        if core != grammar {
            wrong.push(format!(
                "{name}: the reference parser reads {core:?}, the grammar {grammar:?}\n  source: {source:?}"
            ));
        }
    }
    assert!(compared > 0, "no fixture has a truth row to compare");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every truth row in a module as `PATTERN OUTPUT`, in source order.
fn core_rows(module: &cairn_lang_core::ast::Module) -> Vec<String> {
    use cairn_lang_core::ast::{Item, Statement};
    let mut out = Vec::new();
    for item in &module.items {
        let Item::Struct { body, .. } = item else {
            continue;
        };
        for statement in body {
            if let Statement::AssertTruth { rows, .. } = statement {
                out.extend(rows.iter().map(|row| {
                    let output = match row.output {
                        Some(bit) => u8::from(bit).to_string(),
                        None => "-".to_owned(),
                    };
                    format!("{} {output}", row.inputs)
                }));
            }
        }
    }
    out
}

/// The same list read off the tree-sitter tree.
fn grammar_rows(node: tree_sitter::Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    if node.kind() == "truth_row" {
        let text = |field: &str| {
            node.child_by_field_name(field)
                .unwrap_or_else(|| panic!("a `truth_row` has a `{field}` field"))
                .utf8_text(source.as_bytes())
                .expect("utf-8")
                .to_owned()
        };
        out.push(format!("{} {}", text("inputs"), text("output")));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        out.extend(grammar_rows(child, source));
    }
    out
}

/// Over generated layouts, the grammar accepts nothing the reference parser refuses and places
/// every member where it does.
#[test]
fn generated_layouts_agree_in_the_direction_that_must_hold() {
    let mut parser = new_parser();
    let mut generator = Layouts::seeded();
    let mut accepted_wrongly = Vec::new();
    let mut misplaced = Vec::new();
    let mut both_accept = 0usize;
    for _ in 0..SWEPT_LAYOUTS {
        let source = generator.next_source();
        let core = cairn_lang_core::parse::parse(&source);
        let tree = parser.parse(&source, None).expect("parse produced no tree");
        let grammar_accepts = !tree.root_node().has_error();
        let Ok(module) = core else {
            if grammar_accepts {
                accepted_wrongly.push(format!("{source:?}"));
            }
            continue;
        };
        if !grammar_accepts {
            continue;
        }
        both_accept += 1;
        let mut core_side = Vec::new();
        core_members(&module, &mut core_side);
        let mut grammar_side = Vec::new();
        grammar_members(tree.root_node(), &source, 0, &mut grammar_side);
        if core_side != grammar_side {
            misplaced.push(format!(
                "{source:?}\n  reference parser {core_side:?}\n  grammar          {grammar_side:?}"
            ));
        }
    }
    assert!(
        both_accept > SWEPT_LAYOUTS / 100,
        "only {both_accept} of {SWEPT_LAYOUTS} generated layouts were accepted by both parsers; \
         the generator is no longer producing a corpus worth sweeping",
    );
    let mut report = String::new();
    if !accepted_wrongly.is_empty() {
        writeln!(
            report,
            "the grammar accepted {} sources the reference parser refuses:\n{}",
            accepted_wrongly.len(),
            accepted_wrongly.join("\n"),
        )
        .expect("a String never fails to take a write");
    }
    if !misplaced.is_empty() {
        writeln!(
            report,
            "{} sources both parsers accept place their members differently:\n{}",
            misplaced.len(),
            misplaced.join("\n"),
        )
        .expect("a String never fails to take a write");
    }
    assert!(report.is_empty(), "of {SWEPT_LAYOUTS} layouts, {report}");
}

/// Enough for a lone `\r` under an indent, and the other rare shapes, to appear many times over.
const SWEPT_LAYOUTS: usize = 20_000;

/// A deterministic generator of layouts, the three line endings mixed within one file.
struct Layouts(u64);

impl Layouts {
    /// Changing the seed changes the corpus, and any divergence it turns up is new work.
    fn seeded() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }

    /// xorshift64*: the sweep needs repeatability, not statistical quality.
    fn bits(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.bits() % n as u64).expect("modulus fits")
    }

    fn next_source(&mut self) -> String {
        const LINES: &[&str] = &[
            "@cairn 1",
            "theme t:",
            "slot a -> @b",
            "struct s size=3x3",
            "def d size=2x2:",
            "site p:",
            "floor mat_slot=f",
            "level y=0",
            "walls b=2",
            "logic s.out = a.b and c",
        ];
        const ENDINGS: &[&str] = &["\n", "\r\n", "\r"];

        let rows = 1 + self.below(9);
        let mut out = String::new();
        // The indent walks: independent draws almost never form a legal nesting.
        let mut level = 0usize;
        for _ in 0..rows {
            level = match self.below(4) {
                0 => level + 1,
                1 => level.saturating_sub(1),
                _ => level,
            };
            for _ in 0..level * 2 {
                out.push(' ');
            }
            match self.below(8) {
                0 => {}
                1 => out.push_str("# c"),
                _ => out.push_str(LINES[self.below(LINES.len())]),
            }
            if self.below(8) == 0 {
                out.push(' ');
            }
            out.push_str(ENDINGS[self.below(ENDINGS.len())]);
        }
        if self.below(4) == 0 {
            while out.ends_with('\n') || out.ends_with('\r') {
                out.pop();
            }
        }
        out
    }
}

/// `(depth, keyword)` of each member command in the reference AST, an item's own at depth 1.
fn core_members(module: &cairn_lang_core::ast::Module, out: &mut Vec<(usize, String)>) {
    use cairn_lang_core::ast::Item;
    for item in &module.items {
        // A theme body holds rules, not member commands; `grammar_members` skips it too.
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
        out.push((depth, keyword.to_owned()));
        depth
    } else if node.kind() == "struct_body" || node.kind() == "def_body" {
        // `def_body` differs from `struct_body` only in taking `requires`. A theme body holds no
        // members, as in `core_members`.
        depth + 1
    } else {
        depth
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        grammar_members(child, source, inner, out);
    }
}
