- *(core)* A source with a `pressure_plate at=<side>.outside` at `y=0` on a struct with no roof
  written with an `overhang=` of at least 1 now fails `cairn check`, `cairn compile`, `cairn lower`
  and `cairn info` with `E_INVALID_MEMBER` and exit 1, where it passed and placed the plate under
  the wall column. The language server marks the line as an error. `cairn synth` reads no block
  array and is unchanged.

  Listed here rather than as a fix under `spec/compatibility` C.4: `spec/redstone` "Signal binding"
  refused such a plate only above `y = 0`, so the build did what the spec said, and the spec
  changed.
