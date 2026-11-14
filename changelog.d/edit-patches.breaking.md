- *(core)* `ast::Module` gains `edits`, and `IntentModule` gains `edits` and a private field, so a
  struct literal of either no longer compiles.
- *(core)* A theme selector row's `level=NAME` selects the members under the `level` of that id,
  and no longer makes `level=` a key a member's line may write, so `window level=floor2` is
  `E_UNKNOWN_ARGUMENT` even beside a `window[level=floor2]` row. Put the member under
  `level id=floor2` instead.
- *(tree-sitter)* A `selector` node holds one `filter` field per bracket, so `node-types.json`
  marks `filter` as multiple.
