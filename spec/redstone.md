# 14. Redstone (logic circuits)

Cairn can describe redstone at the **logic level**. The author declares a **signal graph (dataflow)**,
and the compiler **synthesizes → places → routes (place-and-route)** the actual dust/repeaters/torches/
comparators into voxels. This is the application where P1 (declare intent, the compiler resolves the
physics) pays off most: signal attenuation, crosstalk, and delay calculation — physics an AI handles
even worse than voxel building — are derived deterministically from the logic description.

**Design core**: the first-class object of the logic layer is not "behavior" but the **signal
dependency graph (an IR-able dataflow)**. Time is not carried in the language core (14.4). This is what
best aligns with P1/P3/P5.

## 14.1 The two-tier model and the v1 boundary (replaces the old "non-goal")
- **Tier 0 physical placement**: `repeater facing=north delay=2`, etc. The author places parts and the
  blockstate is derived. Behavior is not modeled ([blockstate.md](blockstate.md)).
- **Tier 1 logic (this chapter)**: declare a signal graph; the compiler turns it into voxels via
  synthesis → placement → routing.

The only new keywords are **`logic` / `circuit` / `assert`**. Logic primitives are provided as a
**built-in `def` library**, preserving the small closed vocabulary (P3).

v1 scope (in Verilog terms, only `assign`-equivalent is allowed; clocked assignment is not):
- **○ Combinational**: `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux`
- **○ Curated sequential macros**: `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`
- **× Out of scope (→ Tier 0 / raw)**: `always` / `process` / `state` / `case` / FSM / clocked
  assignment / CPU and other general sequential synthesis.

## 14.2 Signal binding (sensor → signal graph → actuator)
Sensors emit signals; actuators consume them. Both are physical members
([components-editing-sites.md](components-editing-sites.md)) placed in earlier phases.
```
# sensor → signal
lever      id=sw   side=front offset=2 y=1 -> sig.power
button     id=bt   side=front               -> sig.ring
daylight   id=dl   at=..                     -> sig.day
observer   id=ob   at=.. facing=down         -> sig.tick

# actuator ← signal
lamp       id=l1   at=..  lit_by=sig.lamps
piston     id=p1   at=..  powered_by=sig.mem facing=up sticky=true
door       id=d1   ..     opened_by=sig.power
dispenser  id=ds   at=..  fired_by=sig.pulse facing=south
```

## 14.3 The logic layer = a signal dependency graph (DAG)
The author writes dependencies among signals (boolean combination + macro application). This is a pure,
time-free dataflow that becomes a Logic IR (DAG) inside the compiler.
```
logic sig.lamps = sig.power and not sig.day
logic sig.mem   = latch(set=sig.a, reset=sig.b)   # RS latch (macro)
logic sig.pulse = pulse(sig.ring, 4)              # monostable: 4 stages (→ expanded into repeater stages internally)
logic sig.fire  = edge_rising(sig.tick)
logic sig.sel   = mux(sel=sig.s, a=sig.x, b=sig.y)
```
- The logic expression itself contains no time arithmetic (14.4). The `4` in `pulse(sig.ring, 4)` is a
  **stage count**, not a tick value.

## 14.4 Time model: not carried in the language core
- In v1, only **macros (`delay`/`pulse`/`edge`/`latch`/`counter`) carry time**. `delay(3)` is a cell
  macro that lowers to `Repeater×3` internally. **It is not a DSL where you write a tick operator.**
- **Delay is carried in neither the Logic IR nor the Netlist IR. It is determined for the first time in
  the Placement IR** (14.8). `and` is logically zero-delay, but the tick count is only known after cell
  selection (`and → ComparatorAND(Java)`) and the actual post-placement wire length.
- A number appears as time (ticks) **only in verification assertions** (14.7). The author never does
  tick arithmetic inside a logic expression.

## 14.5 Place-and-route: 2D to the DSL, pseudo-2.5D internally
The user is shown a 2D mental model, but a purely 2D floorplan gets stuck, so the **internal
implementation is pseudo-2.5D, handling crossings, fanout, and wire length**. It holds the three
concepts `plane` / `via` / `bridge` internally (not exposed in the DSL).
- Circuit classes a pure 2D model cannot handle: **fanout / bus / crossing / comparator feedback /
  observer chain**.
- The internal algorithm is five stages: **Placement → Steiner routing → Delay insertion → Crossing
  legalization → Edition legalization**.
  - placement: topological order, left→right.
  - routing: Manhattan. Crossings escape to a `bridge tile` or a vertical layer. Fanout builds a tree.
  - delay insertion: insert a repeater as a buffer only where a segment exceeds the signal attenuation
    limit of 15.
- Routing is confined to the `circuit` region; if it does not fit, fail-loud (report congestion = area
  shortage).
```
circuit region=basement void=3       # reserve a 3-high service layer; route the synthesized circuit here
```
```text
E_ROUTE_CONGESTION line 21 circuit=basement:
  synthesized netlist needs ~3.2x the reserved area (void=3, region 9x7).
  Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks.
```

## 14.6 Edition differences: absorbed by the cell library; QC/BUD cannot be synthesized
The cell library is three-tier, **confining the edition difference to the cell library alone**:
```
Logical Cell → Edition Cell → Physical Tile
  AND        → Java:    ComparatorAND → block array
             → Bedrock: TorchAND      → block array
```
- **Absorbed (○)**: repeater / observer / comparator / orientation (cell-implementation differences).
- **Not absorbed (×)**: QC (quasi-connectivity) / BUD / update order / quasi-connectivity. These depend
  on the implicit semantics of block-update order, for which no portable implementation exists.
- If the logic requires update-order semantics, it is a **compile error (cannot be synthesized)**. This
  is consistent with "recompile, don't transcode" (P1 / [versioning-editions.md](versioning-editions.md)).
```text
E_NO_PORTABLE_IMPL line 15:
  this circuit requires update-order (quasi-connectivity / BUD) semantics.
  No portable redstone implementation exists for the target edition.
  Fix: redesign the logic to be order-independent, or drop to Tier 0 with an @edition guard.
```
- The logic is edition-neutral; the synthesized circuit is edition-specific. Hand-placed redstone breaks
  across editions, but with a logic description the compiler can emit an edition-correct circuit — the
  biggest motivation for logic description.

## 14.7 Verification: check three assertion kinds against a tick simulator (extends [evaluation.md](evaluation.md))
Declare the intent, then **simulate the synthesized circuit per tick (headless)** and check it. There
are three assertion kinds:
```
# combinational: truth table
assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->0 }

# latency (important because P&R changes delay)
assert latency(sig.in -> sig.out) <= 4

# temporal (not full LTL — only bounded eventually)
assert always(sig.button -> eventually sig.door_open within 8)
```
- The self-correction loop (P5) is **synth → sim → diff → patch**. Verification runs **per target
  edition**.
- **The patch targets only P&R / placement hints, repeaters, and buffers. The logic (Logic IR) is never
  rewritten** (self-correction that auto-modifies logic is dangerous).
```text
E_SIM_ASSERTION_FAILED edition=bedrock:
  assert latency(sig.in -> sig.out) <= 4, but measured 6 (extra repeaters from crossing legalization).
  Patch target: placement hint / route. (logic is never auto-modified)
  Suggested: relax to <=6, enlarge circuit void to shorten routes, or pin cell placement.
```

## 14.8 Connection to the IR and phases
For logic descriptions, **three IR layers** sit between the Intent and the block-array
([architecture.md](architecture.md)). They are separated because their roles differ (standard in HDL):
```
Intent IR        (logic declarations / circuit region / signal binding)
   ↓ logic_synth
Logic IR         (logical expressions / dependency DAG. Edition-neutral, zero delay)
   ↓
Netlist IR       (cells/nets. Logical Cell selection. Still carries no delay)
   ↓ logic_place
Placement IR     (cell coordinates + actual wire length → delay/tick determined here for the first time)
   ↓ logic_route
block-array IR   (voxel reality of dust/repeater/torch/comparator)
```
The phase model ([compilation.md](compilation.md)) splits the step right after `fixtures` into three:
```
massing → envelope → openings → fixtures → logic_synth → logic_place → logic_route → raw
```
Only once `fixtures` (sensors/actuators) are placed in 3D do the I/O port absolute coordinates become
fixed, enabling placement and routing. **Delay is not carried in the Logic IR / Netlist IR; it is
determined for the first time in the Placement IR** (14.4).

## 14.9 Reverse conversion
In v1, hand-built redstone imported from a schematic ([ecosystem-interop.md](ecosystem-interop.md)) is
kept as **Tier 0 raw**. Reverse-synthesizing logic from a mass of dust is out of scope for v1
(consistent with the generation-first, lossy approach).
