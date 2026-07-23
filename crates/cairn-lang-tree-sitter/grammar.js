/// <reference types="tree-sitter-cli/dsl" />

/**
 * Wrap an indented block: `_indent`, then zero or more `item` each followed
 * by `_newline`, then `_dedent`. Used by later rules for indent-structured
 * bodies (theme_body, struct_body, ...).
 */
function body($, item) {
  return seq($._indent, repeat(seq(item, $._newline)), $._dedent);
}

/**
 * Build a `struct`/`def`/`site` declaration rule: `keyword name [args]`,
 * newline-terminated, followed by an indented `struct_body`.
 */
function declOf(keyword) {
  return $ => seq(
    keyword,
    field('name', $.identifier),
    optional(field('args', $.attribute_list)),
    $._newline,
    field('body', $.struct_body),
  );
}

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/ +/, $.comment],

  externals: $ => [$._indent, $._dedent, $._newline, $._size_x],

  word: $ => $.identifier,

  rules: {
    source_file: $ => seq(
      repeat($._newline),
      repeat(seq(choice($.directive, $._top_level_decl), $._newline)),
    ),

    _top_level_decl: $ => choice(
      $.theme_decl,
      $.struct_decl,
      $.def_decl,
      $.site_decl,
    ),

    struct_decl: declOf('struct'),
    def_decl:    declOf('def'),
    site_decl:   declOf('site'),

    struct_body: $ => seq(
      $._indent,
      repeat(seq($._struct_body_item, $._newline)),
      $._dedent,
    ),

    _struct_body_item: $ => choice($.member_stmt, $.nested_scope, $.logic_decl),

    logic_decl: $ => seq(
      'logic',
      field('name', $.signal_ref),
      '=',
      field('value', $._bool_expr),
    ),

    _bool_expr: $ => choice(
      $.binary_expression,
      $.unary_expression,
      $.parenthesized_expression,
      $.signal_ref,
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

    member_stmt: $ => seq(
      field('keyword', $.member_keyword),
      optional(field('args', $.attribute_list)),
      optional(seq('->', field('output', $.signal_ref))),
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
      ':',
      $._newline,
      field('body', $.theme_body),
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
