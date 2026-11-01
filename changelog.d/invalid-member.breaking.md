- *(core)* A source with a member its own line keeps from being built now fails `cairn check`,
  `cairn compile`, `cairn lower` and `cairn info` with exit 1, where it passed with a
  `W_DEFERRED_MEMBER` warning, and the language server marks the line as an error. The finding is
  the new `E_INVALID_MEMBER`, `DiagnosticCode::InvalidMember` in `cairn-lang-core`. It covers a
  `side=` naming no wall, a `level` with no member under it and no `y=` it can read, `walls` with
  no positive `height=`, a `roof` whose `kind=` or shed `slope_to=` names nothing, a `window` with
  no `y=` or `size=`, with an `offset=`, `y=`, `size=`, `repeat=` or `step=` it cannot read, with
  `repeat=0` or with `repeat=` above 1 and no positive `step=`, or running past its wall, a `door`
  whose `at=` names no anchor, a `door` or `window` whose rows no written wall course holds, an
  eave `stair` with no `kind=`, with a `kind=`, `facing=`, `half=` or `shape=` outside its list or
  a `y=` it cannot read, or that sits above the struct as written or on one where no roof has an
  `overhang=` of at least 1, a `circuit` line that reserves no region, and an actuator patch that
  binds no door. An `overhang=` that is not a non-negative integer counts as none, the default its
  `W_IGNORED_ARGUMENT` puts in its place. `cairn synth` reads no block array and is unchanged.

  Listed here rather than as a fix under `spec/compatibility` C.4: `spec/lint` "Lowering" gave
  these lines `W_DEFERRED_MEMBER`, so the build did what the spec said, and the spec changed.
