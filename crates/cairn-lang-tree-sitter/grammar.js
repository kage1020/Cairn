/// <reference types="tree-sitter-cli/dsl" />

function body($, lineItem, selfTerminating) {
  const alternatives = [seq($._line_start, lineItem, repeat1($._newline))];
  if (selfTerminating) alternatives.push(seq($._line_start, selfTerminating));
  return seq($._indent, repeat(choice(...alternatives)), $._dedent);
}

function declOf(keyword, bodyRule) {
  return $ => seq(
    keyword,
    field('name', $.identifier),
    optional(field('args', $.attribute_list)),
    optional(':'),
    $._newline,
    optional(field('body', $[bodyRule])),
  );
}

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/ +/, $.comment],

  externals: $ => [
    $._indent, $._dedent, $._newline, $._file_start, $._size_x, $._line_start,
    $._file_end, $.bit_pattern, $._error_sentinel,
  ],

  word: $ => $.identifier,

  reserved: {
    global: _ => ['true', 'false'],
  },

  conflicts: $ => [
    [$.member_stmt, $.member_stmt_with_body],
  ],

  rules: {
    // `_file_end` closes the layout a file may trail off in.
    //
    // Blank and comment lines after a declaration are crossed by the
    // scanner on the way to the construct behind them. At the end of a
    // file there is no such construct, so with nothing to cross to the
    // layout was left with no token that could consume it and a file
    // ending that way was refused. A declaration whose body holds a row
    // absorbs its own trailing layout through that body's
    // `repeat1($._newline)`, and a directive through its own, which is
    // why this was the last position in the class.
    //
    // An external token rather than a `repeat($._newline)` here, because
    // the question is one only the scanner can answer. That repeat is
    // ambiguous against the one this rule opens with — for a file
    // holding nothing but line breaks the two own the same tokens — and
    // making the run reachable only after a declaration moves the
    // problem rather than solving it: `_newline` then becomes valid
    // between a declaration's header and the body it opens, which is
    // exactly where the scanner has to cross layout instead of tokenising
    // it. The scanner knows what LR cannot: whether what is left is
    // layout all the way to the end of the file.
    source_file: $ => seq(
      $._file_start,
      repeat($._newline),
      repeat(seq($._line_start, $.directive, repeat1($._newline))),
      repeat(seq($._line_start, $._top_level_decl)),
      optional($._file_end),
    ),

    _top_level_decl: $ => choice(
      $.theme_decl,
      $.struct_decl,
      $.def_decl,
      $.site_decl,
    ),

    struct_decl: declOf('struct', 'struct_body'),
    def_decl:    declOf('def', 'def_body'),

    site_decl: $ => seq(
      'site',
      field('name', $.identifier),
      optional(':'),
      $._newline,
      optional(field('body', $.struct_body)),
    ),

    struct_body: $ => body(
      $,
      choice($.member_stmt, $.logic_decl, $.assert_stmt),
      $.member_stmt_with_body,
    ),

    def_body: $ => body(
      $,
      choice($.requires_stmt, $.member_stmt, $.logic_decl, $.assert_stmt),
      $.member_stmt_with_body,
    ),

    requires_stmt: $ => seq('requires', field('arg', $.directive_literal)),

    assert_stmt: $ => seq('assert', choice($.truth_form, $.temporal_form)),

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

    _dotted_ref: $ => choice($.signal_ref, $.identifier),

    // `bit_pattern` is external, and the only token in this grammar that
    // is a plain run of characters. `-` is a don't-care input, and `-`
    // and `->` share a character: the lexer takes the longest match it
    // can, so `/[01-]+/` reads `11--` out of `11--> 0` and leaves a bare
    // `>` this rule's arrow cannot use. The scanner chooses where the
    // token ends instead of falling into it.
    truth_row: $ => seq(field('inputs', $.bit_pattern), '->', field('output', $.bit)),
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

    member_stmt: $ => $._member_stmt_head,

    member_stmt_with_body: $ => seq(
      $._member_stmt_head,
      $._newline,
      field('body', $.struct_body),
    ),

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

    command_arg_list: $ => repeat1($.command_arg),
    command_arg: $ => choice($.attribute, $._value),

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

    theme_body: $ => body($, choice($.requires_stmt, $.slot_binding, $.selector_rule)),

    slot_binding: $ => seq(
      'slot',
      field('name', $.identifier),
      '->',
      field('target', $._value),
    ),

    selector_rule: $ => seq(
      field('selector', $.selector),
      '->',
      optional(field('bindings', $.attribute_list)),
    ),

    selector: $ => seq(
      field('keyword', $.identifier),
      field('filter', $.selector_filter),
    ),

    selector_filter: $ => seq('[', optional($.filter_list), ']'),

    filter_list: $ => repeat1(choice($.attribute, ',')),

    attribute_list: $ => repeat1($.attribute),

    attribute: $ => seq(field('key', $.identifier), '=', field('value', $._value)),

    material_ref: $ => seq('@', $.identifier, repeat(seq('.', $.identifier))),

    size_literal: $ => seq(
      alias(token(/[0-9]+/), $.integer),
      $._size_x,
      alias(token.immediate(/[0-9]+/), $.integer),
    ),

    directive: $ => choice(
      seq(field('name', alias($._cairn_name, $.directive_name)),
          field('arg', $.directive_literal)),
      seq(field('name', alias($._requires_name, $.directive_name)),
          field('arg', $.directive_literal)),
      seq(field('name', alias($._intended_targets_name, $.directive_name)),
          field('arg', $.string_list)),
    ),

    _cairn_name: _ => seq('@', 'cairn'),
    _requires_name: _ => seq('@', 'requires'),
    _intended_targets_name: _ => seq('@', 'intended_targets'),

    directive_literal: $ => token(/[^#\r\n \t]+( +[^#\r\n \t]+)*/),

    string_list: $ => seq('[', repeat(seq($.string, optional(','))), ']'),

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

    string:  $ => /"[^"\r\n]*"/,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
