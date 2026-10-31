- *(core)* An `at=<side>.outside` pressure plate at `y=0` on a struct with no roof overhang was
  placed in the floor cell under the wall column, where the wall above buried it and its signal
  could never fire, and nothing was reported. It is now refused with `E_UNPLACEABLE_PLATE` on every
  row, since the ring a roof's `overhang=` adds is the only exterior a struct has. The fix it offers
  is a `roof` with an `overhang=` of at least 1, or `at=inside.<side>` at an interior `offset=`;
  it no longer offers `y=0`. An `at=inside.<side>` plate on a struct with no interior is offered
  `at=<side>.outside` only where a roof draws an overhang. The `redstone-door` and `crossbar`
  examples give their structs a flat roof with `overhang=1`, and `redstone-door` puts one plate on
  each side of its door.
