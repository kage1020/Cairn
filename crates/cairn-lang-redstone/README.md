# cairn-lang-redstone

Redstone for Cairn: turns a *signal graph* into voxels, then verifies the result with a headless
per-tick simulator.

Per [redstone](https://cairn.kage1020.com/spec/redstone/), the logical redstone surface (Tier 1) is the application
where Cairn's "declare intent, the compiler resolves the physics" thesis pays off most. Signal
attenuation, crosstalk, delay, and the Java/Bedrock divergence — all things an LLM handles poorly —
are derived deterministically from a small dataflow description.

## Status

Combinational logic synthesis, Netlist IR selection, and Edition Cell
selection landed. `synthesize(&IntentModule)` lowers sensor bindings,
actuator arguments, and `logic sig.X = <expr>` lines into an
edition-neutral Logic IR (DAG of `and` / `or` / `not` gates today, with
`xor` / `nand` / `nor` / `mux` reserved on the enum until the surface
parser reaches them); `compile_netlist(&ScopedLogicIr)` rewrites that
DAG into an edition-neutral Netlist IR of Logical Cells + nets; and
`compile_edition_netlist(&ScopedNetlistIr, Edition)` picks the target-
edition realisation of each cell — the second rung of the three-tier
cell library that sits inside the Netlist → Placement transition (Java
`ComparatorAnd` / `RepeaterOr` / `InverterTorch` vs Bedrock `TorchAnd`
/ `TorchOr` / `InverterTorch`). Place-and-route, the tick simulator,
and QC/BUD refusal (`E_NO_PORTABLE_IMPL`) are still to come.

## Pipeline

The crate is built around four IR layers and the cell library that sits between them
([redstone §14.8](https://cairn.kage1020.com/spec/redstone/), [architecture §3.3](https://cairn.kage1020.com/spec/architecture/)):

```
Intent IR        logic declarations / circuit region / signal binding
   ↓ logic_synth
Logic IR         logical expressions / dependency DAG (edition-neutral, zero delay)
   ↓
Netlist IR       cells / nets (Logical Cell selection; still edition-neutral, zero delay)
   ↓ Edition Cell selection (cell library tier 2; edition-tagged, still no delay)
   ↓ logic_place
Placement IR     cell coordinates + actual wire length — delay/tick first determined here
   ↓ logic_route
block-array IR   voxel reality of dust/repeater/torch/comparator
```

The cell library is three-tier (`Logical Cell → Edition Cell → Physical Tile`), confining the
Java/Bedrock difference to the library alone
([redstone §14.6](https://cairn.kage1020.com/spec/redstone/)).

## v1 scope

- **Combinational**: `and` / `or` / `not` (landed, both Logic IR and Netlist IR) / `xor` / `nand` / `nor` / `mux` (Logic IR + Netlist IR shape only, synth path lands with the follow-up parser PR).
- **Curated sequential macros**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` /
  `counter`.
- **Verification**: truth-table, latency, and bounded-eventually temporal assertions
  ([redstone §14.7](https://cairn.kage1020.com/spec/redstone/)).

Out of scope for v1 (drops to Tier 0 placement or `raw`): general FSMs, CPU-class clocked
assignment, quasi-connectivity / BUD / update-order sensitive circuits
([redstone §14.6](https://cairn.kage1020.com/spec/redstone/)).

## Verification loop

`synth → sim → diff → patch`. The patch may rewrite only placement hints, routing, and buffer
repeaters; **the Logic IR is never auto-modified**
([redstone §14.7](https://cairn.kage1020.com/spec/redstone/)). The simulator runs per target edition, so a single
declaration is checked against both Java and Bedrock implementations.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for sensor/actuator placement and the block-array IR.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
