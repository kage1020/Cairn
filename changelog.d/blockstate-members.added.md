- *(core)* `beam` is a member keyword, laying a run of pillar blocks on their side with
  `kind=pillar`. `at=<side>.top` lays it on the top row of the walls under its `level`, between
  the wall's corners, and `at=<id>.top` on the row above the `door` or `window` with that `id=`,
  one column past each side, over every opening a window's `repeat=` and `sym=true` cut. Its
  `axis` runs along its wall, and `axis=x|y|z` replaces it. It is laid in `envelope`, so a later
  opening cuts through it. Its `mat_slot=` names its block, which at a pinned target must take an
  `axis` of `x`, `y` and `z` (`E_INCOMPATIBLE_MATERIAL` with `required: "pillar"`), and a beam
  without one is built from `@beam.default`. A missing or unknown `kind=`, an `at=` of neither
  form, an id no opening of the body has and a row no `walls` member reaches are each
  `E_INVALID_MEMBER`; a row whose walls paint nothing is `W_DEFERRED_MEMBER`.
- *(core)* `trapdoor` is a member keyword. By a wall it takes the cell a chest with the same
  `at=`, `offset=` and `y=` would, faces away from the wall so it is hinged there and opens flat
  against it, and `facing=in|out|left|right` turns it; at a coordinate triple its `facing=` is a
  compass word, or the block's own when the line writes none. It is built closed in the bottom
  half of its cell, and `open=`, `half=` and `waterlogged=` replace those. A block that is no
  trapdoor is `E_INCOMPATIBLE_MATERIAL` with `required: "trapdoor"`, and a trapdoor without a
  `mat_slot=` is built from `@trapdoor.default`.
- *(core)* `note_block` is a member keyword, set at a coordinate triple. Its `note=`, 0 to 24, and
  its `instrument=` are written into its block's state, and a state its line does not write is
  left to the block's default. At a pinned target an `instrument=` the note block does not take
  is `E_STATE_DOMAIN` on the value, with the new `origin` `argument`, and no note block is built.
- *(core)* A glazed terracotta's `facing`, a snow layer's `layers` and a candle's `candles` take
  no member: they are written as state literals on the material, which `E_STATE_DOMAIN` checks at
  a pinned target.
- *(formats)* Both built-in packs declare the tier-1 tokens `beam.default`, `trapdoor.default` and
  `note_block.default`. A Bedrock build drops a note block's written `note` and `instrument` with
  `W_INTENT_DEGRADED`, since Bedrock keeps the note in a block entity no writer fills yet and works
  the instrument out from the block below.
- *(lsp)* Completion offers `beam`, `trapdoor` and `note_block` with the other member keywords.
- *(vscode)* The grammar highlights the three keywords.
