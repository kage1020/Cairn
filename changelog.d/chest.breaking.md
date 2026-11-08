- *(core)* `chest` is no longer `E_UNKNOWN_KEYWORD`, and a `size=` on a `chest` line is the chest's
  `type`: `E_TYPE_MISMATCH_SIZE` no longer reports `size=single` there, and a size such as
  `size=2x3` is `W_IGNORED_ARGUMENT`.
- *(core)* `MemberRole` gains `Chest` and `JoinKind` gains `Chest`, so an exhaustive match on
  either no longer compiles.
- *(formats)* `ConnectionKind` gains `Chest`, and both built-in packs' hashes move with the new
  `chest.default` row and the chest rows of `connections`.
