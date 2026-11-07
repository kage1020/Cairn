- *(core)* `repeater`, `comparator`, `redstone_torch` and `redstone_dust` are member keywords, the
  Tier 0 parts, each one block placed by hand in a `struct` or `def` body or under an `@edition`
  guard. `at=x,y,z` names its cell as a `raw_block`'s does, and `at=<side>.outside` or
  `at=inside.<side>` with `offset=` and `y=` sets it by a wall. A repeater's or comparator's
  `facing=` is the way it drives: `in`, `out`, `left` or `right` by a wall, away from it when the
  line writes none, and `north`, `south`, `east` or `west` at a triple, where the line must write
  one. A torch by a wall hangs on it, and one at a triple stands, or hangs on the block behind it
  with a compass `facing=`. `delay=` takes 1 to 4, `mode=` `compare` or `subtract`, and
  `connect=none` or `connect=all` builds a dust as a dot or a cross. A missing or unreadable
  `at=`, a `facing=` the anchor does not take there, an `offset=` or `y=` beside a triple and a
  value no part takes are each `E_INVALID_MEMBER`; a value of the wrong kind and a `mat_slot=` are
  `W_IGNORED_ARGUMENT`. A part is in no netlist, so a `->` tail on one is `E_MISPLACED_BINDING`.
- *(core)* Once every member is painted, a redstone dust takes its `north`, `east`, `south` and
  `west` from the blocks beside it as the game sets them: `side` toward dust, a repeater along that
  side, an observer whose back faces it and a block that gives off a signal from any side, `up` or
  `side` toward dust a row up on a block whose top is whole, and `side` toward dust a row down,
  with a block over either diagonal taken to cut it. A dust joined on neither side of one axis is
  joined on both sides of the other.
- *(formats)* Both built-in packs declare `@repeater.default`, `@comparator.default`,
  `@redstone_torch.default`, `@redstone_torch.wall` and `@redstone_dust.default`. Their
  `connections` component reads `redstone_wire` as dust and records, per state, the faces of each
  block that dust beside it joins, extracted from the Java release's block tags.
- *(formats)* A Bedrock id that is a Java block's own and that other Java blocks map onto, such as
  `redstone_torch`, is read as the block whose states the entry carries, so a Java wall torch's
  `facing` writes Bedrock's `torch_facing_direction`.
- *(lsp)* Under an `@edition` guard, completion offers the four parts beside `raw_block` and
  `raw_fill`.
- *(vscode)* The grammar highlights the four parts' keywords.
