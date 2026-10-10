- *(core,redstone)* Of two `circuit` lines in the same `struct` or `def` body, place-and-route read
  one and dropped the other without a word, and its Fix lines suggested writing more than one.
  `check` now refuses each line after the first as `E_DUPLICATE_CIRCUIT`, with a note on the first
  line and a note on the repair. It counts the body's own members, so a `circuit` line under a
  `level`, which is in that level's body, is not counted. Those Fix lines now suggest splitting the
  logic across several scopes, each with its own `circuit` line.
