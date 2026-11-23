- *(core)* A `place` line in a `struct` or `def` body builds a `def` inside it as a member of the
  body. `at=inside.<side> offset= y=` stands it against the inside of a wall, its back to the wall,
  and `at=X,Y,Z facing=<compass>` stands it at a cell, `south` by default. Its cells turn with it by
  quarter turns, and so do the `facing`, `axis`, `rotation`, side and rail `shape` states of its
  blocks. It paints in `fixtures`, its slots resolved against the body's theme. Only a sized `def`
  of slotted `fill` lines can be built so; any other, a cell outside the wall ring or the
  footprint, and a missing `use=` or `at=` are `E_INVALID_MEMBER`, and a def above walls that
  build nothing is `W_DEFERRED_MEMBER`.
- *(core)* The compiler ships a furniture library: `table`, `chair`, `fireplace`, `bookcase`,
  `crafting_table`, `furnace`, `barrel`, `lectern`, `wall_torch` and `lantern`, with a theme of
  its own that builds any slot the body's theme leaves unbound. A body's `use=` names one when the
  file declares no `def` of that name. The library's blocks are Evolving.
- *(core)* A body's `place` that names no def of the file or the library is
  `E_UNRESOLVED_PLACE_REF` with the nearest name and the library's names. A def it names draws no
  `W_UNUSED_DEF`, and a body with no theme that places one of the file's defs draws
  `W_NO_THEME_BOUND`.
- *(lsp)* Completion offers `place` among a `struct` or `def` body's member keywords.
- *(xtask)* `cargo xtask lint furniture-library` holds each language's spec to printing the
  library the compiler ships.
