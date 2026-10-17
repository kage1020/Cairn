- *(core,tree-sitter)* An `assert truth(...)` table written one row per line, the way truth tables
  are written everywhere else, was refused by both parsers with `expected a truth-table input
  pattern, got end of line`: only a table on its `assert` line parsed. A line break may now come
  before or after any row, after a row's `;`, and before the `}`, and the lines inside the braces
  carry no indentation, so the rows and the `}` may sit at any depth with blank and comment lines
  among them. The table reads exactly as it would on one line:

  ```
  assert truth(sig.a, sig.b -> sig.out) {
    00 -> 0
    01 -> 1
  }
  ```

  A row is still one line, and the `{` stays on the `assert` line. A table whose `}` is missing is
  refused at whatever follows its last row, or at the end of that row's line when the file ends
  there, with ``expected a truth-table row or the `}` that closes the table``. An output moved to
  the line below its arrow is ``truth-table output must be `0`, `1`, or `-`, got end of line``
  rather than `expected integer literal`.
