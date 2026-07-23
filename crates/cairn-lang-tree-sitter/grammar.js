/// <reference types="tree-sitter-cli/dsl" />

/**
 * Wrap an indented block: `_indent`, then zero or more `item` each followed
 * by `_newline`, then `_dedent`. Used by later rules for indent-structured
 * bodies (theme_body, struct_body, ...).
 */
function body($, item) {
  return seq($._indent, repeat(seq(item, $._newline)), $._dedent);
}

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/ +/, $.comment],

  externals: $ => [$._indent, $._dedent, $._newline],

  rules: {
    source_file: $ => seq(
      repeat($._newline),
      repeat(seq($.directive, $._newline)),
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
