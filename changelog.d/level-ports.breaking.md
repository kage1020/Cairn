- *(core)* A `connect` port id carried both by a member of the `def` body and by a member directly
  under one of its `level`s resolved to the body member, and is now `E_AMBIGUOUS_PORT`, since both
  are ports. A port under a `level` inside a `level` is still `E_UNRESOLVED_PORT`, with a message
  that names the nesting.
- *(core)* A walkway no longer runs through a building it joins. On a row from a placement's
  ground row to its top, every cell of its `size=` footprint is in the router's way, the room
  inside its walls as well as the walls, so a strip that crossed the room of a building with no
  floor, or ran in at a window, now goes round it, and its lockfile entry changes.
