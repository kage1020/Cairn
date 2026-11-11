- *(cli)* `cairn compile`, `check`, `info` and `lower --edition` build every struct's circuit, so a
  source they passed before can now fail with a redstone code, each now in `spec/lint` "Diagnostic
  codes":
  - a struct or def with a cell to build or an actuator to drive and no `circuit` line is
    `E_NO_CIRCUIT_REGION`, where it built its components with no circuit between them;
  - a `void=` of 1 or 2, or of 3 with a `counter`, is `E_ROUTE_CONGESTION`;
  - a `pulse` of more than 6 stages or a `counter` of more than 4 is `E_ATTENUATION_LIMIT`, and a
    `delay` whose row of two repeaters a stage is longer than the struct is `E_ROUTE_CONGESTION`;
  - a sensor or actuator the circuit under the floor cannot reach, such as a lever above the floor
    row, is `E_ROUTE_UNREACHABLE_COMPONENT`;
  - an `assert latency` the laid circuit misses is `E_SIM_ASSERTION_FAILED`;
  - a target whose pack declares no block for a circuit token, such as the new `@circuit.block`, is
    `E_UNKNOWN_ABSTRACT_TOKEN` on the `circuit` line.

  Without a pinned edition, `check` and `info` report a circuit finding when every edition raises it
  at the same place, or as an edition's own when it or one of its notes lies on a line under that
  edition's guards, and `info` reports each edition's own under that edition.
- *(core)* A struct with a circuit is `void` layers taller, every member standing over the circuit,
  and a `def` placed in a `site` has its origin lowered by as many, so its structure file, its
  `cairn lower` output and its lockfile hash differ from one built before.
  `RenderedDiagnostic::code` is a `&'static str`, so a finding of the redstone crate renders through
  it too, and `BlockEntity` gains `fields`, so a struct literal of it no longer compiles.
- *(redstone)* Place-and-route lays the circuit at full size in the layers under the floor. The
  `plane` / `via` / `bridge` model is gone, and with it `W_ROUTE_CROSS_LAYER_CLEARANCE` and the
  straight-line refusal of a sink past 256 steps. `cairn synth --stage crossing` is
  `--stage legalize`, and the four layout stages print a `CircuitLayout` of tiles, nets and blocks
  instead of placed cells with `wire_length`, `local_delay_ticks` and `buffer_coords`. In the Rust
  API, `compile_crossing`, `CrossingOutput`, `MAX_ATTENUATION_SEGMENT`, `CELL_FOOTPRINT`,
  `PlacementIr`, `ScopedPlacementIr`, `ScopedPlacementIrEntry`, `PlacedCellNode`,
  `PlacedOutputNode`, `CellCoord`, `BufferCoord`, `BufferSegment`, `CircuitRegionReservation`,
  `RouteLayer`, `PlacementPhase`, `PlacementPhaseTransitionError` and `PlacementStage` are removed,
  `compile_placement` takes the `BlockArrayIr` the struct lowered to, and `compile_routing` and
  `compile_delay` take a `ScopedCircuitLayout`.
- *(redstone)* A Bedrock `or` is a `RepeaterOR`, as on Java: one tick from each port, restoring
  strength, where a dust merge took none and passed its strongest input on. A path into an actuator
  also counts the two ticks of the torches that carry the signal up to it, so `assert latency`
  bounds measure higher than before on both editions.
- `examples/control-room.crn`, `crossbar.crn` and `redstone-door.crn` stand their sensors and
  actuators where the circuit under the floor reaches them, reserve `void=3` or more, and bound
  their latency by what the laid circuit takes.
