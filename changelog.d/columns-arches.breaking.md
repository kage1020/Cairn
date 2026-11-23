- *(core)* `column` and `arch` are no longer `E_UNKNOWN_KEYWORD`.
- *(core)* A window's `shape=` is no longer `W_IGNORED_ARGUMENT` and ignored: `shape=arch` on a
  window narrower than 3 or lower than 2, or on a `repeat=` series whose `step=` is less than the
  window's width, and any identifier other than `rect` or `arch`, are `E_INVALID_MEMBER`, and the
  window is not cut.
- *(core)* `MemberRole` gains `Column` and `Arch`, so an exhaustive match on it no longer compiles.
- *(formats)* Both built-in packs' hashes move with the new `column.default` and `arch.default`
  rows.
