- *(redstone)* A whole `cairn synth --stage <s>` dump from `placement` on reads back:
  `serde_json::from_str::<ScopedPlacementIr>(&dump)` returns the Placement IR the dump was written
  from, equal to it except that every span is empty. Only the cells had a reader before, so a tool
  reached into each scope's `cells` array itself and could not read the outputs, the region, the
  inputs or the signal definitions back. `PlacedOutputNode`, `PlacementIr`,
  `CircuitRegionReservation`, `NetlistInput`, `ScopeKind` and `Scoped<T>` / `ScopedEntry<T>` now
  have a `Deserialize`.

  An output is read as a cell is: its `stage` tag picks its phase and a payload key the tag
  disagrees with is refused. Its buffers stand on the `out` segment, so one on a driver port is
  refused too:

  ```text
  an output tagged `stage: "route"` has been through the route stage and must carry `wire_length`
  `buffer_coords[1]` stands on `a`, a cell's driver port, and an output's buffers stand on the `out` segment
  ```

  A key the dump leaves out because it is empty (`region`, `inputs`, `outputs`, `cells`,
  `signal_defs`) reads back empty. These are refused as deserialisation errors: an unknown key at
  any depth, an unknown scope kind, a key a node gives twice, a signal that `signal_defs` names
  twice, and a dotted name with no segment, an empty segment (`"sig."` as a `signal_defs` key,
  `["sig", ""]` as a name) or a segment holding a `.` (`["sig.a"]`). The reader checks each node,
  not the IR as a whole: a `NetRef` out of range, nodes at another stage than the pass expects or
  two cells on one `coord` read back, and `compile_routing`, `compile_delay` and
  `compile_crossing` panic on them, as their `# Panics` sections now say. Reading back is for
  self-describing formats such as JSON.
