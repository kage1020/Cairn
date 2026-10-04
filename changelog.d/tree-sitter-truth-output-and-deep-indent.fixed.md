- *(tree-sitter)* The grammar accepted two shapes `cairn-lang-core` refuses, and built a tree for a
  file nobody wrote:
  - A truth-row output run into the next row, as in `{ 00->101->0 }`, read as the two rows `00->1`
    and `01->0`. The output was `/[01]/`, one character, and the next pattern started on the
    character after it; the reference lexer reads `101` as one integer and refuses it as an output.
    The output is now scanned by the external scanner, which takes one `0` or `1`, and only when no
    digit follows it.
  - A line indented by 131072 spaces read as level 0, a second top-level declaration, and one of
    131074 spaces as an ordinary body line. The scanner narrowed the level to 16 bits before
    comparing it with the indent stack; it now compares first, so the line is refused as an indent
    that opens more than one level.
