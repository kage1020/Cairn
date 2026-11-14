- *(core)* `beam`, `trapdoor` and `note_block` are no longer `E_UNKNOWN_KEYWORD`.
- *(core)* `MemberRole` gains `Beam`, `Trapdoor` and `NoteBlock`, so an exhaustive match on it no
  longer compiles.
- *(formats)* Both built-in packs' hashes move with the new `beam.default`, `trapdoor.default` and
  `note_block.default` rows.
