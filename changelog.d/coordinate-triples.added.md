- *(core)* A coordinate triple, `x,y,z` as in `at=4,3,2`, parses as a `key=` value and names one
  cell of its scope, counted from the low corner of the `size=` footprint. No member reads one yet,
  so a key that takes an integer or a label refuses it as a value of the wrong kind. The tree-sitter
  and VS Code grammars highlight it.
