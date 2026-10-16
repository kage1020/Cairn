- *(core)* `W_NO_THEME_BOUND` said "every `mat_slot=` will lower to air", which is true of a
  `floor` only. It now reads "struct `s` has no theme bound, so no `mat_slot=` in it resolves to a
  block", and a new first note says what each role does instead: a `floor` is left air; no `walls`
  is built, so every `door` and `window` is refused for want of a wall; a `roof`, eave `stair` or
  `pressure_plate` is built from its default block. The code and severity are unchanged.
- *(spec)* `spec/lint` said the same in `W_NO_THEME_BOUND`'s catalog row and in the two passages
  that explain why `E_UNKNOWN_SLOT_TARGET` is an error. All three now say what each role does
  without its material, and the reason for the error is reworded to match.
