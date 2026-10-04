- *(core)* `W_NO_THEME_BOUND`, "every `mat_slot=` will lower to air", was reported on every scope
  with no theme bound, including an empty struct and one whose only member is a `roof kind=flat`
  built from its fallback material, neither of which has a `mat_slot=` to lower. It is now
  reported only on a scope with a member that carries a `mat_slot=`, one under a `level` included.
  A member the `level` drops, such as a `floor` above `y=0`, paints nothing whatever the theme and
  does not count.
