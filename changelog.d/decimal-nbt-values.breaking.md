- *(core)* `lex` reads `1.5` as one `TokenKind::Decimal`, where it read `Int`, `Dot` and `Int`,
  and a `{` after a key's `=` as one `TokenKind::Nbt` holding the compound, where it read the
  braces and what they hold token by token. It returns an error for a `.` with no digit after it
  (`1.`), a `.` with no digit before it that follows no name, `]` or `)` (`.5`), a decimal that
  runs into a word (`1.5e3`), a size followed by `.` and a digit (`9x7.5`, `TrailingSizeSegment`,
  where it read `Size`, `Dot` and `Int`) and a compound it refuses.
