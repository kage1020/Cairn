# cairn-redstone

Redstone for Cairn: turns a *signal graph* into voxels, then verifies the result with a headless
per-tick simulator.

Per [redstone](https://kage1020.github.io/Cairn/spec/redstone/), the logical redstone surface (Tier 1) is the application
where Cairn's "declare intent, the compiler resolves the physics" thesis pays off most. Signal
attenuation, crosstalk, delay, and the Java/Bedrock divergence — all things an LLM handles poorly —
are derived deterministically from a small dataflow description.

## Status

Skeleton. None of the IR layers, the cell library, or the simulator are implemented yet.

## Pipeline

The crate is built around four IR layers and the cell library that sits between them
([redstone §14.8](https://kage1020.github.io/Cairn/spec/redstone/), [architecture §3.3](https://kage1020.github.io/Cairn/spec/architecture/)):

```
Intent IR        logic declarations / circuit region / signal binding
   ↓ logic_synth
Logic IR         logical expressions / dependency DAG (edition-neutral, zero delay)
   ↓
Netlist IR       cells / nets (logical cell selection; still carries no delay)
   ↓ logic_place
Placement IR     cell coordinates + actual wire length — delay/tick first determined here
   ↓ logic_route
block-array IR   voxel reality of dust/repeater/torch/comparator
```

The cell library is three-tier (`Logical Cell → Edition Cell → Physical Tile`), confining the
Java/Bedrock difference to the library alone
([redstone §14.6](https://kage1020.github.io/Cairn/spec/redstone/)).

## v1 scope

- **Combinational**: `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux`.
- **Curated sequential macros**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` /
  `counter`.
- **Verification**: truth-table, latency, and bounded-eventually temporal assertions
  ([redstone §14.7](https://kage1020.github.io/Cairn/spec/redstone/)).

Out of scope for v1 (drops to Tier 0 placement or `raw`): general FSMs, CPU-class clocked
assignment, quasi-connectivity / BUD / update-order sensitive circuits
([redstone §14.6](https://kage1020.github.io/Cairn/spec/redstone/)).

## Verification loop

`synth → sim → diff → patch`. The patch may rewrite only placement hints, routing, and buffer
repeaters; **the Logic IR is never auto-modified**
([redstone §14.7](https://kage1020.github.io/Cairn/spec/redstone/)). The simulator runs per target edition, so a single
declaration is checked against both Java and Bedrock implementations.

## Dependencies

- [`cairn-core`](../cairn-core/README.md) for sensor/actuator placement and the block-array IR.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
