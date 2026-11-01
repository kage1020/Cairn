- *(core)* A `pressure_plate` its own line puts on no voxel is refused with `E_INVALID_MEMBER`
  rather than built around with `W_DEFERRED_MEMBER`, so a structure no longer builds at exit 0
  without it, as `spec/lint` "Error vs warning" requires of a dropped member. The lines it covers
  are no `at=` or one naming no anchor, an `offset=` or `y=` that is not a non-negative integer, an
  `offset=` past the wall, a row above every row the struct as written reaches, an
  `at=<side>.outside` on a struct where no roof has an `overhang=` of at least 1, and an
  `at=inside.<side>` at a corner or on a struct under 3 on an axis. Each names the values that
  would be valid and the change that fixes it. An `at=` one typo or a swap away from an anchor,
  such as `at=front.inside`, is offered that anchor, and a bare side such as `at=front` is offered
  both anchors on it. A plate that only a `walls` member painting nothing, a `roof` that draws
  nothing or a `level` that is not placed keeps off its voxel stays `W_DEFERRED_MEMBER`, with a
  note saying which of them has the repair. Every plate left unplaced that carries a `->` binding,
  the state-literal `mat_slot=` and pack-default `E_UNKNOWN_ID` refusals included, now notes that
  its signal still reaches the netlist with no plate to drive it.
