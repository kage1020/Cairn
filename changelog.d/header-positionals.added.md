- *(core)* A bare value on a `struct`, `def`, `site` or `theme` header, a `theme` rule or an
  `assert` line is `E_UNEXPECTED_POSITIONAL` where it stands, with a note on how to fix that line,
  and the parse goes on to report the rest of the file. So is a `logic` line written without its
  `=`, on the whole line, and it binds nothing. Each was `E_PARSE`, which stopped at the first:
  `struct s size 3x3` is now told to write `size=3x3`. `cairn parse` runs no check, so it now exits
  0 on such a file and prints the bare values as the line's `positional`, and a `logic` line
  without its `=` as a `Statement::UnboundLogic`, which leaves every `Statement::Logic` a binding.
  A `key=value` on a line that takes none, a `site` or `theme` header, a `slot` row or an `assert`
  line, is still `E_PARSE`, and the message now says that line takes no `key=value` argument, what
  it holds instead, and to delete the argument. A block id among a header's bare values is refused
  at its `:`, naming the block id. A header's `size=9x` is now read as a member line reads it,
  `size=9` and a bare `x`, so it is `E_TYPE_MISMATCH_SIZE` and `E_UNEXPECTED_POSITIONAL` rather
  than `E_PARSE`.
- *(tree-sitter)* The grammar reads the bare values on a `struct` or `def` header, a `theme` rule
  and an `assert` line, and a `logic` line without its `=`, as core does, labelling those after a
  `slot` row or an `assert` form `positional`. A bare value on a `site` or `theme` header is still
  a syntax error there.
