- *(core)* A source with a member its own line keeps from being built now fails `cairn check`,
  `cairn compile`, `cairn lower` and `cairn info` with `E_INVALID_MEMBER` and exit 1, where it
  passed with a `W_DEFERRED_MEMBER` warning, and the language server marks the line as an error.
  That covers a `side=` naming no wall, a `level` with no readable `y=`, `walls` with no positive
  `height=`, a `roof` whose `kind=` or shed `slope_to=` names nothing, a `window` missing or
  misreading `offset=`, `y=` or `size=`, with `repeat=0` or `step=0`, or running past its wall, a
  `door` whose `at=` names no anchor, a `door` or `window` whose rows no written wall course holds,
  an eave `stair` whose `kind=`, `facing=`, `half=` or `shape=` is outside its list or that sits
  above the struct or on one no roof is written with an `overhang=`, a `circuit` line that reserves
  no region, and an actuator patch that binds no door. `cairn synth` reads no block array and is
  unchanged.

  Listed here rather than as a fix under `spec/compatibility` C.4: `spec/lint` "Lowering" gave
  these lines `W_DEFERRED_MEMBER`, so the build did what the spec said, and the spec changed.
