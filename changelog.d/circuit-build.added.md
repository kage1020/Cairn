- *(redstone)* `build_circuits` takes each scope's `logic` through synthesis, the update-order
  check, placement, routing, delay insertion, legalization and the latency check for one edition,
  and lays every circuit in the layers its `circuit` line reserves under the struct:
  - each sensor enters the circuit by a pad under it and each actuator is driven by a riser, two
    torches carrying the signal up under it, so a component stands in the floor row `y = 0` or at
    `y = 1` over a floor block that passes power;
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
  and into `block_position_data` in a `.mcstructure`, such as a Java circuit comparator's output
  strength, as `OutputSignal`. Bedrock writes a lit or powered redstone state
  with the block that holds it, such as `unlit_redstone_torch` and `powered_repeater`, through
  `TargetRegistry::spelling`.
- *(formats)* Both built-in packs carry a `conductors` component, per version: which states of
  which blocks conduct redstone power, so that a plate's power or a riser torch's reaches the
  other side of the floor block between them. Java's is extracted by asking the game of each block
  state; Bedrock's is written from what its game is known to do, and a row with
  `"unsettled": true` names the blocks, such as mud, a spawner and a command block, whose Bedrock
  conduction no source settles. `PackView::conducts`, behind the new `TargetRegistry::conducts`,
  answers from it at a pinned version or, unpinned, with what every version that declares the
  block agrees on, and reads a Bedrock id as the canonical state of each block it is written from,
  so `oak_double_slab` conducts as `oak_slab[type=double]`. It answers `None` where the table
  cannot say: without the table, for an unsettled block, for a Bedrock block no canonical block is
  written as, and for a state the version does not give the block. The loader refuses a component
  that misses a targetable version, an empty row, a lookup that reads or maps nothing, maps no
  state to `true`, gives a key the wrong number of values or leaves out a state its block takes,
  an unsettled row with a lookup or defaults, and a block or state value the version's block table
  or state map does not give.
