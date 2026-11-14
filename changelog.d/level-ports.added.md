- *(core)* A `door` or `window` directly under a `level` is a `connect` port. It sits one block
  outside its wall on that level's base row, so a building written with `level y=0` for its ground
  floor can have its front door joined by a walkway. Its doorway's rows are measured from the
  level, as the cut measures them. Two ports under a `level` of one `y=` are joined by a strip on
  that row, routed around what the placements hold there. Two ports on different rows lay nothing
  and are `W_DEFERRED_MEMBER`, since a walkway does not climb.
- *(core)* `stair` and `roof` are no longer reserved as port roles: neither opens a wall for a
  strip to enter by, so a port on one is refused for good, and the note no longer calls it
  reserved.
