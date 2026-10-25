- *(core)* A `pressure_plate` its own line puts on no voxel is refused with the new error
  `E_UNPLACEABLE_PLATE` rather than built around with `W_DEFERRED_MEMBER`, so a structure no
  longer builds at exit 0 without it, as `spec/lint` "Error vs warning" requires of a dropped
  member. The lines it covers are no `at=` or one naming no anchor, an `offset=` or `y=` that is not
  a non-negative integer, an `offset=` past the wall, a row at or above the struct's height, an
  `at=<side>.outside` above `y=0` on a struct no roof gives an overhang, and an `at=inside.<side>`
  at a corner or on a struct under 3 on an axis. Each names the values that would be valid and the
  change that fixes it. An `at=` one typo or a swap away from an anchor, such as `at=front.inside`,
  is offered that anchor, and a bare side such as `at=front` is offered both anchors on it. A plate
  that only a `walls` member painting nothing, or a `roof` written with an `overhang=` that draws
  nothing, keeps off its voxel stays `W_DEFERRED_MEMBER`, with a note saying whether the `walls` or
  the `roof` has the repair. Every plate left unplaced that carries a `->` binding, the
  state-literal `mat_slot=` and pack-default `E_UNKNOWN_ID` refusals included, now notes that its
  signal still reaches the netlist with no plate to drive it.
