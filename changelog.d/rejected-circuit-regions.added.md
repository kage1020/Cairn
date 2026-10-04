- *(core)* `circuit_lines` reads each `circuit` line at the top level of a `struct` or `def`, or
  under a `level` in one, into the `CircuitRegion` it reserves or a `RejectedCircuitRegion` whose
  `CircuitRegionDefect` says why it reserves nothing; the defect's `Display` is that reason as a
  clause. `circuit_regions` returns the same reservations as before, out of the same walk.
