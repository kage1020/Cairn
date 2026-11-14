- *(core)* A decimal, `scale=1.5`, parses where an integer is a value: after a key's `=`, in a
  value list and as a bare value. It is kept as written, as the new `ValueKind::Decimal`, and has
  no sign, exponent or suffix: `1.5e3`, `1.5.2`, `1.` and `.5` are `E_PARSE` at the literal, the
  last two told to write `1.0` and `0.5`. No key reads a decimal yet, so one is a value of the wrong
  kind wherever it lands:
  - a label or size key, such as `mat_slot=` or `size=`, is a type mismatch;
  - a member key, such as `height=` or `offset=`, refuses its member as `E_INVALID_MEMBER`;
  - a key that falls back to its default, such as `sym=` or `overhang=`, warns with
    `W_IGNORED_ARGUMENT`;
  - a coordinate component, a size extent, a stage count, an assertion's bound and a state
    literal's value are `E_PARSE` at the literal.
- *(core)* An `nbt={...}` compound parses as a `key=` value, as the new `ValueKind::Nbt`, kept as
  written. It holds the part of Java's SNBT that every targeted version reads alike, on the line it
  opens on, and the lexer refuses the rest as `E_PARSE` where it starts, with what would be valid:
  an unquoted string, a trailing or doubled comma, a key named twice, a list of mixed types, an
  array element of another type, a number past its type's range, a whole number with a leading
  zero, an exponent, an escape other than `\` and the string's own quote, and nesting past 64
  (`lex::MAX_NBT_DEPTH`). No keyword reads `nbt=` yet, so on a header or a member it is
  `E_UNKNOWN_ARGUMENT` where it was `E_PARSE`.
- *(tree-sitter)* The grammar reads a decimal as a `decimal` node, highlighted as `@number.float`,
  and a compound as an `nbt_compound` of `nbt_entry` nodes, whose values are `nbt_compound`,
  `nbt_list`, `nbt_array`, `nbt_number`, `nbt_string` and `boolean` nodes.
- *(vscode)* Decimals are highlighted as numbers, and inside an `nbt={...}` compound its keys,
  strings, numbers, booleans and array types.
