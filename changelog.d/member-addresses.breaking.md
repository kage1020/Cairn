- *(core)* The `members` record of each member that built a cell, a struct's `circuit` included,
  carries the member's `address`, which `cairn lower --format json` prints and `resolved_ir_hash`
  covers. A rebuild of a source that builds a cell therefore writes a `resolved_ir_hash` other than
  the one an earlier release wrote for it. `ResolvedMember` gains `address`, so a struct literal of
  it no longer compiles, and `BlockArrayIr::attach_circuit` takes the `circuit` line's address
  after its span.
