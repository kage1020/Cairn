- *(core)* `ast::Module` gains `edits`, and `IntentModule` gains `edits` and a private field, so a
  struct literal of either no longer compiles.
- *(tree-sitter)* A `selector` node holds one `filter` field per bracket, so `node-types.json`
  marks `filter` as multiple.
