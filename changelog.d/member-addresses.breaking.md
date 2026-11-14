- *(core)* Each `members` record of the block-array IR carries its member's `address`, which
  `cairn lower --format json` prints and `resolved_ir_hash` covers, so every lock's hash moves: a
  rebuild writes a `resolved_ir_hash` other than the one an earlier release wrote for the same
  source. `ResolvedMember` gains `address`, so a struct literal of it no longer compiles, and
  `BlockArrayIr::attach_circuit` takes the `circuit` line's address after its span.
