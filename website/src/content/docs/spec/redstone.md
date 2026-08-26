---
title: "14. Redstone (logic circuits)"
---

Cairn describes redstone at the **logic level**. You declare a signal graph; the compiler
synthesizes, places, and routes the actual dust, repeaters, torches, and comparators into voxels.

This is where P1 pays off most. Signal attenuation, crosstalk, and delay calculation are physics an
AI handles even worse than voxel building, and all three are derived deterministically from the
logic description.

The first-class object of the logic layer is the **signal dependency graph**, not behaviour. Time is
not carried in the language core ([§14.4](#144-time-model)).

## 14.1 Two tiers, and the v1 boundary

- **Tier 0 — physical placement.** `repeater facing=north delay=2` and the like. You place parts and
  the blockstate is derived; behaviour is not modeled ([Blockstate Model](blockstate)).
- **Tier 1 — logic (this chapter).** You declare a signal graph and the compiler turns it into
  voxels through synthesis → placement → routing.

The only new keywords are `logic`, `circuit`, and `assert`. Logic primitives ship as a built-in
`def` library, which keeps the vocabulary small and closed (P3).

In Verilog terms, v1 allows the `assign` equivalent and no clocked assignment:

| | Scope |
|---|---|
| **Combinational** | `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux` |
| **Curated sequential macros** | `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter` |
| **Out of scope** (→ Tier 0 or `raw`) | `always` / `process` / `state` / `case` / FSMs / clocked assignment / CPUs |

## 14.2 Signal binding

Sensors emit signals and actuators consume them. Both are physical members
([Components, Editing, and Multi-building](components-editing-sites)) placed in earlier phases.

```
# sensor → signal
lever      id=sw   side=front offset=2 y=1 -> sig.power
button     id=bt   side=front               -> sig.ring
daylight   id=dl   at=..                     -> sig.day
observer   id=ob   at=.. facing=down         -> sig.tick
pressure_plate id=pp at=front.outside offset=0 y=0 -> sig.step

# actuator ← signal
lamp       id=l1   at=..  lit_by=sig.lamps
piston     id=p1   at=..  powered_by=sig.mem facing=up sticky=true
door       id=d1   ..     opened_by=sig.power
dispenser  id=ds   at=..  fired_by=sig.pulse facing=south
```

That pairing is normative. A `-> sig.X` tail belongs to a sensor, and each actuator key belongs to
the one component that reads it. A binding written anywhere else is `E_LOGIC_MISPLACED_BINDING`:
`walls ... powered_by=sig.x` describes no circuit, and accepting it would put a port in the netlist
with no component behind it.

**Of the components above, only `door` and `pressure_plate` are accepted today.** `lit_by=`,
`powered_by=`, and `fired_by=` have no host yet and are refused wherever they are written.

**Signal names.** Sensors emit into the `sig.` namespace and actuators read from it, so a name
outside it can never be read — whether on the left of a `logic` line, in a sensor's `->` tail, or as
an actuator key's value. That is `E_LOGIC_INVALID_SIGNAL`. A name is `sig.` and exactly one segment
after it: `opened_by=a` is not a wire to a signal called `a`, and `opened_by=sig.a.b` names nothing
either.

The host is checked before the value, so `walls -> a` is one fault and it belongs to the host — no
way of writing the value makes `walls` a sensor.

**Bindings go after the `[selector]`, never inside it.** `door[id=front] opened_by=sig.power` binds;
`door[id=front,opened_by=sig.power]` does not. The brackets pick the member the line acts on, so
nothing written among them is read as a binding. A bracketed pair earns whichever finding still
applies once it is moved out — `E_LOGIC_MISPLACED_BINDING` naming the brackets when that is the only
problem, and otherwise the finding for the host or the key.

A `sig.` value under a key that is not one of the four actuator keys is
`E_LOGIC_UNKNOWN_BINDING_KEY`. The value says a signal was meant to be wired and the key says
nothing reads it — the shape a typo takes, as in `oepend_by=sig.power`.

## 14.3 The logic layer is a dependency DAG

You write dependencies among signals: boolean combination plus macro application. It is a pure,
time-free dataflow that becomes a Logic IR inside the compiler.

```
logic sig.lamps = sig.power and not sig.day
logic sig.mem   = latch(set=sig.a, reset=sig.b)   # RS latch (macro)
logic sig.pulse = pulse(sig.ring, 4)              # monostable: 4 stages
logic sig.fire  = edge_rising(sig.tick)
logic sig.sel   = mux(sel=sig.s, a=sig.x, b=sig.y)
```

The expression contains no time arithmetic. The `4` in `pulse(sig.ring, 4)` is a **stage count**,
not a tick value.

## 14.4 Time model

In v1 only the macros — `delay`, `pulse`, `edge`, `latch`, `counter` — carry time. `delay(3)` is a
cell macro that lowers to three repeaters internally. There is no tick operator to write.

**Delay is carried in neither the Logic IR nor the Netlist IR. It is determined for the first time
in the Placement IR** ([§14.8](#148-connection-to-the-ir-and-phases)). `and` is logically
zero-delay, but the tick count is known only after cell selection (`and → ComparatorAND` on Java)
and the actual post-placement wire length.

A number appears as ticks only in verification assertions ([§14.7](#147-verification)). You never do
tick arithmetic inside a logic expression.

## 14.5 Place-and-route

The DSL shows you a 2D mental model. A purely 2D floorplan gets stuck, so the internal
implementation is pseudo-2.5D, holding `plane` / `via` / `bridge` concepts that the DSL never
exposes. Circuit classes a pure 2D model cannot handle: fanout, bus, crossing, comparator feedback,
observer chain.

```
circuit region=basement void=3       # reserve a 3-high service layer; route the circuit here
```

The internal algorithm runs five stages:

1. **Placement** — topological order, left to right.
2. **Steiner routing** — Manhattan, around what is already standing. Cell bodies and I/O pads are
   reserved: dust cannot be drawn on one, and a signal cannot pass *through* one, since a component
   either emits or consumes. Every sink is therefore a leaf of its net's tree, and a fanout is a
   trunk beside the row with a tap into each sink rather than a chain through them. Where nothing is
   in the way the wire is a straight rectilinear run; otherwise it goes around, or climbs to a
   `bridge` layer inside the `void=<N>` budget.
3. **Delay insertion** — a repeater goes in as a buffer only where a segment exceeds the attenuation
   limit of 15. The segment is measured along the **routed** path from driver to sink, and the
   buffer stands on that path, so the straight line between the two is not always wire.
4. **Crossing legalization** — specified but unbuilt. What the pass lifts onto a `bridge` layer is a
   buffer repeater whose coordinate is taken, never the wire itself, so two nets sharing a wire
   coordinate stay one strand of dust carrying two signals. The pass **reports** that rather than
   passing it over, naming the pair of signals and the coordinate they meet on, one finding per
   pair. A reservation with no layer above the plane (`void=1`) has nowhere to lift onto, and the
   scope is refused.
5. **Edition legalization** — see [§14.6](#146-edition-differences).

Routing is confined to the `circuit` region. If it does not fit, the compiler fails loud. A sink
with no clear path from its driver — every way out walled in by a component — is the same refusal
for a different reason, and says which two coordinates it could not join.

```text
E_ROUTE_CONGESTION line 21 circuit=basement:
  synthesized netlist needs ~3.2x the reserved area (void=3, region 9x7).
  Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks.
```

## 14.6 Edition differences

A three-tier cell library confines the edition difference to the library alone:

```
Logical Cell → Edition Cell → Physical Tile
  AND        → Java:    ComparatorAND → block array
             → Bedrock: TorchAND      → block array
```

- **Absorbed**: repeater, observer, comparator, orientation — cell-implementation differences.
- **Not absorbed**: QC (quasi-connectivity), BUD, update order. These depend on the implicit
  semantics of block-update order, for which no portable implementation exists.

Logic that requires update-order semantics is a compile error rather than a silent footgun,
consistent with "recompile, don't transcode".

```text
E_NO_PORTABLE_IMPL line 15:
  this circuit requires update-order (quasi-connectivity / BUD) semantics.
  No portable redstone implementation exists for the target edition.
  Fix: redesign the logic to be order-independent, or drop to Tier 0 with an @edition guard.
```

Hand-placed redstone breaks across editions. With a logic description the compiler emits an
edition-correct circuit instead — the single biggest reason to describe logic rather than parts.

## 14.7 Verification

Declare the intent, then simulate the synthesized circuit per tick, headless, and check it. There
are three assertion kinds:

```
# combinational: truth table
assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->0 }

# latency — matters because place-and-route changes delay
assert latency(sig.in -> sig.out) <= 4

# temporal — bounded eventually only, not full LTL
assert always(sig.button -> eventually sig.door_open within 8)
```

The self-correction loop (P5) is **synth → sim → diff → patch**, and verification runs per target
edition. The patch targets place-and-route hints, repeaters, and buffers only. **The Logic IR is
never rewritten** — self-correction that auto-modifies logic is dangerous.

```text
E_SIM_ASSERTION_FAILED edition=bedrock:
  assert latency(sig.in -> sig.out) <= 4, but measured 6 (extra repeaters from crossing legalization).
  Patch target: placement hint / route. (logic is never auto-modified)
  Suggested: relax to <=6, enlarge circuit void to shorten routes, or pin cell placement.
```

## 14.8 Connection to the IR and phases

Three IR layers sit between the Intent IR and the block-array IR, separated the way HDL separates
them:

```
Intent IR        logic declarations / circuit region / signal binding
   ↓ logic_synth
Logic IR         logical expressions, dependency DAG. Edition-neutral, zero delay
   ↓
Netlist IR       cells and nets. Logical Cell selection. Still no delay
   ↓ logic_place
Placement IR     cell coordinates + actual wire length. Delay determined here
   ↓ logic_route
block-array IR   the voxel reality of dust, repeater, torch, comparator
```

The phase model ([Compilation Model](compilation)) splits the step right after `fixtures` into
`logic_synth → logic_place → logic_route`, because the I/O port coordinates are not fixed until
sensors and actuators are placed in 3D.

## 14.9 Reverse conversion

Hand-built redstone imported from a schematic ([Ecosystem Interop](ecosystem-interop)) is kept as
Tier 0 raw in v1. Reverse-synthesizing logic from a mass of dust is out of scope, consistent with
the generation-first, lossy approach.
