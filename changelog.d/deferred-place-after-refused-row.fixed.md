- *(core)* A `place` row refused with `E_INVALID_PLACE_ID` drew a second error on every later row
  whose `east_of=` / `north_of=` named it: `E_UNRESOLVED_PLACE_REF`, with a note telling the author
  to declare the target above that line, which it already was. That row is now `W_DEFERRED_PLACE`,
  a warning whose note points at the refused row, where the repair is, and a `connect` naming it
  still gets `W_DEFERRED_CONNECT`. A reference above the refused row names no prior place and is
  still `E_UNRESOLVED_PLACE_REF`.
