- *(core)* `W_NO_THEME_BOUND` said "every `mat_slot=` will lower to air", which is true of a `floor`
  only. It now reads "has no theme bound, so no `mat_slot=` in it resolves to a block", and a new
  first note says what each role does instead: a `floor` is left air; no `walls` is built or takes
  up rows, so no `door` or `window` is cut; a `roof` is built from its default block, and so is an
  eave `stair` or `pressure_plate` that still fits the shorter structure. The code and severity are
  unchanged.
- *(spec)* `spec/lint` said the same in `W_NO_THEME_BOUND`'s catalog row and in the two passages
  that explain why `E_UNKNOWN_SLOT_TARGET` is an error, and `spec/compilation` said a themeless
  struct's walls lower to air. The catalog row now stops at what resolves, the passages say what
  each role does without its material, an eave `stair` or `pressure_plate` higher than the shorter
  structure reaches is refused, and `E_UNKNOWN_SLOT_TARGET` is an error because the slot's value is
  wrong in the source.
