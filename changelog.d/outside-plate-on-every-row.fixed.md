- *(core)* An `at=<side>.outside` pressure plate at `y=0` on a struct whose roofs draw no overhang
  was placed in the floor cell under the wall column, where the wall above buried it and its signal
  could never fire, and nothing was reported. On a struct with no roof written with an `overhang=`
  of at least 1 it is now refused with `E_UNPLACEABLE_PLATE`, since the ring a roof's `overhang=`
  adds is the only exterior a struct has. Beside a roof written with an `overhang=` that draws
  nothing, such as one with no `kind=` or one under a raised `level`, it is left unplaced with
  `W_DEFERRED_MEMBER`, whose note says the roof has the repair. The refusal offers a `roof` in the
  struct's body with an `overhang=` of at least 1, or `at=inside.<side>` at an interior `offset=`,
  at `y=1` where a `floor` takes row 0. A refusal of an outside plate's `offset=` or row adds the
  roof it needs too. The `redstone-door` and `crossbar` examples give their structs a flat roof with
  `overhang=1`, and `redstone-door` puts one plate on each side of its door.
