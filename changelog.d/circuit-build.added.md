- *(redstone)* `build_circuits` takes each scope's `logic` through synthesis, the update-order
  check, placement, routing, delay insertion, legalization and the latency check for one edition,
  and lays every circuit in the layers its `circuit` line reserves under the struct:
  - each sensor enters the circuit by a pad under it and each actuator is driven by a riser, two
    torches carrying the signal up under it, so a component stands in the floor row `y = 0` or at
    `y = 1` over a block of the floor;
  - each cell's tile is laid at the free spot nearest the tiles its nets join, each net's dust runs
    up and down staircases apart from every other net's, and a buffer repeater goes in wherever an
    input would read its signal too weak;
  - a `delay` or `pulse` runs its operand through repeaters of one tick each, so it passes every
    change held for a stage as it came;
  - every part is written in the state the circuit rests in with its sensors off, its block the
    target's for `@redstone_dust.default`, `@redstone_torch.wall`, `@repeater.default`,
    `@comparator.default` or the new `@circuit.block`.
- *(cli)* `cairn lower --edition E` lowers the members E's guards hold and lays each struct's
  circuit, as `cairn compile --edition E` builds it.
- *(formats)* Both writers write a block's block entity, into its `blocks` entry's `nbt` in a `.nbt`
  and into `block_position_data` in a `.mcstructure`; the one a build writes is a Java circuit
  comparator's output strength, as `OutputSignal`. Bedrock writes a lit or powered redstone state
  with the block that holds it, such as `unlit_redstone_torch` and `powered_repeater`, through
  `TargetRegistry::spelling`.
