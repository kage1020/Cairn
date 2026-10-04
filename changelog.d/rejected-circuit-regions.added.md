- *(core)* `circuit_lines` reads every `circuit` line of every `struct` and `def`, including one
  under a `level`, into the `CircuitRegion` it reserves or a `RejectedCircuitRegion` whose
  `CircuitRegionDefect` says why it reserves nothing; the defect's `Display` is that reason as a
  clause. `circuit_regions` returns the same reservations as before, out of the same walk.
