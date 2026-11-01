- *(core)* A source with a `pressure_plate` whose own `at=`, `offset=` or `y=` puts it on no voxel
  now fails `cairn check`, `cairn compile`, `cairn lower` and `cairn info` with `E_INVALID_MEMBER`
  and exit 1, where it passed with a `W_DEFERRED_MEMBER` warning, and the language server marks
  the line as an error. `W_DEFERRED_MEMBER` no longer covers those lines. `cairn synth` reads no
  block array and is unchanged.

  Listed here rather than as a fix under `spec/compatibility` C.4: `spec/redstone` "Signal binding"
  said a plate outside the interior earns `W_DEFERRED_MEMBER`, so the build did what the spec said,
  and the spec changed.
