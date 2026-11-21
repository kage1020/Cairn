- *(core)* A decimal, `scale=1.5`, parses where an integer is a value: after a key's `=`, in a
  value list and as a bare value. It is kept as written, as the new `ValueKind::Decimal`, and has
  no sign, exponent or suffix: `1.5e3`, `1.5.2`, `1.` and `.5` are `E_PARSE` at the literal, the
  last two told to write `1.0` and `0.5`, and so is a decimal past a double's range. A number
  compares by its value wherever two values are compared, so a theme selector's `[height=4.0]`
  matches `height=4`, and an address spells `02.50` as `2.5`. No key reads a decimal yet, so one is
  a value of the wrong kind wherever it lands:
  - a label or size key, such as `mat_slot=` or `size=`, is a type mismatch;
  - a member key, such as `height=` or `offset=`, refuses its member as `E_INVALID_MEMBER`;
  - a key that falls back to its default, such as `sym=` or `overhang=`, warns with
    `W_IGNORED_ARGUMENT`;
  - a coordinate component, a size extent, a stage count, an assertion's bound and a state
    literal's value are `E_PARSE` at the literal.
- *(core)* A `{` after any key's `=` opens an NBT compound, `nbt={...}`, which parses as the new
  `ValueKind::Nbt`, kept as written. It holds the part of Java's SNBT that every targeted version
  reads alike, on the line it opens on, and the lexer refuses the rest as `E_PARSE` where it
  starts, with what would be valid and how to write it: an unquoted string, a trailing or doubled
  comma, a key named twice, a list of mixed types, an array element of another type, a space
  inside an array's `[B;`, a number past its type's range, a whole number with a leading zero, a
  `.` with no digit on one side, an exponent, an escape other than `\` and the string's own quote,
  and nesting past 64 (`MAX_NBT_DEPTH`). No keyword reads `nbt=` yet, so on a header or a member it is
  `E_UNKNOWN_ARGUMENT` where it was `E_PARSE`.
- *(core)* `lex::scan_line` reads one line as the lexer does: where its comment opens, and whether
  it ends inside a string or an NBT compound.
- *(tree-sitter)* The grammar reads a decimal as a `decimal` node, highlighted as `@number.float`,
  and a compound as an `nbt_compound` of `nbt_entry` nodes, whose values are `nbt_compound`,
  `nbt_list`, `nbt_array`, `nbt_number`, `nbt_string` and `boolean` nodes.
- *(lsp)* Completion reads a line's NBT compound as the lexer does, so the line completes past it,
  and nothing is offered inside it.
- *(vscode)* Decimals are highlighted as numbers, and inside an `nbt={...}` compound its keys,
  strings, numbers, booleans and array types.
