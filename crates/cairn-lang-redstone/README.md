# cairn-lang-redstone

Redstone for Cairn: lowers a *signal graph* to the blocks of a circuit laid under each struct, and will verify the result with a headless per-tick simulator. The crate builds the circuit today; checking `truth` and `always` assertions against it is still ahead.

Per [redstone](https://cairn.kage1020.com/spec/redstone/), the logical redstone surface is where Cairn's "declare intent, the compiler resolves the physics" thesis pays off most. Signal attenuation, crosstalk, delay, and the Java/Bedrock divergence — all things an LLM handles poorly — are derived deterministically from a small dataflow description.

## Status

`build_circuits(&IntentModule, &mut BlockArrayIr, Edition, Option<&dyn TargetRegistry>)` runs every pass below for one edition and lays each legalized circuit in the layers its `circuit` line reserves under the struct, which rises over them. `cairn compile`, `check`, `info` and `lower --edition` call it; `cairn synth --experimental-logic-synth` prints each pass's output, whose shape is explicitly unstable.

| Pass | Entry point | What it produces |
|---|---|---|
| Logic synthesis | `synthesize(&IntentModule)` | An edition-neutral Logic IR DAG from sensor bindings, actuator arguments, and `logic sig.X = <expr>` lines |
| Netlist | `compile_netlist(&ScopedLogicIr)` | Logical Cells and nets, still edition-neutral and delay-free |
| Edition selection | `compile_edition_netlist(&ScopedNetlistIr, Edition)` | The target-edition realisation of each cell, such as Java `ComparatorAND` and Bedrock `TorchAND` |
| Portability | `verify_portable(&IntentModule, &BlockArrayIr, Edition)` | `E_NO_PORTABLE_IMPL` for the components of the built struct that read one another through a block update |
| Placement | `compile_placement(&ScopedEditionNetlistIr, &IntentModule, &BlockArrayIr)` | Each sensor's pad and each actuator's riser under it, then each cell's tile at the free spot nearest the tiles its nets join |
| Routing | `compile_routing(&ScopedCircuitLayout)` | Each net's dust from its driver to every input it feeds, up and down staircases, apart from every other net's |
| Delay insertion | `compile_delay(&ScopedCircuitLayout)` | A buffer repeater in a net's dust wherever an input would read its signal weaker than it needs |
| Legalization | `compile_legalize(&ScopedCircuitLayout, Option<&dyn TargetRegistry>)` | Every voxel as a block, in the state the circuit rests in with its sensors off |
| Latency | `verify_latency(&ScopedCircuitLayout, &IntentModule)` | `E_SIM_ASSERTION_FAILED` for each `assert latency` the legalized circuit's slowest path exceeds |

Each `CircuitLayout` records the last pass that filled it as a `LayoutStage`, dumped as a `"stage"` key in the vocabulary `cairn synth --stage <s>` accepts, so a JSON consumer reads the stage off the output rather than inferring it from which keys are present.

A cell counts its delay per input port (`EditionCell::port_delay_ticks`), and a combinational cell settles when the *last* of its inputs arrives, so an arrival time maxes over the ports, each the arrival on its net plus the buffer repeaters on the wire into the port and that port's delay. `verify_latency` walks it that way, on to the two torches of the riser under an actuator that reads the bound's end ([redstone "Verification"](https://cairn.kage1020.com/spec/redstone/)).

## Pipeline

Four IR layers, with the cell library sitting between them ([redstone "Connection to the IR and phases"](https://cairn.kage1020.com/spec/redstone/), [architecture "Redstone sub-layers"](https://cairn.kage1020.com/spec/architecture/)):

```
Intent IR        logic declarations / circuit region / signal binding
   ↓ logic_synth
Logic IR         logical expressions / dependency DAG (edition-neutral, no ticks)
   ↓
Netlist IR       cells / nets (Logical Cell selection; still edition-neutral, no ticks)
   ↓ Edition Cell selection (cell library tier 2; edition-tagged, still no delay)
   ↓ logic_place
Placement IR     every tile at its voxels: cells, pads under sensors, risers under actuators
   ↓ logic_route
Placement IR     + each net's dust and buffer repeaters, then every voxel's block — delay first determined here
   ↓
block-array IR   the struct, risen over the circuit laid under it
```

The cell library is three-tier (`Logical Cell → Edition Cell → Physical Tile`), which confines every Java/Bedrock difference to the library alone ([redstone "Edition differences"](https://cairn.kage1020.com/spec/redstone/)).

## v1 scope

- **Combinational**: `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux` are synthesised end to end, each to a cell on both editions.
- **Curated sequential macros**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter` are synthesised the same way, each to one cell of torches and repeaters on both editions, and a `latch` or `counter` keeps its feedback inside its cell, so the Logic IR stays a DAG. A macro's stage count travels with its cell, and its ticks are its Edition Cell's.
- **Verification**: truth-table, latency, and bounded-eventually temporal assertions ([redstone "Verification"](https://cairn.kage1020.com/spec/redstone/)). An `assert` is checked for shape and for signals it names that nothing defines; `verify_latency` checks each `assert latency` against the legalized circuit, and evaluating `truth` and `always` waits on the simulator.

Out of scope for v1, and dropped to plain placement or `raw`: general FSMs, CPU-class clocked assignment, and quasi-connectivity / BUD / update-order sensitive circuits, which `verify_portable` refuses ([redstone "Edition differences"](https://cairn.kage1020.com/spec/redstone/)).

## Verification loop

`synth → sim → diff → patch`. The patch may rewrite placement hints, routing, and buffer repeaters only; **the Logic IR is never auto-modified** ([redstone "Verification"](https://cairn.kage1020.com/spec/redstone/)). The simulator is to run per target edition, so one declaration gets checked against both the Java and the Bedrock implementation. Only the synthesis half of that loop exists today.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for sensor/actuator placement and the block-array IR.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
