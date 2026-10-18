- *(core)* A `circuit` line under a `level` in a `struct` or `def`, directly or through `level`s
  nested in it, reserves nothing, since place-and-route reads a reservation only from a line among
  the scope's own members, yet `cairn check` and the language server said nothing about it. Beside
  a line at the scope's top level it was dropped without a word, and in a scope with logic and no
  line of its own, `cairn synth` refused the scope at the placement stage. `check` now refuses the
  line as `E_MISPLACED_MEMBER`, saying it reserves nothing and belongs among the scope's own
  members, so a source that built before now exits 1 from `cairn check`, `cairn compile` and
  `cairn synth`. A note points at the line the scope keeps, its own or, when it has none, the first
  line under a `level`, which is the one asked to move out. `E_DUPLICATE_CIRCUIT` still counts only
  the body's own members, so the line is not reported twice.
