- *(core)* A `connect` naming a door or window declared under a `level` was refused as a port the
  def does not declare, with a note to add the `id=` the member already had. A port is still looked
  up among the def body's own members only, but this `E_UNRESOLVED_PORT` now says the member is
  declared under a `level`, naming it by its `id=` when it has one, and that a member there cannot
  be a port yet.
