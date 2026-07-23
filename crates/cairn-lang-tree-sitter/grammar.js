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
      repeat(seq($._blank_placeholder, $._newline)),
    ),

    _blank_placeholder: $ => $.identifier,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
