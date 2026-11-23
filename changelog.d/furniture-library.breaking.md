- *(core)* A `place` in a `struct` or `def` body is no longer `E_MISPLACED_MEMBER`.
- *(core)* `offset=`, `y=` and `facing=` on a `site`'s `place` are `W_IGNORED_ARGUMENT` instead of
  `E_UNKNOWN_ARGUMENT`, and so are `theme=`, `east_of=`, `north_of=` and `gap=` on a body's.
- *(core)* `ScopeResolution` gains the public field `placed`, so a struct literal of it no longer
  compiles.
