/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/[ \t]+/, $.comment],

  rules: {
    source_file: $ => optional($.identifier),

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
