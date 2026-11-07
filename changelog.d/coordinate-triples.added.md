- *(core)* A coordinate triple, `x,y,z` as in `at=4,3,2`, parses as a `key=` value and names one
  cell of its scope, counted from the low corner of the `size=` footprint. No member reads one yet,
  so a key that reads another kind of value treats a triple as it treats any value of the wrong
  kind:
  - a label or size key, such as `mat_slot=` or `size=`, is a type mismatch;
  - a member key, such as `side=` or `height=`, refuses its member as `E_INVALID_MEMBER`;
  - a key that falls back to its default, such as `sym=` or `overhang=`, warns with
    `W_IGNORED_ARGUMENT` and the build goes on.

  A `-` before a component, or a size where the last component should end (`1,2,3x4`), is
  `E_PARSE` with a message that says why.
- *(tree-sitter)* A coordinate triple reads as a `coordinate` node, highlighted as
  `@number.special`.
- *(vscode)* Coordinate triples are highlighted as numbers.
