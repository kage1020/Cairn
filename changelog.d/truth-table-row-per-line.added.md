- *(core,tree-sitter)* An `assert truth(...)` table may give each row a line of its own. Only a
  table on its `assert` line parsed before: core refused a line break inside the braces with
  `expected a truth-table input pattern, got end of line`, and the tree-sitter grammar gave an
  ERROR node. A line break may now come before or after any row, after a row's `;`, and before the
  `}`, and the lines inside the braces carry no indentation, so the rows and the `}` may sit at any
  depth with blank and comment lines among them. The table reads exactly as it would on one line:

  ```
  assert truth(sig.a, sig.b -> sig.out) {
    00 -> 0
    01 -> 1
  }
  ```

  A row is still one line, and the `{` stays on the `assert` line. A table whose `}` is missing is
  refused at whatever follows its last row, or at the end of that row's line when the file ends
  there, with ``expected a truth-table row or the `}` that closes the table``. An output that is not
  an integer, such as the `}` of `{ 0 -> }` or an output moved to the line below its arrow, is now
  ``truth-table output must be `0`, `1`, or `-`, got …`` where it was `expected integer literal`.
