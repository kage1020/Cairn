- *(core)* A `door` whose wall course holds the row it opens at but not the one above is
  `E_INVALID_MEMBER`, where it cut a one-row opening before, and a walkway anchored on it is
  skipped for that port. A door's `mat_slot=` is read, so `W_IGNORED_ARGUMENT` no longer reports
  it, and a slot bound to a block that is not a door, such as a wall material, is now
  `E_INCOMPATIBLE_MATERIAL`.
- *(core)* A structure with a door holds door blocks where it held air, so its palette and its
  `resolved_state` differ from one built before, and both built-in packs' hashes move with the
  new `door.default` row.
