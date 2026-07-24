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
 *    `$._dedent` (e.g. `nested_scope`, itself a `struct_body`), so no
 *    trailing `$._newline` follows them here — there usually isn't a real
 *    newline character left to consume at that point, since a blank line
 *    between that item and whatever comes next is already swallowed by
 *    the *nested* body's own trailing `repeat1($._newline)` (see below).
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
 * Build a `struct`/`def`/`site` declaration rule: `keyword name [args]
 * [:]`, newline-terminated, followed by an indented `struct_body`. The
 * trailing colon is optional, matching
 * cairn-lang-core::parse::Parser::consume_optional_colon, called after
 * struct/def/site headers exactly as it is after `theme` — real-world
 * `.crn` files use it inconsistently (e.g. `struct cottage size=9x7` vs.
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

  externals: $ => [$._indent, $._dedent, $._newline, $._size_x],

  word: $ => $.identifier,

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
    // `directive` is single-line, so it needs its own trailing
    // `repeat1($._newline)` here (see the comment on `body()`) to absorb
    // any blank/comment lines up to the next item. `_top_level_decl` is
    // never bare — struct/def/site/theme all require a body — so it
    // already ends in `$._dedent`; by the time control returns here,
    // whatever blank lines followed it were already consumed by that
    // body's own trailing `repeat1($._newline)`, so no additional
    // `$._newline` is expected (or, past EOF, available) right here.
    source_file: $ => seq(
      repeat($._newline),
      repeat(choice(
        seq($.directive, repeat1($._newline)),
        $._top_level_decl,
      )),
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

    // `nested_scope` and `member_stmt_with_body` are `selfTerminating` (see
    // `body()`): both have their own `struct_body`, so they already end in
    // `$._dedent` and need no trailing `$._newline` of their own here.
    struct_body: $ => body(
      $,
      choice($.member_stmt, $.logic_decl, $.assert_stmt),
      choice($.nested_scope, $.member_stmt_with_body),
    ),

    assert_stmt: $ => seq('assert', choice($.truth_form, $.temporal_form)),

    truth_form: $ => seq(
      'truth', '(',
      field('inputs', $.signal_list),
      '->',
      field('output', $._dotted_ref),
      ')',
      '{',
      $.truth_row,
      repeat(seq(';', $.truth_row)),
      optional(';'),
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

    truth_row: $ => seq($.bit_pattern, '->', $.bit_pattern),
    bit_pattern: $ => /[01]+/,

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
    // optional dotted tail. `signal_ref` in this grammar requires ≥1 tail
    // segment (repeat1), so the degenerate bare-identifier case (`a` alone)
    // is covered by the explicit `identifier` alternative below.
    _bool_expr: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.parenthesized_expression,
      $.signal_ref,
      $.identifier,
      $.boolean,
    ),

    binary_expression: $ => choice(
      prec.left(1, seq(field('lhs', $._bool_expr), 'or',  field('rhs', $._bool_expr))),
      prec.left(2, seq(field('lhs', $._bool_expr), 'and', field('rhs', $._bool_expr))),
    ),

    unary_expression: $ => prec(3, seq('not', field('operand', $._bool_expr))),

    parenthesized_expression: $ => seq('(', $._bool_expr, ')'),

    nested_scope: $ => seq(
      field('keyword', alias(choice('level', 'room'), $.identifier)),
      optional(field('args', $.attribute_list)),
      $._newline,
      field('body', $.struct_body),
    ),

    // Member command args, e.g. `connect west.east_corner to east.west_corner
    // path=@gravel`: a mix of `key=value` attributes and bare positional
    // values (identifiers, signal refs, ...), matching
    // cairn-lang-core::parse::Parser::parse_command's generic arg loop
    // (`is_at_key_eq()` picks an attribute, anything else is a positional
    // value). Deliberately not reusing `attribute_list` here: struct/def/
    // site header args and selector bindings stay strictly `key=value`
    // (cairn-lang-core::parse::Parser::parse_header_args_until_eol only
    // ever calls `parse_arg`), only member-command bodies accept bare
    // positional values.
    command_arg_list: $ => repeat1($.command_arg),
    command_arg: $ => choice($.attribute, $._value),

    // `member_stmt` (a `lineItem`, see `body()`) and `member_stmt_with_body`
    // (`selfTerminating`) share the same head — keyword, optional bracket
    // selector, optional arg list, optional `-> output` tail — matching
    // cairn-lang-core::parse::Parser::parse_command up through its call to
    // `parse_value` for the arrow tail (line ~300). Split into two rules
    // rather than making the body itself optional on one rule: `body()`
    // needs to know statically, per alternative, whether the item already
    // consumed its own trailing newline (`selfTerminating`) or needs one
    // supplied (`lineItem`) — an `optional(field('body', ...))` on a single
    // rule can't express "newline handling differs depending on whether the
    // optional part is present".
    member_stmt: $ => $._member_stmt_head,

    member_stmt_with_body: $ => seq(
      $._member_stmt_head,
      $._newline,
      field('body', $.struct_body),
    ),

    _member_stmt_head: $ => seq(
      field('keyword', $.member_keyword),
      optional(seq('[', field('selector', $.attribute_list), ']')),
      optional(field('args', $.command_arg_list)),
      optional(seq('->', field('output', $._value))),
    ),

    member_keyword: $ => choice(
      'floor', 'walls', 'door', 'window', 'roof', 'stair',
      'pressure_plate', 'circuit', 'place', 'connect',
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

    slot_binding: $ => seq(
      'slot',
      field('name', $.identifier),
      '->',
      field('target', $.material_ref),
    ),

    selector_rule: $ => seq(
      field('selector', $.selector),
      optional(seq('->', field('bindings', $.attribute_list))),
    ),

    selector: $ => seq(
      $.identifier,
      optional(seq('[', $.attribute_list, ']')),
      repeat(seq('.', $.identifier)),
    ),

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
    size_literal: $ => seq(
      alias(token(/[0-9]+/), $.integer),
      $._size_x,
      alias(token.immediate(/[0-9]+/), $.integer),
    ),

    directive: $ => choice(
      seq(field('name', alias('@cairn', $.directive_name)),
          field('arg', $.version_expr)),
      seq(field('name', alias('@requires', $.directive_name)),
          optional('version'),
          field('arg', $.version_expr)),
      seq(field('name', alias('@intended_targets', $.directive_name)),
          field('arg', $.value_list)),
    ),

    version_expr: $ => seq(
      optional($._version_op),
      $.version_literal,
    ),

    _version_op: $ => choice('>=', '<=', '>', '<', '='),

    version_literal: $ => /[0-9]+(\.[0-9]+)*/,

    value_list: $ => seq('[', optional(seq($._value, repeat(seq(',', $._value)))), ']'),

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
    string:  $ => /"([^"\\]|\\.)*"/,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
