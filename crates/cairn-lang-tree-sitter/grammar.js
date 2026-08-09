/// <reference types="tree-sitter-cli/dsl" />

/**
 * Wrap an indented block: `_indent`, then zero or more items, then
 * `_dedent`. Used by later rules for indent-structured bodies (theme_body,
 * struct_body, ...).
 *
 * Two item shapes:
 *  - `lineItem`s are single-line (no body of their own), so *this* rule is
 *    on the hook for their line terminator: each is followed by
 *    `repeat1($._newline)`.
 *  - `selfTerminating` (optional) items already end in their own nested
 *    `$._dedent` (e.g. `member_stmt_with_body`, itself holding a
 *    `struct_body`), so no trailing `$._newline` follows them here — there
 *    usually isn't a real newline character left to consume at that point,
 *    since a blank line between that item and whatever comes next is
 *    already swallowed by the *nested* body's own trailing
 *    `repeat1($._newline)` (see below).
 *
 * `repeat1($._newline)` (not a single `$._newline`) after a lineItem: a
 * blank or comment-only line between two body items is invisible to the
 * reference lexer (cairn-lang-core::lex skips it without emitting a
 * token), but the external scanner here emits exactly one NEWLINE per
 * physical line break — so each blank line between items surfaces as one
 * more NEWLINE token the grammar must be able to consume. Whichever
 * lineItem immediately precedes a body/top-level boundary greedily
 * consumes every blank line up to that boundary, so nothing else needs to
 * additionally account for them.
 */
function body($, lineItem, selfTerminating) {
  const alternatives = [seq(lineItem, repeat1($._newline))];
  if (selfTerminating) alternatives.push(selfTerminating);
  return seq($._indent, repeat(choice(...alternatives)), $._dedent);
}

/**
 * Build a `struct`/`def` declaration rule: `keyword name [args] [:]`,
 * newline-terminated, followed by an indented `struct_body`. The trailing
 * colon is optional, matching
 * cairn-lang-core::parse::Parser::consume_optional_colon, called after
 * struct/def headers exactly as it is after `theme` — real-world `.crn`
 * files use it inconsistently (e.g. `struct cottage size=9x7` vs.
 * `def cottage size=3x3:`).
 */
function declOf(keyword) {
  return $ => seq(
    keyword,
    field('name', $.identifier),
    optional(field('args', $.attribute_list)),
    optional(':'),
    $._newline,
    optional(field('body', $.struct_body)),
  );
}

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/ +/, $.comment],

  // `_error_sentinel` is in no rule, so it is valid only during error
  // recovery, when tree-sitter offers every external token at once. The
  // scanner tests it and bows out — see the guard at the top of `scan()`.
  externals: $ => [
    $._indent, $._dedent, $._newline, $._file_start, $._size_x, $._error_sentinel,
  ],

  word: $ => $.identifier,

  // cairn-lang-core::lex::Lexer::scan_ident turns these two lexemes into
  // `Bool` tokens before anything asks what was expected, so they are the
  // only two words in the language that can never stand as an identifier.
  // Reserving them globally is what refuses `logic s.out = true` and
  // `struct true size=3x3`, both of which the reference parser rejects
  // with "expected identifier".
  //
  // Only these two. Every other keyword — `theme`, `struct`, `floor`,
  // `slot` — is an ordinary identifier the parser recognises by position,
  // so `struct s` may hold a member named `theme` and the grammar has to
  // let it.
  reserved: {
    global: _ => ['true', 'false'],
  },

  // `member_stmt` and `member_stmt_with_body` share the `_member_stmt_head
  // _newline` prefix (see the comment on `member_stmt`); whether an
  // `_indent` follows that newline (routing into `member_stmt_with_body`)
  // or not (routing into plain `member_stmt`, with `body()`'s
  // `repeat1($._newline)` consuming the newline instead) isn't decidable
  // from the shared prefix alone. Declaring the conflict lets the
  // generated GLR parser carry both interpretations across the shared
  // `_newline` and resolve once the next token (`_indent` or not) is seen.
  conflicts: $ => [
    [$.member_stmt, $.member_stmt_with_body],
  ],

  rules: {
    // `_file_start` consumes the file's opening layout so the first
    // content line is checked for legal indentation like every other line
    // is — see the branch of the same name in `src/scanner.c`.
    //
    // Directives come before every declaration and never after one:
    // cairn-lang-core::parse::Parser::parse_module reads them in a leading
    // loop, so an `@` once the item loop has started reaches
    // `expect_ident` and fails.
    //
    // `directive` is single-line, so it needs its own trailing
    // `repeat1($._newline)` here (see the comment on `body()`) to absorb
    // any blank/comment lines up to the next item.
    //
    // `_top_level_decl` supplies its own: a declaration with a body ends
    // in `$._dedent`, with the blank lines behind it already eaten by that
    // body's trailing `repeat1($._newline)`, so no newline is expected (or,
    // past EOF, available) right here. A declaration *without* a body has
    // no such body to do the eating, and the blank lines after it go
    // unconsumed — see the `bodyless_decl_*` entries in
    // `tests/parser_parity.rs`, which record that as a known divergence.
    source_file: $ => seq(
      $._file_start,
      repeat($._newline),
      repeat(seq($.directive, repeat1($._newline))),
      repeat($._top_level_decl),
    ),

    _top_level_decl: $ => choice(
      $.theme_decl,
      $.struct_decl,
      $.def_decl,
      $.site_decl,
    ),

    struct_decl: declOf('struct'),
    def_decl:    declOf('def'),

    // `site_decl` diverges from struct/def: cairn-lang-core::parse::parse_site_item
    // does not call parse_header_args_until_eol, so `site foo:` accepts no
    // header args — only the name (and optional colon + body).
    site_decl: $ => seq(
      'site',
      field('name', $.identifier),
      optional(':'),
      $._newline,
      optional(field('body', $.struct_body)),
    ),

    // `member_stmt_with_body` is `selfTerminating` (see `body()`): it has
    // its own `struct_body`, so it already ends in `$._dedent` and needs
    // no trailing `$._newline` of its own here.
    struct_body: $ => body(
      $,
      choice($.member_stmt, $.logic_decl, $.assert_stmt),
      $.member_stmt_with_body,
    ),

    assert_stmt: $ => seq('assert', choice($.truth_form, $.temporal_form)),

    // Rows are optional and their separators are too:
    // cairn-lang-core::parse::Parser::parse_assert_truth loops
    // `while !RBrace && !at_eof`, reading a row and then consuming a `;`
    // only if one is there. So `{ }`, `{ 0 -> 1 1 -> 0 }`, and
    // `{ 0 -> 1; }` are all accepted, while a leading `;` is not — the
    // loop opens by demanding an integer.
    truth_form: $ => seq(
      'truth', '(',
      field('inputs', $.signal_list),
      '->',
      field('output', $._dotted_ref),
      ')',
      '{',
      repeat(seq($.truth_row, optional(';'))),
      '}',
    ),

    signal_list: $ => seq($._dotted_ref, repeat(seq(',', $._dotted_ref))),

    // `signal_ref` itself only ever matches a dotted path (one or more
    // `.identifier` segments); a bare identifier is a degenerate
    // zero-segment case. cairn-lang-core::parse::Parser::parse_dotted_ref
    // (the single reference-parser routine backing every one of these call
    // sites) accepts both shapes uniformly, so every grammar site that
    // calls it accepts `choice($.signal_ref, $.identifier)` instead of
    // `$.signal_ref` alone.
    _dotted_ref: $ => choice($.signal_ref, $.identifier),

    // Both sides hold bits and nothing else, so `2 -> 1` and `0 -> 10`
    // are equally refused. The two are still separate rules because their
    // widths differ: the input side carries one bit per signal in the
    // table's input list, and `parse_assert_truth` compares the two — a
    // count this grammar cannot reach, since it would have to relate two
    // repetitions in different halves of the rule. So a pattern of the
    // wrong *width* parses here and is refused there, which
    // `tests/parser_parity.rs` records rather than pretends away.
    truth_row: $ => seq(field('inputs', $.bit_pattern), '->', field('output', $.bit)),
    bit_pattern: $ => token(/[01]+/),
    bit: $ => token(/[01]/),

    temporal_form: $ => seq('always', '(', $.temporal_expr, ')'),

    temporal_expr: $ => seq(
      field('trigger', $._dotted_ref),
      '->',
      'eventually',
      field('target', $._dotted_ref),
      'within',
      field('bound', $.integer),
    ),

    logic_decl: $ => seq(
      'logic',
      field('name', $._dotted_ref),
      '=',
      field('value', $._bool_expr),
    ),

    // `_bool_expr` operands mirror cairn-lang-core::parse::parse_expr_not,
    // which resolves atoms via parse_dotted_ref — a head identifier with an
    // optional dotted tail, and nothing else. A boolean literal is
    // deliberately absent: `logic s.out = true` reaches `expect_ident` with
    // a `Bool` token and is refused. `signal_ref` in this grammar requires
    // ≥1 tail segment (repeat1), so the degenerate bare-identifier case
    // (`a` alone) is covered by the explicit `identifier` alternative.
    _bool_expr: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.parenthesized_expression,
      $.signal_ref,
      $.identifier,
    ),

    binary_expression: $ => choice(
      prec.left(1, seq(field('lhs', $._bool_expr), 'or',  field('rhs', $._bool_expr))),
      prec.left(2, seq(field('lhs', $._bool_expr), 'and', field('rhs', $._bool_expr))),
    ),

    unary_expression: $ => prec(3, seq('not', field('operand', $._bool_expr))),

    parenthesized_expression: $ => seq('(', $._bool_expr, ')'),

    // `member_stmt` (a `lineItem`, see `body()`) and `member_stmt_with_body`
    // (`selfTerminating`) share the same head — keyword, optional bracket
    // selector, arguments, and an optional `-> output` tail. Split into two
    // rules rather than making the body itself optional on one rule:
    // `body()` needs to know statically, per alternative, whether the item
    // already consumed its own trailing newline (`selfTerminating`) or
    // needs one supplied (`lineItem`) — an `optional(field('body', ...))`
    // on a single rule can't express "newline handling differs depending on
    // whether the optional part is present".
    member_stmt: $ => $._member_stmt_head,

    member_stmt_with_body: $ => seq(
      $._member_stmt_head,
      $._newline,
      field('body', $.struct_body),
    ),

    // One shape for every member command, matching
    // cairn-lang-core::parse::Parser::parse_command, which reads a keyword
    // and then loops over arguments until end of line before recursing into
    // an optional indented body. `level` and `room` have no rule of their
    // own here because they have none there either: they are ordinary
    // keywords whose body happens to be the one the geometry passes read,
    // and the parser lets *any* member carry children (writing one where
    // nothing reads it is `E_UNSUPPORTED_NESTING`, a check diagnostic
    // rather than a syntax error).
    //
    // The `-> output` tail sits mid-loop rather than at the end because
    // that is where `parse_command` handles it: the arrow is one branch of
    // the argument loop, which then continues, so `place x -> out mat=@oak`
    // parses. A second arrow is refused, which the shape here expresses by
    // allowing arguments before and after at most one of them.
    _member_stmt_head: $ => seq(
      field('keyword', alias($.identifier, $.member_keyword)),
      choice(
        seq(
          field('selector', alias($.selector_filter, $.selector)),
          optional(field('args', $.command_arg_list)),
        ),
        field('args', alias($._command_arg_list_no_leading_list, $.command_arg_list)),
        blank(),
      ),
      optional(seq(
        '->',
        field('output', $._value),
        optional(field('args', $.command_arg_list)),
      )),
    ),

    // A mix of `key=value` attributes and bare positional values
    // (identifiers, signal refs, lists, ...), matching `parse_command`'s
    // generic argument loop: `is_at_key_eq()` picks an attribute, anything
    // else is a positional value.
    command_arg_list: $ => repeat1($.command_arg),
    command_arg: $ => choice($.attribute, $._value),

    // The same list, but its first argument may not be a bracketed one:
    // `parse_command` tests for `[` before it enters the argument loop, so
    // a bracket in that one position is the selector or nothing.
    // `place [1,2]` is read as a selector holding `1`, where an attribute
    // is required, and refused.
    _command_arg_list_no_leading_list: $ => seq(
      alias($._command_arg_no_list, $.command_arg),
      repeat($.command_arg),
    ),

    _command_arg_no_list: $ => choice(
      $.attribute,
      $.size_literal,
      $.material_ref,
      prec(2, $.signal_ref),
      $.integer,
      $.boolean,
      $.string,
      $.identifier,
    ),

    signal_ref: $ => prec.left(seq(
      $.identifier,
      repeat1(seq('.', $.identifier)),
    )),

    theme_decl: $ => seq(
      'theme',
      field('name', $.identifier),
      optional(':'),
      $._newline,
      optional(field('body', $.theme_body)),
    ),

    theme_body: $ => body($, choice($.slot_binding, $.selector_rule)),

    // The target is a full value, not only a material reference:
    // cairn-lang-core::parse::Parser::parse_theme_rule calls `parse_value`
    // here, so `slot floor -> oak` and `slot floor -> "oak"` are as valid
    // as `slot floor -> @oak_planks`.
    slot_binding: $ => seq(
      'slot',
      field('name', $.identifier),
      '->',
      field('target', $._value),
    ),

    // A theme row that is not a `slot` must be a selector, and the shape is
    // fixed: keyword, bracketed filter, arrow, then zero or more bindings.
    // `parse_theme_rule` refuses a row with no bracket ("expected `slot` or
    // `<keyword>[..]`") and then demands the arrow, and it reads no dotted
    // tail — `window[side=front].inside` stops at the `.`.
    selector_rule: $ => seq(
      field('selector', $.selector),
      '->',
      optional(field('bindings', $.attribute_list)),
    ),

    // A theme selector row carries its keyword inside the node; a member
    // command's keyword is the statement's own, so `_member_stmt_head`
    // aliases the bracket part alone. `parse_theme_rule` and
    // `parse_command` read the same `[key=value, ...]` shape through
    // `parse_arg_list_until(RBracket)`, which accepts an empty list and
    // treats a comma as separator noise it can skip.
    selector: $ => seq(
      field('keyword', $.identifier),
      field('filter', $.selector_filter),
    ),

    selector_filter: $ => seq('[', optional($.filter_list), ']'),

    // Commas are separator noise, in any number and any position:
    // `parse_arg_list_until` skips one wherever it finds it and only ever
    // requires the list to hold attributes, so `[,a=1]`, `[a=1,]`,
    // `[a=1,,b=2]` and `[,]` all parse. Written as a comma-or-attribute
    // repetition rather than a separated list because a separated list
    // cannot express any of those four.
    filter_list: $ => repeat1(choice($.attribute, ',')),

    // Comma-free, unlike `filter_list`: declaration header args
    // (`parse_header_args_until_eol`) and theme selector bindings both loop
    // on `parse_arg` alone, so a comma between them is not skipped but read
    // as the start of the next argument, where it fails.
    attribute_list: $ => repeat1($.attribute),

    attribute: $ => seq(field('key', $.identifier), '=', field('value', $._value)),

    material_ref: $ => seq('@', $.identifier, repeat(seq('.', $.identifier))),

    // The `x` separator is produced by the external scanner (`$._size_x`)
    // rather than a plain `token.immediate('x')`. Reason: `attribute_list`
    // is `repeat1($.attribute)`, so an `identifier` token starting a second
    // attribute is *also* grammatically valid right where the separator
    // must appear; for input like `9x7`, tree-sitter's keyword-extraction
    // machinery (triggered by `word: $.identifier`) greedily scans the
    // word-shaped run `x7`, and since that whole run isn't a registered
    // keyword, falls back to a generic `identifier` token covering `x7` —
    // so a grammar-level literal `x` token can never win against it,
    // regardless of precedence (longest match is compared before
    // precedence). The external scanner runs before that machinery, is
    // only ever consulted where the grammar expects this separator, and
    // is checked before extras are skipped, which is exactly what enforces
    // immediate adjacency (`9 x 7` must not parse as one size literal).
    //
    // The first extent shares its pattern with `integer` deliberately: a
    // narrower one (say, refusing an all-zero run, which `parse_value`
    // does) would be a second token matching the same text at the same
    // position, and the lexer has nothing to choose between them — every
    // bare `1` in a value position would start a size literal and then
    // fail for want of an `x`.
    //
    // The height carries no such guard, though `scan_number` refuses a
    // literal whose run continues into a third extent or a word
    // (`2x2x9`, `2x2y`). Expressing that needs negative lookahead, which
    // tree-sitter's regex engine does not have, and the alternative —
    // making the height a token that also swallows the offending tail —
    // would put the error inside `size_literal` instead of ending it.
    // Recorded in `tests/parser_parity.rs` as a divergence rather than
    // approximated.
    size_literal: $ => seq(
      alias(token(/[0-9]+/), $.integer),
      $._size_x,
      alias(token.immediate(/[0-9]+/), $.integer),
    ),

    // `@cairn` and `@requires` carry an opaque rest-of-line literal.
    // `parse_header` consumes every token up to the newline and keeps the
    // raw source slice, leaving version syntax to a later pass — so
    // `@cairn draft` and `@requires mc>=1.20` are as much a parse as
    // `@cairn 2026.06` is, and an empty value is the one refusal.
    // `@intended_targets` is the exception: its value is re-lexed and must
    // be a list of strings.
    //
    // The name is `@` followed by one whole identifier rather than a
    // literal `'@cairn'`, because a literal has no word boundary after it:
    // `@cairnx 2026.06` would match the literal and leave `x 2026.06` as
    // the opaque value, which the reference parser refuses — it reads the
    // name with `expect_ident` and matches the string it gets. Spelling
    // the name that way here means the same word decides.
    directive: $ => choice(
      seq(field('name', alias($._cairn_name, $.directive_name)),
          field('arg', $.directive_literal)),
      seq(field('name', alias($._requires_name, $.directive_name)),
          field('arg', $.directive_literal)),
      seq(field('name', alias($._intended_targets_name, $.directive_name)),
          field('arg', $.string_list)),
    ),

    // `@` and the word are separate tokens, deliberately. That is what
    // gives the name a word boundary: `word: $.identifier` puts keyword
    // extraction in front of the lexer, so `@cairnx` scans the whole run
    // `cairnx`, finds it is not the keyword `cairn`, and fails — where a
    // glued `'@cairn'` literal would match its first six characters and
    // leave `x 2026.06` as the value. It also matches the reference
    // lexer, which emits `At` and then an identifier, so `@ cairn 1` is
    // the same directive to both.
    _cairn_name: _ => seq('@', 'cairn'),
    _requires_name: _ => seq('@', 'requires'),
    _intended_targets_name: _ => seq('@', 'intended_targets'),

    // Stops at `#` because the reference lexer strips a comment before the
    // header parser ever sees it, and holds internal spaces because that
    // parser spans from the first token after the name to the last one on
    // the line. The trailing run keeps the token from ending on a space,
    // which would otherwise put whitespace inside the node.
    directive_literal: $ => token(/[^#\r\n \t]+( +[^#\r\n \t]+)*/),

    string_list: $ => seq('[', repeat(seq($.string, optional(','))), ']'),

    // A comma between items is optional in both directions: the list loop
    // in `parse_value` reads a value and then consumes a comma if one
    // follows, so `[a,]` and `[a b]` both parse while `[,a]` does not.
    value_list: $ => seq('[', repeat(seq($._value, optional(','))), ']'),

    _value: $ => choice(
      $.size_literal,
      $.material_ref,
      prec(2, $.signal_ref),
      $.integer,
      $.boolean,
      $.string,
      $.identifier,
      $.value_list,
    ),

    integer: $ => /[0-9]+/,
    boolean: $ => choice('true', 'false'),

    // No escape sequences: `scan_string` ends the literal at the first
    // `"`, whatever precedes it. So `"a\"b"` lexes as the string `a\`,
    // then the identifier `b`, then a quote that reaches the line's end
    // unclosed — which is `LexError::UnterminatedString`, and a lex error
    // refuses the whole file rather than the one line.
    string:  $ => /"[^"\r\n]*"/,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
