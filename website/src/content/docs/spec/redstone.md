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

- **Tier 0, physical placement.** You write `repeater facing=north delay=2` and the like, placing
  parts yourself while the compiler derives the blockstate. Behaviour is not modeled
  ([Blockstate Model](/spec/blockstate/)). None of these parts is a keyword yet, so such a line is
  `E_UNKNOWN_KEYWORD` today.
- **Tier 1, logic.** This chapter. You declare a signal graph and the compiler turns it into voxels
  through synthesis → placement → routing.

The only new keywords are `logic`, `circuit`, and `assert`. Logic primitives are not keywords: they
ship as a built-in library, which keeps the vocabulary small and closed (P3).

In Verilog terms, v1 allows the `assign` equivalent and no clocked assignment:

| | Scope |
|---|---|
| **Combinational** | `and` / `or` / `not` / `xor` / `nand` / `nor` / `mux` |
| **Curated sequential macros** | `latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter` |
| **Out of scope** (→ Tier 0 or `raw`) | `always` / `process` / `state` / `case` / FSMs / clocked assignment / CPUs |

**The built-in logic library.** Each entry is a primitive's name and its notation. A prefix
primitive stands before its one operand. An infix primitive stands between its two, and its rank
says how tightly it binds: a higher rank groups first, and one rank groups left to right. A called
primitive takes its operands in parentheses after its name, each named once as `name=expression`,
in any order, with a comma between two and none after the last. An applied primitive takes its one
operand in parentheses after its name, by position, and one that counts stages takes a stage count
after the operand and a comma, a whole number from 1 to 4294967295. The parser takes an
expression's shape from the library, so a primitive the library gains in one of these notations
needs no new syntax. The library holds thirteen primitives:

| Primitive | Notation |
|---|---|
| `or`, `nor` | infix, rank 0 |
| `xor` | infix, rank 1 |
| `and`, `nand` | infix, rank 2 |
| `not` | prefix |
| `mux` | called, `mux(sel=…, a=…, b=…)` |
| `latch` | called, `latch(set=…, reset=…)` |
| `edge_rising`, `edge_falling` | applied, `edge_rising(…)` |
| `delay`, `pulse`, `counter` | applied with a stage count, `delay(…, N)` |

The ranks follow the usual order of the bitwise operators, `and` before `xor` before `or`, and each
negated form takes the rank of the primitive it negates. A prefix primitive binds tighter than any
infix one, so `sig.a or sig.b and not sig.c` reads as `sig.a or (sig.b and (not sig.c))`, and
`sig.a nor sig.b xor sig.c nand sig.d` as `sig.a nor (sig.b xor (sig.c nand sig.d))`. Parentheses
group as written. A call is one operand, as a signal is, so `not mux(sel=sig.s, a=sig.x, b=sig.y)
and sig.z` reads as `(not mux(…)) and sig.z`, and each operand inside it is a whole expression. So
is an application, so `delay(sig.a and sig.b, 2)` delays the `and`.

`mux` is `a` while `sel` is off and `b` while it is on. A call that passes an operand by position,
leaves one out, names one twice or names one the primitive does not take is `E_PARSE`, and the
message names the operands the primitive takes, and for a positional or missing operand, the ones
still to name: `mux(sel=sig.s, sig.x, sig.y)` stops at the second `sig`, expecting `a=` or `b=`.

An applied primitive refuses the reverse, a named operand: `pulse(in=sig.x, 4)` stops at `in`. A
stage count is `E_PARSE` where it is wrong, and the message names the form the macro takes:
missing, as `delay(sig.x)` stops at `)` expecting `,`; written without its comma, as
`delay(sig.x 2)` stops at `2`; given to a primitive that counts none, as `edge_rising(sig.x, 2)`
stops at `,`; or outside 1 to 4294967295, as `pulse(sig.x, 0)` stops at `0`. What each macro
computes is the time model's ([§14.4](#144-time-model)).

A word the library does not hold is `E_PARSE` where it stands, and the message names what could
stand there: `sig.a xnor sig.b` stops at `xnor`, and `toggle(sig.x)` at its `(`, naming the
primitives the library calls or applies.

## 14.2 Signal binding

Sensors emit signals and actuators consume them. Both are physical members
([Components, Editing, and Multi-building](/spec/components-editing-sites/)) placed in earlier phases.

```
# sensor → signal
lever          id=sw at=inside.front offset=2 y=1 -> sig.power
button         id=bt at=front.outside offset=5 y=1 -> sig.ring
daylight       id=dl at=back.outside offset=3 y=0 -> sig.day
observer       id=ob at=inside.left offset=2 y=1 facing=down -> sig.tick
pressure_plate id=pp at=front.outside offset=1 y=0 -> sig.step

# actuator ← signal
lamp      id=l1 at=inside.back offset=3 y=2 lit_by=sig.lamps
piston    id=p1 at=inside.right offset=2 y=1 powered_by=sig.mem facing=up sticky=true
door      id=d1 side=front at=center opened_by=sig.power
dispenser id=ds at=front.outside offset=6 y=1 fired_by=sig.pulse facing=out
```

That pairing is normative. A `-> sig.X` tail belongs to a sensor, and each actuator key belongs to
the one component that reads it. A binding written anywhere else describes no circuit, and
accepting it would put a port in the netlist with no component behind it.

Which code says so is decided by what the finding needs in order to be made. A `->` tail on a member
that is not a sensor is `E_MISPLACED_BINDING`: whether a keyword may emit is a fact about the member
line, so `check` answers it and every command reports it. An actuator key on a component that does
not read it is a fact about the member line too, since the key is outside that member's vocabulary:
`walls opened_by=sig.power` is `E_UNKNOWN_ARGUMENT` ([Lint](/spec/lint/#syntax-and-structure)),
which `check` reports and every command stops on, with a note naming the component the key belongs
to. The synth pipeline's `E_LOGIC_MISPLACED_BINDING`, which names that component too, is reached
only where a `theme` selector has made the key one that member reads
([Lint](/spec/lint/#113-error-vs-warning)).

**Every component but `door` is anchored as a plate is.** A `lever`, `button`, `daylight`,
`observer`, `lamp`, `piston` or `dispenser` is one block set by a wall, written with the same `at=`,
`offset=` and `y=` as a `pressure_plate`. The rules below are stated for the plate and hold for each
of them alike: each is placed, refused or deferred wherever a plate would be, and its finding names
it.

**Where an `at=<side>.outside` plate may sit.** The plate takes the wall cell at `offset=` and sits
one voxel outward from it, in the ring a roof's `overhang=` adds around the walls
([Compilation Model §4.3](/spec/compilation/#43-gable-roof-voxel-rules)). That ring is the only
exterior a struct has, so where no roof draws an overhang the plate has no cell at any row, the
floor row `y = 0` included. It is not placed on the wall's own column instead, where the wall above
would bury it and its signal could never fire.

**Where an `at=inside.<side>` plate may sit.** The plate takes the wall cell at `offset=` and sits
one voxel inward from it, and that voxel must be strictly inside the wall ring: the footprint's
outermost row and column, where `walls` paint their courses. So `offset` runs from 1 to the wall's
length minus 2 (the length is `size.w` for `front` and `back`, `size.h` for `left` and `right`), and
a struct needs a `size` of at least 3 on both axes to have an interior at all. The ring is decided
from the footprint whether or not a `walls` member paints it, and the rule holds at every `y`,
including the floor row `y = 0`. A plate outside it is not placed.

**A plate that cannot be placed.** A plate whose own line puts it on no voxel is `E_INVALID_MEMBER`,
and the build stops. Its line does that with no `at=` or one other than `<side>.outside` or
`inside.<side>`, an `offset=` or `y=` that is not a non-negative integer, an `offset=` at or past
its wall's length, a row above every row the struct as written reaches ([Compilation Model
§4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)), an `at=<side>.outside` on a
struct where no roof has an `overhang=` of at least 1, or an `at=inside.<side>` outside the
interior. A plate the struct would have held had another member built is `W_DEFERRED_MEMBER`
instead, because the repair is on that member: a row that `walls` painting nothing, a `roof` drawing
nothing or `walls` under a `level` that is not placed would have reached, or an `at=<side>.outside`
beside a `roof` with an `overhang=` that draws nothing ([Lint](/spec/lint/#lowering)). The plate's
own line is judged first, so a line that puts it nowhere is the error even when such a member is
missing too. Either way synthesis reads the member line and not the voxels, so a binding of the
component still reaches the netlist with no component behind it, and the finding on its line says
so. That binding is a sensor's `->` tail, an actuator's key, or the key of a patch that binds the
actuator, wherever it names a `sig.` signal. One that synthesis refuses, such as a tail on an
actuator or a value outside `sig.`, reaches no netlist, and the finding does not mention it.

**How a component is turned.** A `lever` or `button` hangs on the face of the wall cell its anchor
names (`face=wall`), looking away from that wall: into the struct from `inside.<side>`, out of it
from `<side>.outside`. An `observer`, `piston` or `dispenser` looks away from its wall too unless
its `facing=` turns it: `in` and `out` are into and out of the struct, `left` and `right` run along
the wall toward and away from `offset=0`, and `up` and `down` are vertical. That is the way its
front looks: the cell an observer watches, the way a piston pushes and the way a dispenser fires.
Any other word, a compass direction among them, is `E_INVALID_MEMBER`, and the component is not
placed. The message lists the words, and for a compass direction it names the word that turns the
component that way from its wall, since a compass direction would mean a different thing on each
wall. A `daylight` and a `lamp` look no way and take no `facing=`.

`sticky=true` builds a `piston` as a sticky piston, and `sticky=false` or no `sticky=` as a plain
one. A `facing=` that is not a word, or a `sticky=` that is not `true` or `false`, is
`W_IGNORED_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)), and the component is built
as if it were not written.

Each component is built as it stands unpowered: a lever off, a button unpressed, a lamp unlit, a
piston retracted and a dispenser not triggered. The circuit powers it in the world, so none of
those states is written on its line, and `facing=` is the one state promoted to intent
([Blockstate Model §6.2](/spec/blockstate/#62-intent_state-and-resolved_state)).

**What a component is built from.** A `button` reads `mat_slot=`, and the slot must bind a button:
any other block is `E_INCOMPATIBLE_MATERIAL`, and the button is built from `@button.default` instead.
With no `mat_slot=` it is built from `@button.default`. Every other component is built from its
tier-1 tokens ([Materials and Themes](/spec/materials-themes/#tier-1-tokens)), a piston from
`@piston.default` or, with `sticky=true`, `@piston.sticky`. No `mat_slot=` chooses among them, so
one on it is `W_IGNORED_ARGUMENT`.

**What a lever or button hangs on is not checked.** It is written against whatever the wall cell
holds, and the build does not ask whether that block is one the game lets it hang on. Where no
`walls` paints that cell, or paints it with a block whose face cannot hold one, the game drops the
lever or button as soon as a block update reaches it.

**Signal names.** Sensors emit into the `sig.` namespace and actuators read from it, so a name
outside it can never be read, whether on the left of a `logic` line, in a sensor's `->` tail, or as
an actuator key's value. That is `E_LOGIC_INVALID_SIGNAL`. A name is `sig.` and exactly one segment
after it: `opened_by=a` is not a wire to a signal called `a`, and `opened_by=sig.a.b` names nothing
either.

The host is checked before the value, so `walls -> a` is one fault and it belongs to the host: the
`E_MISPLACED_BINDING` above, and not also the `E_LOGIC_INVALID_SIGNAL` the value would earn on a
sensor. No way of writing the value makes `walls` a sensor.

**Bindings go after the `[selector]`, never inside it.** `door[id=front] opened_by=sig.power` binds;
`door[id=front,opened_by=sig.power]` does not. The brackets pick the member the line acts on, so
nothing written among them is read as a binding. A bracketed pair earns the finding its key would
earn once moved out: `E_UNKNOWN_ARGUMENT` for a key outside the member's vocabulary, and
`E_LOGIC_UNKNOWN_BINDING_KEY`, below, for a `sig.` value under a key the member reads that is not
an actuator key. A component's own actuator key inside its brackets is `E_LOGIC_MISPLACED_BINDING`,
which names the brackets, whatever the value.

**A selector line binds the member its brackets pick.** `door[id=front] opened_by=sig.power` binds
the door declared as `door id=front ...` in the same scope, at the top level or directly under a
`level`, and `lamp[id=l1] lit_by=`, `piston[id=p1] powered_by=` and `dispenser[id=ds] fired_by=`
bind a lamp, piston and dispenser the same way. The `id=` has to name exactly one component of the
patch's keyword, written without brackets. The patch is a line of its own, before or after the
component it binds, at the top level or directly under a `level`, wherever that component is. A
patch whose selector has no `id=`, or whose id no component of its keyword carries or two carry, is
`E_LOGIC_UNRESOLVED_PATCH`: it acts on no component, so a port for it would have no component behind
it. An actuator patch is the only binding a selector line carries, so a `->` tail on
`pressure_plate[id=p]` or `lever[id=sw]` is `E_MISPLACED_BINDING`, as it is on a member that is not
a sensor. Nor does a sensor take a patch of any other kind: a sensor line in the selector form, such
as `lever[id=sw] at=inside.front offset=2 y=1`, places nothing, since its brackets pick a member
declared elsewhere, and it is `E_INVALID_MEMBER`.

**An actuator takes one binding per key.** A door carries one `opened_by=`, a lamp one `lit_by=`, a
piston one `powered_by=` and a dispenser one `fired_by=`, whether it is written on the component's
own line, through a patch, or on both. A second is `E_LOGIC_DUPLICATE_BINDING`: two bindings would
be two wires into one component, a wired OR the logic layer never states. Signals are combined there
instead: write `logic sig.open = sig.a or sig.x` and bind the door to `sig.open`.

A `sig.` value under a key the member reads that is not one of the four actuator keys is
`E_LOGIC_UNKNOWN_BINDING_KEY`, raised by the synth pipeline, as in `walls height=sig.power`. The
value says a signal was meant to be wired and the key is not one any component reads a signal from.
A misspelt actuator key such as `oepend_by=sig.power` is outside the member's vocabulary, so it is
the `E_UNKNOWN_ARGUMENT` that `check` reports first.

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

A macro is one node, and a `latch` or `counter` keeps the feedback it holds its state by inside its
own cell ([§14.6](#146-edition-differences)), so the graph stays acyclic and placement keeps its
topological order. A signal that reads itself back is still `E_LOGIC_CYCLE`, through a macro or not:
`logic sig.q = latch(set=sig.a, reset=sig.q)` is a cycle the author wrote.

## 14.4 Time model

In v1 only the macros carry time: `delay`, `pulse`, `edge_rising`, `edge_falling`, `latch`, and
`counter`. There is no tick operator to write. A macro's count is of stages, never of ticks. A
stage of `delay` or `pulse` is two ticks on both editions, so `delay(sig.x, 3)` lowers to repeaters
in series that take six ticks in all, and the edges are on for one stage. A stage of `counter` is
one flip-flop of its ring, so `counter(sig.x, 3)` is built of three. What a macro adds to a path is
its Edition Cell's ticks, counted as every cell's are ([§14.6](#146-edition-differences)).

| Macro | Its output |
|---|---|
| `delay(x, n)` | `x`, `n` stages later |
| `pulse(x, n)` | on while `x` is on and was off `n` stages before: for the first `n` stages of each time `x` turns on after `n` stages off, or until `x` turns off if that is sooner |
| `edge_rising(x)` | `pulse(x, 1)`: on for one stage each time `x` turns on after a stage off |
| `edge_falling(x)` | on while `x` is off and was on a stage before: for one stage each time `x` turns off after a stage on |
| `latch(set=s, reset=r)` | turns on while `s` is on and `r` is off, turns off while `r` is on, and holds while both are off |
| `counter(x, n)` | turns on the `n`-th time `x` turns on and off the `2n`-th, toggling every `n` times, so `counter(x, 1)` is a toggle |

`reset` wins: a `latch` whose `set` and `reset` are both on is off, and stays off when they turn off
together. A `latch` and a `counter` act on an operand that holds each change for at least a stage,
and what they do with one that changes back sooner is not specified; an edge or `pulse(x, 1)` is on
for a whole stage, so either can drive one. A `latch` and a `counter` start off, as every macro does
when the structure is built: the tile tier writes each cell in the state it settles to with its
operands off.

**Delay is carried in neither the Logic IR nor the Netlist IR. It is determined for the first time
in the Placement IR** ([§14.8](#148-connection-to-the-ir-and-phases)). `and` is logically
zero-delay, but the tick count is known only after cell selection (`and → ComparatorAND` on Java)
and the actual post-placement wire length. A macro's stage count is carried in both, as part of
which cell it is, and its ticks are not.

A cell takes each operand on an input port of its own: `a` and `b` on a two-input cell, `a` on
`not` and on each applied macro, and on `mux` and `latch` the operands their calls name. A cell's
delay is counted per input port, as the ticks from that port to the cell's output, since one input
can pass through more of a cell than another. A signal's delay through a cell is the delay of the
port it enters by. One change at a macro's port can change its output more than once, as a `pulse`
turns on and then off, so a macro's port counts the ticks to the last change it makes there.

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

A `struct` or `def` has one such line: a second one in the same body is `E_DUPLICATE_CIRCUIT`
([Lint §11.1](/spec/lint/#111-diagnostic-codes)). Logic that needs more than one reservation goes
in more than one scope.

The line stands among the body's own members: a reservation spans the scope's `size=WxH` and takes
no `level`'s `y=`, so place-and-route reads no `circuit` line under a `level`. Such a line is
`E_MISPLACED_MEMBER` on that line, not `E_DUPLICATE_CIRCUIT`.

The internal algorithm runs five stages:

1. **Placement.** Topological order, left to right, one clear column between each pair of cells. The
   row starts in the column after the input pads and ends at the latest in the column before the
   actuator pads, so a row of `n` cells needs `2n + 1` columns. That last column is the actuator
   pads' own: at `2n` the last cell would stand in it, face to face with the pads at `z = 0` and `z
   = 2`. Neither pad column carries a pad on the cell row, so an end cell that stands beside a pad
   column keeps the coordinate beside it there as a free neighbour. A cell body is a block, so a net
   reaches it through a neighbouring coordinate; a two-input gate has three distinct nets touching
   it (its two drivers and its own output) and therefore needs three free neighbours, and a `mux`,
   with three drivers, four. Packed against each other the cells in the middle of a row have two, at
   any region size, so a row spaced like this is what makes a short-free wiring possible at all.

   The row also stands one row in from the near edge of the region. In a scope with cells the I/O
   pads step along `z` from `0`, skipping the cell row; a scope with no cells has no cell row, and
   its pads take every row from `0`. A pad on the cell row would stand face to face with the end
   cell whenever the row reaches its column, so the terminal of a net that cell may have nothing to
   do with would take one of its faces. With the pads off that row and the row ending before the
   actuator-pad column, no pad touches a cell at any width the row check accepts. Dust reads the
   dust in the coordinate beside it, so a lane of free coordinates carries one net however long it
   is; a cell against the edge has one lane, and the three nets touching a two-input gate cannot
   share it. One row in gives every cell a lane on each side, so with the clear columns either side
   of it each cell is placed with four free neighbours, the number a `mux` needs. That costs one row
   for the whole netlist rather than one per cell, so unlike the column spacing it does not grow
   with the cell count.

   Neither spacing is a guarantee of a wiring: a net passing through can still take a face a cell
   needs. Stage 2 lays the nets again in another order when that strands a sink, and a scope no
   order wires is refused rather than shorted. Three kinds of region are refused here
   rather than left to fail as an unreachable sink at stage 2: one that cannot hold the row
   (`2n + 1` columns for `n` cells, and three rows); one that cannot hold the rows its pads stand in
   (a row per sensor or actuator on the busier edge, and, in a scope with cells, one more for the
   cell row once that edge carries two); and, in a scope with both sensors and actuators, one
   without two columns for its two pad columns, which at one column are the same column.
2. **Steiner routing.** Manhattan, around what is already standing and around the dust of the nets
   already laid. Cell bodies and I/O pads are reserved: dust cannot be drawn on one, and a signal
   cannot pass *through* one, since a component either emits or consumes. Every sink is therefore a
   leaf of its net's tree, and a fanout is a trunk beside the row with a tap into each sink rather
   than a chain through them. Where nothing is in the way the wire is a straight rectilinear run;
   otherwise it goes around, or climbs to a `bridge` layer inside the `void=<N>` budget.

   Two nets on one coordinate would be one strand of dust carrying two signals, and so would two
   nets one coordinate apart in the same plane, because dust joins the dust next to it. So the nets
   are laid one at a time, and each goes round the dust already laid *and* the coordinates beside
   it. That is the crossing escape, and it happens here rather than at stage 4 so the climb is
   measured: `wire_length` and the delay pass's tick count are both read off the routed tree. The
   order is fanout descending, then the net's own key. That is a total order, so one layout has one
   answer however many passes ask for it.

   An earlier net's dust can wall in a later net's sink: a `mux` beside another cell needs four of
   its faces, and a net on its way past takes one. So when the order strands a sink, the first net
   with a stranded sink moves to the front and every net is laid again, fewer times in all than
   there are nets. The first order that strands nothing is kept, so a scope the first order wires is
   laid as before, and each order follows from the one before it, so a layout still has one answer.
   A scope no order wires keeps the first order's trees and is refused on them.

   Beside is per-plane. Whether two strands a layer apart read each other depends on what is
   standing between them, which this model does not carry: the internal model is pseudo-2.5D and the
   voxel realisation belongs to the physical tile layer. Separating two strands within one step of
   each other across layers is that layer's obligation rather than the router's. That covers both
   the stacked pair and the diagonal one, which is a staircase and is the more numerous of the two,
   because a coordinate has one coordinate directly under it and four diagonally under it, and a run
   that climbed to clear a lane then travels alongside that lane, one step across from it, for as
   long as the two run parallel. The tile tier carries the obligation as a requirement on what a
   `bridge` coordinate may render as ([§14.6](#146-edition-differences)).

   Named, not inferred. A scope whose escape leaves two *strands* within one step of each other
   across layers earns `W_ROUTE_CROSS_LAYER_CLEARANCE`, which lists the coordinates and the nets, so
   those pairs are readable from the layout rather than derived from it. It is advisory and elides
   nothing: the layout is not at fault, and a rule the router invented here would refuse layouts for
   a reason nothing in its model can check. Nor is there a remedy to offer. Enlarging the region
   does not remove the pairs: where a net has to climb at its own doorstep, because the lanes either
   side of its cell are taken, more room only lengthens the run it then makes on the upper layer.

   ```text
   W_ROUTE_CROSS_LAYER_CLEARANCE line 45 circuit=floor:
     routed netlist for struct `crossbar` leaves 12 pairs of dust within one step of each other
     across layers (2 stacked, 10 staircase) — …
     note: (4,1,1) on cell #0 stands directly over (4,0,1) on cell #1
     note: (5,1,1) on cell #0 stands directly over (5,0,1) on cell #1
     note: (1,1,1) on cell #0 stands a layer over, and one step across from, (1,0,0) on sig.a
     note: (1,1,1) on cell #0 stands a layer over, and one step across from, (1,0,2) on sig.b
     note: and 8 more of the same two shapes
     …
     Fix: nothing in the source is wrong — the pairs are what the escape costs, and enlarging
     the region is not a remedy: …
   ```

   The segment cap is tested here too, before any wire is laid. The Manhattan distance between a
   driver and its sink is a lower bound on every route between them, so a sink already past the cap
   in a straight line is past it however the router goes, and that scope earns `E_ATTENUATION_LIMIT`
   at this stage rather than at stage 3. A floor and not the measure: it refuses strictly less than
   stage 3 does and replaces nothing. A region 256 wide puts its pad 255 blocks from the driver and
   may route 257 to get there, which is over the cap and not over this. The fix line differs from
   stage 3's for the same reason: nothing shortens a straight line, so enlarging the region is not
   the remedy here.

   The router's search is bounded by the same cap. A path from a net's wire to a sink is part of
   that sink's segment, so the search does not look past the cap. Each sink it cannot reach is then
   judged on its own. It earns `E_ROUTE_CONGESTION`, below, when the router can prove it walled in:
   none of its faces can be arrived through, or the search ran out of coordinates before the cap cut
   any off, or the free coordinates the sink opens onto all lie within the cap of it and the net's
   wire is not among them. Otherwise it earns `E_ATTENUATION_LIMIT` here, which says only that no
   route within the cap exists. A sink walled off in a free region wider than the cap is one of
   these. When a scope has sinks of both kinds, the message is about a walled-in one. The bound is
   what keeps giving up on a sink proportional to the cap rather than to the reservation: without
   it, the search visited every free coordinate the net could reach before it refused.
3. **Delay insertion.** A repeater goes in as a buffer only where a segment carries more dust than
   the attenuation limit of 15 blocks: a source at strength 15 carries 15 blocks of dust, the last
   at strength 1, which still drives the sink after it. The segment is measured along the **routed**
   path from driver to sink, and the buffer stands on that path, so the straight line between the
   two is not always wire. A segment past the v1 cap of 256 steps is refused rather than buffered,
   under the same `E_ATTENUATION_LIMIT` stage 2 raises; what differs is that this pass measures the
   wire that was actually laid. The figure the pass records per cell is a local cost, the delay of
   its slowest driven port plus the buffer repeaters on the nets feeding it, not a signal's latency.

   A repeater reads the block behind it and drives the one in front of it, at its own height, so it
   stands only where the wire runs straight through a coordinate at one height and nothing branches
   off there, nor where any block of its net other than the two it joins (dust the route reaches
   some other way, or one of the net's cells or pads) is on any of its six faces, which a repeater
   there would cut off. On a straight run from a fresh source it stands in place of every 16th
   block, with 15 blocks of dust behind it. Where that coordinate turns, climbs or forks, the
   repeater stands on the last coordinate before it that runs straight, which can leave a route one
   repeater more than its length alone implies. When the coordinate it steps back from is a fork,
   the one it lands on is on the trunk, so one block serves every branch past it; a fork further
   back than that gets a repeater on each branch. Repeaters are placed on each net's routed tree
   rather than per sink, and a sink is charged for the ones on its route. A stretch of dust past the
   limit with no coordinate a repeater can stand on is refused with `E_ATTENUATION_LIMIT`.

   The limit is counted from the last component that restores strength (a sensor pad, a repeater, a
   torch), not from the start of each segment. Two of the cells pass on the strength they receive
   instead: the Bedrock OR is a dust merge, and the Java comparator AND, a comparator with one
   operand inverted onto its side ([§14.6](#146-edition-differences)), outputs the strength its rear
   input carries while it is on. The wire into either cell on an input it passes on (both of the
   merge's, and the comparator's rear) and the wire out of it count as one run of dust, though the
   cell stands between them, measured from whichever of those inputs has spent the most. While the
   cell is on, its output is never weaker than that input: the merge passes on its strongest lit
   input and the comparator its rear. The wire into the comparator's side, port `b`, ends at the
   inverter, which restores strength, so it is held to the limit as the wire into any sink that
   restores strength is, and the dust spent on it does not count on the wire out. A net on both
   ports feeds the rear as well, so it stays in the run. The repeater the run needs goes where the
   running total would pass 15, or, on a wire of the run into such a cell, what the cell can spare
   of it, since the wire out needs the rest. Where that is a block of dust, the repeater stands in
   its place and reads the dust before it, as on a straight run. Where it is a sink, as such a cell
   is, the repeater cannot stand in its place and stands on the wire before it. It lands on the wire
   out of the cell, or on the wire into it when the wire out has no coordinate near enough to the
   cell to take one.
4. **Crossing legalization.** Assigns the coordinate of every buffer repeater stage 3 counted. The
   wire needs no legalizing by this point: a repeater stands on its own net's routed path, that
   path belongs to that net alone, and no other net runs within a step of it, so there is no
   short to lift and no coordinate to contest. It applies the straight-line cap as well, since it
   lays nets of its own and a scope can reach it from a netlist an earlier stage handed over, and
   for the same reason it refuses a stretch of dust with nowhere for a repeater as stage 3 does.
5. **Edition legalization.** See [§14.6](#146-edition-differences). This stage is not implemented
   yet.

Routing is confined to the `circuit` region. If it does not fit, the compiler fails loud. A sink
whose every way out is walled in, by a component, by an earlier net's dust or the coordinates beside
it, or by the edge of the reservation, earns the same refusal for a different reason, and the
message says which two coordinates it could not join and which nets took the faces. Its fix names
the lowest `void` that wires the scope, trying up to four layers above the scope's own, since a
layer to climb onto is what frees a face; when none of those does, it says to split the logic across
scopes instead, and names the highest `void` it tried.

```text
E_ROUTE_CONGESTION line 21 circuit=basement:
  synthesized netlist needs ~3.2x the reserved area (void=3, region 9x7).
  Fix: increase `void`, enlarge region, or split the logic across several scopes, each with its
  own `circuit` line.
```

`E_ATTENUATION_LIMIT` is the other refusal these passes raise, for four shapes. Stages 2, 3 and 4
all lay nets through one shared step, which tests the straight line between a driver and its sink
against the cap before any route is laid, and refuses a sink no route within the cap reaches once
the router has searched for one. Stage 3 also measures each routed segment against the cap. Stages 3
and 4 both refuse a stretch of dust past the limit of 15 on which no coordinate can hold a repeater:
each works out where the repeaters stand from the same routed tree, by the same rule, so the two
agree on which stretch that is and the message names the node that goes dark. Which stage refuses a
scope depends only on which one reached it first. The primary names the netlist that pass read
(`placed` at stage 2, `routed` at stage 3, `delayed` at stage 4), so the message says where in the
pipeline the shape was measured.

```text
E_ATTENUATION_LIMIT line 13 circuit=floor:
  placed netlist for struct `wide_pack` puts output pad #0 299 steps from its driver in a
  straight line — exceeds the v1 attenuation limit of 256 steps, and no route between two coords
  is shorter than the straight line between them.
  Fix: split the logic across several scopes, each with its own `circuit` line, or reserve a
  `region=` whose pad column sits within the cap of the cells it serves — a larger reservation
  cannot help, because the straight line between these two is already over the cap.
```

## 14.6 Edition differences

A three-tier cell library confines the edition difference to the library alone:

```
Logical Cell → Edition Cell → Physical Tile
  AND        → Java:    ComparatorAND → block array
             → Bedrock: TorchAND      → block array
```

The Java `ComparatorAND` is a comparator in subtract mode with one operand inverted onto its side at
full strength. While that operand is on, the side reads 0 and the comparator outputs the strength
its rear input carries; while it is off, the side reads 15 and the output is 0 whatever the rear
carries. Both operands reach the cell as the netlist carries them, so the inversion is the cell's
own, and subtract mode is the one that then computes AND: while the side reads 15, compare mode
would still pass a rear input at 15 through.

The `ComparatorAND` takes port `a` at its rear and inverts `b` onto its side. Counted per port
([§14.4](#144-time-model)), a signal entering by `a` passes the comparator alone, 1 tick, and one
entering by `b` passes the inverter and then the comparator, 2 ticks. Which operand of `and` or `or`
the netlist puts on `a` is not specified, so the order written does not decide a path's ticks
through a `ComparatorAND`; `assert latency` ([§14.7](#147-verification)) checks them, counting the
port the operand lands on.

Every logical cell has one realisation per edition. A port's ticks count the slowest path from it
to the output, or for a macro the last change it makes there ([§14.4](#144-time-model)), and a cell
either restores strength, so the wire into each of its ports ends there, or passes on what one of
its inputs carries ([§14.5](#145-place-and-route)):

| Logical cell | Java | Bedrock | Ticks per port | Strength |
|---|---|---|---|---|
| AND | `ComparatorAND` | `TorchAND`, two torches inverting an OR | Java `a` 1, `b` 2; Bedrock 2 | Java passes its rear on; Bedrock restores |
| OR | `RepeaterOR`, two repeaters onto a common line | `TorchOR`, a dust merge | Java 1; Bedrock 0 | Java restores; Bedrock passes its strongest input on |
| NOT | `InverterTorch` | `InverterTorch` | 1 | restores |
| NAND | `TorchNAND`, two inverter torches onto a common line | the same | 1 | restores |
| NOR | `TorchNOR`, one torch on a block both operands power | the same | 1 | restores |
| XOR | `TorchXOR`, the torch NORs of `not a` and `b` and of `a` and `not b`, onto a common line | the same | 2 | restores |
| MUX | `TorchMUX`, the torch NORs of `sel` and `not a` and of `not sel` and `not b`, onto a common line | the same | 2 | restores |
| DELAY `n` | `RepeaterDelay`, repeaters in series of `2n` ticks in all | the same | `2n` | restores |
| PULSE `n` | `TorchPulse`, the torch NOR of `not a` and of `a` through repeaters of `2n + 1` ticks | the same | `2n + 2` | restores |
| EDGE_RISING | `TorchEdgeRising`, a `TorchPulse` of one stage | the same | 4 | restores |
| EDGE_FALLING | `TorchEdgeFalling`, the torch NOR of `a` and of `not a` through a repeater | the same | 3 | restores |
| LATCH | `TorchLatch`, two torch NORs each powered by the other | the same | `set` 4, `reset` 5 | restores |
| COUNTER `n` | `RepeaterCounter`, `n` flip-flops of locking repeaters in a twisted ring | the same | 3 | restores |

An operand of `xor` reaches one of its NORs straight and the other through an inverter, so its
output settles once the inverted path has, two torches after the operand changes. Each port of
`mux` likewise passes an inverter and a NOR on its slowest path, `sel` on the NOR that takes it
inverted.

Nothing lays a cell out in blocks yet ([§14.8](#148-connection-to-the-ir-and-phases)), so the ticks
above are the ones each cell's Physical Tile is to take, and no simulation checks them.

A macro's cell is built of torches and repeaters alone, the same on both editions, and a `latch` or
`counter` holds its feedback inside its own cell:

- **`TorchPulse`** turns on once `not a` has turned off and off once `a` comes out of its
  repeaters, which take a tick more than its `n` stages to match the inverter, so it is on for `n`
  stages. Its port counts to that turning off: the repeaters and the NOR, `2n + 2` ticks. An
  earlier `a` still in the repeaters holds the output off, so a full pulse needs `a` off for the `n`
  stages before it turns on.
- **`TorchEdgeRising`** is a `TorchPulse` of one stage, on for two ticks, its port counting four.
  **`TorchEdgeFalling`** turns on once `a` has turned off and off once `not a` has come through its
  one-tick repeater: one stage on, three ticks to the last change.
- **`TorchLatch`**: the output torch is the NOR of `reset` and the second torch, and the second
  torch is the NOR of the output and of `set` gated by `reset`, which is the torch NOR of `not set`
  and of `reset` held on two ticks past its fall by a repeater beside it. A `set` turns the output
  on through four torches: `not set`, the gate, the second torch and the output. A `reset` turns it
  off through the output torch alone, and its fall, while `set` is on, turns it back on five ticks
  later, through the repeater and those three torches after it. The hold is what makes `reset` win:
  while `reset` is on or has just fallen, `set` reaches nothing.
- **`RepeaterCounter`**: each stage is a master–slave flip-flop of two repeaters, the master locked
  while `a` is on and the slave while it is off. Each stage feeds the next and the last feeds the
  first through an inverter, a twisted ring of `n` stages that steps through `2n` states from all
  off, so its last stage, the output, changes every `n` times `a` turns on. The slave's lock lifts
  two ticks after `a` turns on and its repeater passes the master on a tick later, three ticks in
  all, and the master locks at two ticks, before any stage it reads from can change.

The tile tier carries one place-and-route obligation of its own: a `bridge` coordinate renders as a
tile that conducts to neither **another net's** coordinate below it nor another net's coordinates
diagonally below it. Its own net's coordinate below it is the climb that put it there, since there
is no separate via, and has to conduct. Stage 2 ([§14.5](#145-place-and-route)) keeps each net's
dust off another net's dust and off the coordinates beside it in their own plane, and climbs to
clear what it cannot go round, so every strand a layer above another net is the escape's doing and
separating the two is the tile's.

It is a requirement on the catalogue rather than on the author, and it is wider than the finding
that reports it: `W_ROUTE_CROSS_LAYER_CLEARANCE` names the strand-to-strand pairs, which are the
ones two signals could come to share, while the obligation above holds over any coordinate of
another net below a `bridge` tile, a cell body or an I/O pad as much as dust. Climbing over a
component is the ordinary way past it, so a finding that named those too would report the layout
rather than what is unresolved in it.

- **Absorbed**: repeater, observer, comparator, and orientation, all cell-implementation
  differences.
- **Not absorbed**: QC (quasi-connectivity), BUD, update order. These depend on the implicit
  semantics of block-update order, for which no portable implementation exists.

Logic that requires update-order semantics is a compile error rather than a silent footgun,
consistent with "recompile, don't transcode". No pass checks for it yet, so `E_NO_PORTABLE_IMPL`
below is not raised today.

```text
E_NO_PORTABLE_IMPL line 15:
  this circuit requires update-order (quasi-connectivity / BUD) semantics.
  No portable redstone implementation exists for the target edition.
  Fix: redesign the logic to be order-independent, or drop to Tier 0 with an @edition guard.
```

Hand-placed redstone breaks across editions. With a logic description the compiler emits an
edition-correct circuit instead. That is the biggest reason to describe logic rather than parts.

## 14.7 Verification

Declare the intent, then check the synthesized circuit against it, per target edition. There are
three assertion kinds:

```
# combinational: truth table
assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->0 }

# latency — matters because place-and-route changes delay
assert latency(sig.in -> sig.out) <= 4

# temporal — bounded eventually only, not full LTL
assert always(sig.button -> eventually sig.door_open within 8)
```

The synth pipeline refuses an assertion of any kind that names a signal nothing in its scope
drives (`E_LOGIC_UNBOUND_SIGNAL`), and a truth table's rows are checked against each other
([Lint](/spec/lint/#truth-tables)).

`assert latency(<from> -> <to>) <= N` bounds the ticks a change at `<from>` takes to reach where
`<to>` is read. It takes `<=` and a whole number of ticks, and no other comparison, which is
`E_PARSE` at the comparison: place-and-route only adds ticks to a path, so an upper bound is the one
a layout can break. The ticks are counted per target edition on the legalized Placement IR
([§14.8](#148-connection-to-the-ir-and-phases)), where every buffer repeater stands. A path starts
at what drives `<from>`, a sensor's pad or a cell's output, and runs through cells to `<to>`. Each
cell it enters costs the ticks of the port it enters by ([§14.4](#144-time-model)), each buffer
repeater on the routed wire it runs along costs a tick ([§14.5](#145-place-and-route)), and dust
costs none. Where an actuator reads `<to>` by that name, the path runs on to the actuator's pad,
over the repeaters on that wire, to the slowest of them where several do; where none does, it ends
where `<to>` is driven, at the output of its cell or at its sensor's pad. The bound holds the
slowest path: where a change at `<from>` reaches one cell by two ports, the slower port counts.

A bound the slowest path exceeds is `E_SIM_ASSERTION_FAILED` on that edition: one source can meet
a bound on Bedrock, where an OR is a dust merge, and miss it on Java, where it is two repeaters. The
finding gives the ticks it measured and, in a note each, the cells the path enters, the port it
enters each by, and the repeaters on each wire. A `<to>` that does not depend on `<from>` fails on
every edition, since no path carries a change from one to the other, and synth refuses it with the
same code before an edition is chosen. Dependence is read on the Logic IR, where a gate written
twice is one gate and `logic sig.d = sig.c` gives both names one net: there, two names on one net
depend on each other, and the path between them takes no ticks. Place-and-route does not read a bound, so it is met in the
source: relaxed, or met by a path through fewer cells or along a wire with fewer repeaters.
`cairn synth --experimental-logic-synth --stage crossing` checks every bound after crossing
legalization and prints no IR when one fails.

```text
E_SIM_ASSERTION_FAILED line 13:
  struct=reach: `assert latency(sig.a -> sig.c) <= 1` fails on bedrock: its slowest path from
  sig.a to sig.c takes 2 ticks, 2 in buffer repeaters on the routed wire
  note: the path enters cell #0 (`TorchOR`) by port `a`: 0 ticks through the cell
  note: then the pad of the actuator reading sig.c: 2 buffer repeaters on the wire to it
  note: the path's wires carry 2 buffer repeaters, one wherever a wire runs past 15 blocks of
  dust, so a smaller `circuit` region, which puts the pads and the cells closer together, needs
  fewer
  note: Fix: relax the bound to `<= 2`, or shorten the path; the logic is never rewritten to
  meet a bound
```

Nothing simulates the circuit yet, so `truth` and `always` are not checked against it: a truth
table its logic contradicts still synthesizes. The self-correction loop (P5) is
**synth → sim → diff → patch**, run per target edition. The patch targets place-and-route hints,
repeaters, and buffers only. **The Logic IR is never rewritten**, because self-correction that
auto-modifies logic is dangerous. The loop does not exist yet: a bound that fails is reported, and
nothing patches the layout to meet it.

## 14.8 Connection to the IR and phases

Four IR layers sit between the Intent IR and the block-array IR, separated the way HDL separates
them:

```
Intent IR           logic declarations / circuit region / signal binding
   ↓ logic_synth
Logic IR            logical expressions, dependency DAG. Edition-neutral, no ticks
   ↓
Netlist IR          cells and nets. Logical Cell selection. Still no delay
   ↓
Edition Netlist IR  the same cells and nets. Edition Cell selection. Still no wire
   ↓ logic_place
Placement IR        cell coordinates
   ↓ logic_route
Placement IR        + actual wire length + delay + buffer repeater coordinates. Delay determined
   ↓
block-array IR      the voxel reality of dust, repeater, torch, comparator
```

The Placement IR is filled pass by pass ([§14.5](#145-place-and-route)): placement gives every cell
its coordinates, then routing adds the wire length, delay insertion the delay, and crossing
legalization the buffer repeaters' coordinates. An Edition Cell fixes the ticks from each of its
ports ([§14.6](#146-edition-differences)), but there is no wire to count before routing, so the
delay is determined in the Placement IR and no earlier. Nothing lowers the Placement IR to the
block-array IR yet: edition legalization is not implemented, `cairn compile` runs none of these
passes, and a circuit goes no further than the IRs `cairn synth --experimental-logic-synth` prints.

The phase model ([Compilation Model](/spec/compilation/)) splits the step right after `fixtures` into
`logic_synth → logic_place → logic_route`, because the I/O port coordinates are not fixed until
sensors and actuators are placed in 3D.

## 14.9 Reverse conversion

Hand-built redstone imported from a schematic ([Ecosystem Interop](/spec/ecosystem-interop/)) is kept as
Tier 0 raw in v1. Reverse-synthesizing logic from a mass of dust is out of scope, consistent with
the generation-first, lossy approach.
