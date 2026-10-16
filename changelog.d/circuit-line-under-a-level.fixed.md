- *(core)* A `circuit` line under a `level` in a `struct` or `def` reserves nothing, since
  place-and-route reads a reservation only from a line among the scope's own members, yet
  `cairn check` and the language server said nothing about it. Beside a line at the scope's top
  level it was dropped without a word; on its own, `cairn synth` refused the scope at the placement
  stage. `check` now refuses the line as `E_MISPLACED_MEMBER`, at any depth under a `level`, saying
  it reserves nothing and belongs among the scope's own members. When the scope has a `circuit`
  line of its own, a note points at it and asks for one of the two to go. `E_DUPLICATE_CIRCUIT`
  still counts the body's own members only, so the line is not reported twice.
