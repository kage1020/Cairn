- *(core)* `raw_block` and `raw_fill` are member keywords, the escape hatch built in a `raw` phase
  after every other. `mat=` is a block id, `namespace:path` with an optional state literal touching
  it, as in `mat=minecraft:light[level=15]`. A `raw_block` puts it in the one cell its `at=` names,
  and a `raw_fill` in every cell of the box between `from=` and `to=`, measured as a `fill`'s
  corners are. The block replaces whatever another member put in its cells, air included, and is
  written as given: water poured before it does not waterlog it, and the connection rules neither
  join it to its neighbours nor pair it with a chest, though a neighbour still joins it, reading
  the connection states it writes and the block's defaults for the rest. Two raw members writing
  one cell contest it with `W_PHASE_CONFLICT`. A raw member needs no theme, and a build that pins
  a version checks its id and states as any block's, with `E_UNKNOWN_ID`, `E_STATE_DOMAIN` and
  `E_STATE_UNMAPPED`. A `mat=` that is missing or no block id, or a cell outside the footprint or
  above the struct, is `E_INVALID_MEMBER`, and a `mat_slot=` on the line is `W_IGNORED_ARGUMENT`.
  The `mat=` is judged even where the cells wait or are refused.
- *(core)* A block id is a `key=` value of its own, `ValueKind::BlockId`. A space on either side of
  its `:`, a path that is no identifier, and a block id on a theme's `slot` row are each `E_PARSE`,
  with a message naming the block id or canonical token to write instead.
- *(tree-sitter)* The grammar reads a block id as a `block_id` node and highlights it as a type.
