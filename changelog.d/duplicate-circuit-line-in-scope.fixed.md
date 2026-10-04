- *(core,redstone)* A second `circuit` line in a `struct` or `def` was dropped without a word, and
  the place-and-route Fix lines suggested writing one. `check` now refuses it as
  `E_DUPLICATE_CIRCUIT`, with a note on the first line, and those Fix lines suggest splitting the
  logic across several scopes, each with its own `circuit` line.
