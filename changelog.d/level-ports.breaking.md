- *(core)* A `connect` port id carried both by a member of the `def` body and by a member directly
  under one of its `level`s resolved to the body member, and is now `E_AMBIGUOUS_PORT`, since both
  are ports. A port under a `level` inside a `level` is still `E_UNRESOLVED_PORT`, with a message
  that names the nesting.
