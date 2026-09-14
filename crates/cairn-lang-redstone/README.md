# cairn-lang-redstone

Redstone for Cairn: lowers a *signal graph* toward voxels, and will verify the result with a headless per-tick simulator. The crate stops at a placed and routed layout today — emitting the blocks and simulating them are both still ahead.

Per [redstone](https://cairn.kage1020.com/spec/redstone/), the logical redstone surface is where Cairn's "declare intent, the compiler resolves the physics" thesis pays off most. Signal attenuation, crosstalk, delay, and the Java/Bedrock divergence — all things an LLM handles poorly — are derived deterministically from a small dataflow description.

## Status

The synthesis half of the pipeline is in place; the verification half is not. Nothing here reaches a compiled artifact yet — the stages are reachable only through `cairn synth --experimental-logic-synth`, whose output shape is explicitly unstable.

| Pass | Entry point | What it produces |
|---|---|---|
| Logic synthesis | `synthesize(&IntentModule)` | An edition-neutral Logic IR DAG from sensor bindings, actuator arguments, and `logic sig.X = <expr>` lines |
| Netlist | `compile_netlist(&ScopedLogicIr)` | Logical Cells and nets, still edition-neutral and delay-free |
| Edition selection | `compile_edition_netlist(&ScopedNetlistIr, Edition)` | The target-edition realisation of each cell — Java `ComparatorAnd` / `RepeaterOr` / `InverterTorch`, Bedrock `TorchAnd` / `TorchOr` / `InverterTorch` |
| Placement | `compile_placement(&ScopedEditionNetlistIr, &IntentModule)` | Cell coordinates inside each scope's `circuit region=` reservation |
| Routing | `compile_routing(&ScopedPlacementIr)` | Steiner trees per net, filling each cell's `wire_length` |
| Delay insertion | `compile_delay(&ScopedPlacementIr)` | Each cell's `delay_ticks`, base delay plus the implicit buffer repeaters; refuses with `E_ATTENUATION_LIMIT` past the v1 sanity cap |
| Crossing legalization | `compile_crossing(&ScopedPlacementIr)` | Each cell's `buffer_coords`, the repeater every driver segment passes through |

Two nets that would merge into one strand of dust never need legalizing: routing already goes around the dust laid by earlier nets *and* around the coords beside it, since dust reads the dust one step away in its own plane. The short is not made in the first place.

Every placed cell records which of the last four passes touched it as a `PlacementStage`, dumped as a `"stage"` key in the same vocabulary `cairn synth --stage <s>` accepts, so a JSON consumer reads the stage off the output rather than inferring it from which optional keys are present.

Still to come: edition legalization, the tick simulator, and the QC/BUD refusal (`E_NO_PORTABLE_IMPL`).

## Pipeline

Four IR layers, with the cell library sitting between them ([redstone §14.8](https://cairn.kage1020.com/spec/redstone/), [architecture §3.3](https://cairn.kage1020.com/spec/architecture/)):

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

The cell library is three-tier (`Logical Cell → Edition Cell → Physical Tile`), which confines every Java/Bedrock difference to the library alone ([redstone §14.6](https://cairn.kage1020.com/spec/redstone/)).

## v1 scope

- **Combinational**: `and` / `or` / `not` are synthesised end to end. `xor` / `nand` / `nor` / `mux` are reserved on the Logic IR and Netlist IR enums, but no surface syntax produces them yet.
- **Curated sequential macros**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`. Named in the spec, implemented nowhere yet.
- **Verification**: truth-table, latency, and bounded-eventually temporal assertions ([redstone §14.7](https://cairn.kage1020.com/spec/redstone/)). An `assert` is checked for shape and for signals it names that nothing defines; evaluating one waits on the simulator.

Out of scope for v1, and dropped to plain placement or `raw`: general FSMs, CPU-class clocked assignment, and quasi-connectivity / BUD / update-order sensitive circuits ([redstone §14.6](https://cairn.kage1020.com/spec/redstone/)).

## Verification loop

`synth → sim → diff → patch`. The patch may rewrite placement hints, routing, and buffer repeaters only; **the Logic IR is never auto-modified** ([redstone §14.7](https://cairn.kage1020.com/spec/redstone/)). The simulator is to run per target edition, so one declaration gets checked against both the Java and the Bedrock implementation. Only the synthesis half of that loop exists today.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for sensor/actuator placement and the block-array IR.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
