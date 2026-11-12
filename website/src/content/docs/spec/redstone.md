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

- **Tier 0, physical placement.** You write `repeater at=3,1,2 facing=north delay=2` and the like,
  placing parts yourself while the compiler derives the blockstate. Behaviour is not modeled
  ([Blockstate Model](/spec/blockstate/)). The parts are [below](#tier-0-parts).
- **Tier 1, logic.** This chapter. You declare a signal graph and the compiler turns it into voxels
  through synthesis → placement → routing.

The logic layer adds three keywords, `logic`, `circuit` and `assert`, besides the sensors and
actuators it binds ([§14.2](#142-signal-binding)) and the four Tier 0 parts
([below](#tier-0-parts)). Logic primitives are not keywords: they ship as a built-in library, which
keeps the vocabulary small and closed (P3).

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

### Tier 0 parts

Four keywords place a redstone part by hand: `repeater`, `comparator`, `redstone_torch` and
`redstone_dust`. Each sets one block, whose blockstate the compiler derives as it does any member's.
What the part does is not modeled: it is in no netlist, nothing checks what it powers or what powers
it, and it carries no signal binding, so a `->` tail on one is `E_MISPLACED_BINDING`
([§14.2](#142-signal-binding)). A part stands in a `struct` or `def` body, directly or under a
`level`, and an `@edition` guard holds it as it holds a `raw_block` ([Versioning and
Editions](/spec/versioning-editions/#an-edition-guard-holds-the-escape-hatch)). In a `site` body it
is `E_MISPLACED_MEMBER`.

```
repeater       at=3,1,2 facing=east delay=2
comparator     at=4,1,2 facing=east mode=subtract
redstone_torch at=inside.back offset=2 y=2
redstone_torch at=5,1,2 facing=east
redstone_dust  at=6,1,2
redstone_dust  at=6,1,4 connect=none
```

**Where a part stands.** `at=x,y,z` names its cell as a `raw_block`'s `at=` does ([Materials and
Themes §7.4](/spec/materials-themes/#74-raw-blocks)), and the part is refused or deferred where a
`raw_block` would be. `at=<side>.outside` or `at=inside.<side>`, with `offset=` and `y=`, sets it by
a wall as a `pressure_plate` is set ([§14.2](#142-signal-binding)). An `offset=` or `y=` beside a
triple is `E_INVALID_MEMBER`, since the triple names the cell whole.

**Which way it looks.** A `repeater` or a `comparator` reads the cell behind it and drives the cell
in front of it, and its `facing=` is the way it drives. By a wall it takes `in`, `out`, `left` or
`right`, as an `observer` does, and drives away from the wall when its line writes none. At a triple
it takes `north`, `south`, `east` or `west`, north toward the `back` wall and east toward the
`right` one, and its line must write one, since no wall stands by it to drive away from. A
`redstone_torch` by a wall hangs on the wall's face, pointing away from it. At a triple it stands on
the block under it, or, with a `facing=` of the four compass words, hangs on the side of the block
behind it, pointing that way: that is the torch to wall-torch substitution of [Blockstate Model
§6.1](/spec/blockstate/#61-derive-by-default-promote-on-override), read off what the torch is
attached to, and a torch by a wall has nothing for a `facing=` to choose. A `redstone_dust` looks
no way, so `facing=` is not among its keys. Any word a part's anchor does not take there, a compass
word by a wall, a word from a wall at a triple or a `facing=` on a torch by a wall among them, is
`E_INVALID_MEMBER`, and the part is not placed.

**Its states.** `delay=` sets a repeater's delay, a whole number from 1 to 4, and 1 when the line
writes none. `mode=compare` or `mode=subtract` sets a comparator's mode, `compare` when the line
writes none. A dust's four sides are derived once every member is painted ([Blockstate Model,
Connections](/spec/blockstate/#connections)), unless `connect=none` builds it as a dot, joining no
side, or `connect=all` as a cross, joining all four. Each part is built as it stands with nothing
powering it: a repeater neither powered nor locked, a comparator unpowered, a torch lit and dust at
power 0, and the game settles each once a block update reaches it. A `facing=`, `delay=`, `mode=`
or `connect=` whose value is not the kind its key takes, a number where a word goes or a word where
a number goes, is `W_IGNORED_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)), and the
part is built as if it were not written. A value of the right kind that names nothing listed here,
such as `delay=5`, is `E_INVALID_MEMBER`.

**What it is built from.** Each part has one block to be built from, its tier-1 token ([Materials
and Themes](/spec/materials-themes/#tier-1-tokens)), and a `mat_slot=` on one is
`W_IGNORED_ARGUMENT`. What a part stands or hangs on is not checked, as a lever's is not: a part
with no block under it, or a torch with none behind it, drops as soon as a block update reaches it.

**A part and the circuit.** A Tier 0 part is outside the netlist, so nothing reads what it does.
The two never share a cell: a `circuit` reservation lies under the scope's base plane
([§14.5](#145-place-and-route)), and every member, a Tier 0 part among them, stands at or above it.
Since no table says what a part powers, the circuit runs no dust under one or under a cell beside
it, and refuses an actuator whose riser would power one.

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
either. On an actuator patch the block-array pass reads the value first, so a name outside `sig.`
there is the patch's `E_INVALID_MEMBER` ([Lint](/spec/lint/#lowering)), and only `cairn synth`,
which builds no block array, reports it as `E_LOGIC_INVALID_SIGNAL`.

The host is checked before the value, so `walls -> a` is one fault and it belongs to the host: the
`E_MISPLACED_BINDING` above, and not also the `E_LOGIC_INVALID_SIGNAL` the value would earn on a
sensor. No way of writing the value makes `walls` a sensor.

**Bindings go after the `[selector]`, never inside it.** `door[id=front] opened_by=sig.power` binds;
`door[id=front,opened_by=sig.power]` does not. The brackets pick the member the line acts on, so
nothing written among them is read as a binding. The brackets of an actuator patch take only
`id=<label>`, so any other pair among them, a component's own actuator key included, is
`E_INVALID_MEMBER` on the patch, and the message names each such pair
([Lint](/spec/lint/#lowering)). `cairn synth`, which builds no block array, reports an actuator key
there as `E_LOGIC_MISPLACED_BINDING`, which names the brackets, and a `sig.` value under another key
the member reads as `E_LOGIC_UNKNOWN_BINDING_KEY`.

**A selector line binds the member its brackets pick.** `door[id=front] opened_by=sig.power` binds
the door declared as `door id=front ...` in the same scope, at the top level or directly under a
`level`, and `lamp[id=l1] lit_by=`, `piston[id=p1] powered_by=` and `dispenser[id=ds] fired_by=`
bind a lamp, piston and dispenser the same way. The `id=` has to name exactly one component of the
patch's keyword, written without brackets. The patch is a line of its own, before or after the
component it binds, at the top level or directly under a `level`, wherever that component is. A
patch whose selector has no `id=`, or whose id no component of its keyword carries or two carry,
acts on no component, so the block-array pass refuses it as `E_INVALID_MEMBER`, and the message says
which. `cairn synth`, which builds no block array, reports it as `E_LOGIC_UNRESOLVED_PATCH`, since a
port for it would have no component behind it. An actuator patch is the only binding a selector line
carries, so a `->` tail on `pressure_plate[id=p]` or `lever[id=sw]` is `E_MISPLACED_BINDING`, as it
is on a member that is not a sensor. Nor does a sensor take a patch of any other kind: a sensor line
in the selector form, such as `lever[id=sw] at=inside.front offset=2 y=1`, places nothing, since its
brackets pick a member declared elsewhere, and it is `E_INVALID_MEMBER`.

**An actuator takes one binding per key.** A door carries one `opened_by=`, a lamp one `lit_by=`, a
piston one `powered_by=` and a dispenser one `fired_by=`, whether it is written on the component's
own line, through a patch, or on both. A second is `E_LOGIC_DUPLICATE_BINDING`: two bindings would
be two wires into one component, a wired OR the logic layer never states. Signals are combined there
instead: write `logic sig.open = sig.a or sig.x` and bind the door to `sig.open`.

A `sig.` value under a key the member reads that is not one of the four actuator keys is
`E_LOGIC_UNKNOWN_BINDING_KEY`, raised by the synth pipeline, as in `door hinge=sig.power`. The
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
together. Every macro acts on an operand that holds each change for at least a stage, and what one
does with an operand that changes back sooner is not specified; an edge and `pulse(x, 1)` are on for
a whole stage, so either can drive any macro. A `latch` and a `counter` start off, as every macro
does when the structure is built: the tile tier writes each cell in the state it settles to with its
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

Place-and-route builds each scope's netlist as a circuit at full size, one block for each part, in
layers the scope reserves under its floor. The struct rises over them.

```
circuit region=floor void=3       # reserve 3 layers under the floor and lay the circuit in them
```

A `struct` or `def` has one such line: a second one in the same body is `E_DUPLICATE_CIRCUIT`
([Lint §11.1](/spec/lint/#111-diagnostic-codes)). Logic that needs more than one reservation goes
in more than one scope.

The line stands among the body's own members: a reservation lies under the scope's base plane,
where no member stands, and takes no `level`'s `y=`, so place-and-route reads no `circuit` line
under a `level`. Such a line is `E_MISPLACED_MEMBER` on that line, not `E_DUPLICATE_CIRCUIT`.

A scope needs the line once its logic has a cell to build or an actuator to drive. A scope without
one is `E_NO_CIRCUIT_REGION` at its first cell, or at its first actuator binding when it has no
cell. A scope with no `size=WxH` has no footprint to reserve under, so its line is
`E_NO_CIRCUIT_REGION` on that line. A line with no `region=` label, or with a `void=` that is not a
whole number of at least 1, reserves nothing and is `E_INVALID_MEMBER` on that line
([Lint](/spec/lint/#lowering)). Such logic in a `site` is `E_NO_CIRCUIT_REGION` too, since a `site`
has no base plane to reserve under.

**The reservation.** `void=N` reserves `N` layers under the base plane, across the whole footprint
the struct is built on, the ring a roof's `overhang=` adds included. The built struct is `N`
layers taller, every member standing over the reservation as it stood over the ground, and a `def`
placed in a `site` has its origin lowered by `N`, so its base plane stays where the `site` sets it.
A `void=` that takes the struct past the most blocks one structure holds, the volume
`W_STRUCTURE_TOO_LARGE` guards ([Lint](/spec/lint/#lowering)), is `E_ROUTE_CONGESTION` on the
`circuit` line.

The circuit lies in the top `min(N, 5)` of those layers, and any under them stay empty:

- the **bottom** layer, laid whole with the circuit block, which everything over it stands on;
- the **cells'** layer, which every cell's tile stands in, a `counter`'s rising one layer over it;
- up to **two** layers over that, where one signal's dust climbs over another's;
- the **top** layer, directly under the floor, where the circuit meets the struct.

So a reservation needs three layers, and four when its logic holds a `counter`, whose feedback
crosses its own wire a layer up. A `void=` of fewer is `E_ROUTE_CONGESTION` on the `circuit` line,
and the message names the count. The circuit block is `@circuit.block` ([Materials and
Themes](/spec/materials-themes/#tier-1-tokens)), and it also fills every voxel of the top layer
the circuit leaves empty under a block of the floor, except where a block would take a torch's
power from under it or cut a climb of dust past it, which stays air.

**Where the circuit meets the struct.** Each sensor whose signal something reads enters the
circuit by a pad in the top layer under it, and each actuator a binding drives is driven by a
riser: two torches in series that carry the signal up from the cells' layer to the top layer, the
upper one under the actuator. Neither reaches further than the floor row, so a component is met
only where it stands in the floor row `y = 0`, or at `y = 1` over a block of the floor that passes
power between it and the top layer:

| Component | Where it stands | What meets it |
|---|---|---|
| `pressure_plate` | `y = 0`, or `y = 1` on a block | at `y = 0`, a block under it, which it powers, and dust under that; at `y = 1`, dust under the block it stands on, which it powers whole |
| `lever`, `button` | `y = 0` | dust under it |
| `daylight` | `y = 0` | dust under it, then a repeater, since its strength follows the sky |
| `observer` | looking up, `facing=up`, at `y = 0`, or at `y = 1` over a block | dust under its back, or under the block its back powers whole, then a two-tick repeater that holds its one-tick pulse for a stage |
| `door` | `y = 1`, over a block | the riser's upper torch, under that block, which it powers |
| `lamp`, `piston`, `dispenser` | `y = 0`, or `y = 1` over a block | the riser's upper torch, under it or under the block it stands on |

A component anywhere else is `E_ROUTE_UNREACHABLE_COMPONENT` at its line: the message says where
that kind of component reaches the layer under the floor, and the fix moves it there. So is a
`piston` looking down, which would push into the torch under it, and an actuator whose riser would
drive another component as well. The cell the riser's torch powers, the actuator itself in the
floor row or the block under it at `y = 1`, powers whatever stands beside it in the floor row or
over it, so no other actuator, Tier 0 part or raw member may stand there. A sensor may, since power
moves no sensor.

```text
room.crn:8:3: error[E_ROUTE_UNREACHABLE_COMPONENT]: struct=room: the lever `l` at y=2 cannot reach the circuit under the floor: a lever or button reaches the layer under the floor only from the floor row `y = 0`, the row over it
  note: Fix: set `y=0` on it
```

The top layer stays clear of what the struct powers, since dust under a powered block reads that
power. Dust runs there only under a **quiet** cell of the floor row: air or a block no component
holds, with no sensor, Tier 0 part or raw member beside it or over it. A pad's own dust may also run
under a cell its own sensor is the only one to power. A riser's upper torch hangs on a block its
lower torch powers, and that block stands only under a cell no component holds.

The internal algorithm runs four stages for each edition, each on the layout the stage before it
left. Every choice in them is ordered, so one source lays one circuit.

1. **Placement.** The pads and risers are laid first, since each stands under its component. Each
   is turned the way that fits beside the others with the end the circuit joins nearest the
   middle of the reservation, and a component whose pad or riser fits no way is `E_ROUTE_CONGESTION` at its
   line. Each cell's tile then follows, in the netlist's order, at the free spot nearest the tiles
   its nets join: the spot whose ports and outputs lie least far, in all, from those of the tiles
   already laid that share a net with it, then the least `z`, the least `x`, and the turns in the
   order east, south, west, north. A spot is free when the tile stays inside the reservation and
   under its top layer, keeps a margin of clear columns from every other tile, and leaves every port
   and output laid so far a side a wire can still reach it from. The cells are laid at margins of 2,
   1 and 0 columns, and the layout whose cells lie nearest, in all, the tiles they share nets with
   is kept. A cell with no free spot at any margin is `E_ROUTE_CONGESTION` at its `logic` line, and
   the message names the columns its tile needs.

   A `pulse` of more than 6 stages or a `counter` of more than 4 is `E_ATTENUATION_LIMIT` at its
   `logic` line before anything is laid. The one's operand runs a block of dust past each of its
   repeaters, and the other's past every stage, on one strand that goes dark past 15 blocks; the
   message offers `x and not delay(x, n)` for a longer pulse. A `delay` lays its repeaters in one
   row, two for each stage, so one whose row with its port and output is longer than both sides of
   the struct is `E_ROUTE_CONGESTION` at its `logic` line before its cells are laid.
2. **Routing.** Each net's dust runs from the tile that drives it to every input it feeds, along a
   layer and up and down staircases. It takes the cheapest path from the net's dust so far to the
   nearest input it has not reached, a step along a layer costing 1 and a step up or down 2. A step
   up keeps the cell over the lower dust empty, and a step down the cell beside the upper one, so
   the two link, and dust with nothing under it gets a block. Dust runs only over the bottom layer,
   and never where it would link to another net's dust or run beside a tile's own. The nets are
   routed most inputs first, then in the netlist's order. A net that finds no way is moved to the
   front and the nets routed again, up to three times, and then the cells are laid again at the
   other margins. A net that finds no way through any of them is `E_ROUTE_CONGESTION` at the line
   that drives it, as the example `crossbar.crn` is with `void=3`:

   ```text
   crossbar.crn:25:3: error[E_ROUTE_CONGESTION]: struct=crossbar: the sensor's signal finds no way to every input it feeds through the reservation (region 14x12, void=3) without running beside another signal's dust
     note: Fix: widen the struct's `size=WxH`, increase `void`, or split the logic across several scopes, each with its own `circuit` line
   ```
3. **Delay insertion.** A signal leaves a part that restores it, a pad, a torch, a repeater or a
   cell that restores strength ([§14.6](#146-edition-differences)), at strength 15, and loses one
   for each block of dust. The Java `ComparatorAND` restores nothing: it passes on the strength its
   rear input reads. Each input reads what its net's dust carries to it from each part that drives
   the net on its own, and needs at least 1, or more where its tile's own dust carries the signal on
   past the port. Where an input would read less than it needs, a buffer repeater replaces a block
   of the net's own dust, at the spot nearest the input, on the way back to the part that last
   restored the signal, where a repeater fits: where the dust runs straight and level through three
   blocks, joins nothing else, still carries power into the repeater, and is not driven from the
   side. Repeaters go in one at a time until every input reads what it needs. An input that no such
   spot lifts far enough is `E_ATTENUATION_LIMIT` at the line of the tile that reads it, and the
   message says the dust runs past 15 blocks with no straight stretch for a repeater.
4. **Legalization.** Every part becomes the block it is built as, its id the target's for the part's
   token: `@redstone_dust.default` for dust, `@redstone_torch.wall` for a torch, `@repeater.default`
   for a repeater, `@comparator.default` for a comparator, and `@circuit.block` for every other
   block. Each is written in the state the circuit rests in with every sensor off, which this stage
   finds by running the circuit until nothing changes: dust with its power and the sides it joins, a
   torch lit or not, a repeater powered and locked or not, and a comparator, which only a Java
   circuit lays ([§14.6](#146-edition-differences)), with the strength it outputs, which the
   structure keeps in its block entity. A `latch`, a `counter` and every other macro start off
   ([§14.4](#144-time-model)). Bedrock keeps an unlit torch and a powered repeater or comparator in
   blocks of their own, such as `unlit_redstone_torch` and `powered_repeater`, and its build writes
   those. A target whose pack declares no block for one of these tokens is
   `E_UNKNOWN_ABSTRACT_TOKEN` on the `circuit` line, and the message names each it lacks.

Routing is confined to the `circuit` reservation. A circuit that does not fit it is refused, never
laid past it.

A stage that finds the circuit laid wrong by one before it refuses it with `E_ROUTE_CONGESTION` on
the `circuit` line: a part where none fits, which the message places, a block outside the
reservation, a circuit that never rests, or a latency bound whose path the circuit no longer
carries ([§14.7](#147-verification)). No layout the stages before accept should do any of these, so
the message asks for the struct as a bug report.

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
| OR | `RepeaterOR`, two repeaters onto a common line | the same | 1 | restores |
| NOT | `InverterTorch` | `InverterTorch` | 1 | restores |
| NAND | `TorchNAND`, two inverter torches onto a common line | the same | 1 | restores |
| NOR | `TorchNOR`, one torch on a block both operands power | the same | 1 | restores |
| XOR | `TorchXOR`, the torch NORs of `a` and of `b`, each with the NOR of the two, onto a common line | the same | 2 | restores |
| MUX | `TorchMUX`, the torch NORs of `sel` and `not a` and of `not sel` and `not b`, onto a common line | the same | 2 | restores |
| DELAY `n` | `RepeaterDelay`, `2n` one-tick repeaters in series | the same | `2n` | restores |
| PULSE `n` | `TorchPulse`, the torch NOR of `not a` and of `a` through `2n + 1` one-tick repeaters | the same | `2n + 2` | restores |
| EDGE_RISING | `TorchEdgeRising`, a `TorchPulse` of one stage | the same | 4 | restores |
| EDGE_FALLING | `TorchEdgeFalling`, the torch NOR of `a` and of `not a` through a repeater | the same | 3 | restores |
| LATCH | `TorchLatch`, two torch NORs each powered by the other | the same | `set` 4, `reset` 5 | restores |
| COUNTER `n` | `RepeaterCounter`, `n` flip-flops of locking repeaters in a twisted ring | the same | 3 | restores |

An operand of `xor` reaches its own NOR straight and the other through the NOR of the two, so its
output settles once that path has, two torches after the operand changes. Each port of
`mux` likewise passes an inverter and a NOR on its slowest path, `sel` on the NOR that takes it
inverted.

A macro's cell is built of torches and repeaters alone, the same on both editions, and a `latch` or
`counter` holds its feedback inside its own cell:

- **`RepeaterDelay`**'s repeaters, and **`TorchPulse`**'s, take a tick each. A repeater stretches a
  change shorter than its delay to its delay, and a change as long as its delay ends on the tick
  the repeater would follow it, where the update order decides, so only one-tick repeaters pass
  every change held for a stage as it came.

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

Each Edition Cell's Physical Tile is its parts at full size: a block, torch, repeater, comparator or
dust in each voxel, the voxel each port reads, and the dust its output leaves by, which placement
lays turned a quarter at a time ([§14.5](#145-place-and-route)). A port reads its operand as a block
it powers, a repeater it drives, or dust, and dust that carries the operand on inside the tile needs
the strength to reach its end, so a port says how strong the wire into it must arrive.

- **Absorbed**: repeater, observer, comparator, and orientation, all cell-implementation
  differences.
- **Not absorbed**: QC (quasi-connectivity), BUD, update order. These depend on the implicit
  semantics of block-update order, for which no portable implementation exists.

Logic that requires update-order semantics is a compile error, `E_NO_PORTABLE_IMPL`, rather than
a silent footgun, consistent with "recompile, don't transcode". Every cell of the library, the
macros' included, holds its own timing, so a circuit comes to need update order only where one
component of the built structure reads another through a block update. Two shapes do, and each is
checked against the structure as built for the edition:

- **An observer reading the circuit back (BUD), on every edition.** An `observer` with a `->` tail
  whose front watches a `lamp`, `piston`, `dispenser` or `door` that a binding drives, or the cell a
  driven piston's head moves into. Its signal is the circuit's own change, arriving when the block
  update reaches it. The finding is at the observer, with a note at the binding, and its fix names
  the order-free form of the same pulse, `edge_rising(sig.x) or edge_falling(sig.x)`.
- **A piston or dispenser powered through the cell above it (QC), on Java.** A driven `piston` or
  `dispenser` that a sensor powers through the cell above it and not directly, so it acts on that
  power only once a block update reaches it. A sensor powers the cell above when it gives off a
  signal into that cell, a `lever`, `button`, `pressure_plate` or `daylight` into every cell beside
  it and an `observer` into the cell behind it, or when it powers whole a block beside that cell: a
  `lever` or `button` the block it hangs on, a `pressure_plate` the block under it, and an
  `observer` the block behind it. Any block but air, a sensor, a `piston` and a `door` is taken to
  pass that power on, a `lamp` and a `dispenser` among them, since no table records which blocks
  conduct. A piston takes no power through its front. The finding is at the actuator, with a note at
  the sensor. Bedrock has no quasi-connectivity, so this is no finding there.

A [Tier 0 part](#tier-0-parts) and a raw member are the author's, so neither is checked, and a
sensor that also powers the component directly is no dependence on order and is not reported.
Every command that builds a circuit runs the check before place-and-route
([§14.8](#148-connection-to-the-ir-and-phases)). Here the lever two rows above the dispenser stands
beside the cell above it:

```
theme t:
  slot wall -> @cobblestone

struct s size=7x5
  floor mat_slot=wall
  walls mat_slot=wall height=4
  roof kind=flat mat_slot=wall overhang=1
  dispenser id=p at=inside.back offset=2 y=1 fired_by=sig.p
  lever id=sw at=inside.back offset=2 y=3 -> sig.a
  logic sig.p = not sig.a
  circuit region=floor void=4
```

```text
dispenser.crn:8:3: error[E_NO_PORTABLE_IMPL]: struct=s: on java, the dispenser `p` also takes power from the lever `sw` through the cell above it, and acts on it only once a block update reaches it: this circuit requires update-order (quasi-connectivity) semantics
dispenser.crn:9:3:   note: the lever `sw` powers the cell above the dispenser `p` from here
  note: bedrock has no quasi-connectivity, so this is no dependence on order there
  note: Fix: move the lever `sw` so that neither it nor a block it powers whole stands beside the cell above the dispenser `p`, or redesign the logic to be order-independent; or build it by hand as raw members under an `@edition java` guard
```

On Bedrock the same source stops at place-and-route instead, since a lever three rows over the
floor row cannot reach the circuit under it (`E_ROUTE_UNREACHABLE_COMPONENT`,
[§14.5](#145-place-and-route)).

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
costs none. Where an actuator reads `<to>` by that name, the path runs on over the repeaters on the
wire to the actuator's riser and up its two torches, two ticks more, to the slowest of them where
several do; where none does, it ends where `<to>` is driven, at the output of its cell or at its
sensor's pad. The bound holds the slowest path: where a change at `<from>` reaches one cell by two
ports, the slower port counts.

A bound the slowest path exceeds is `E_SIM_ASSERTION_FAILED` on that edition: one source can meet a
bound on Java, where an `and` entered by `a` passes a comparator alone, and miss it on Bedrock,
where it passes two torches. The finding gives the ticks it measured and, in a note each, the cells
the path enters, the port it enters each by, and the repeaters on each wire. A `<to>` that does not
depend on `<from>` fails on every edition, since no path carries a change from one to the other, and
synth refuses it with the same code before an edition is chosen. Dependence is read on the Logic IR,
where a gate written twice is one gate and `logic sig.d = sig.c` gives both names one net: there,
two names on one net depend on each other, and the path between them takes no ticks. Place-and-route
does not read a bound, so it is met in the source: relaxed, or met by a path through fewer cells or
along a wire with fewer repeaters. Every command that builds the circuit checks each bound once it
is legalized, and refuses to build it when one fails
([§14.8](#148-connection-to-the-ir-and-phases)).

```text
reach.crn:13:3: error[E_SIM_ASSERTION_FAILED]: struct=reach: `assert latency(sig.a -> sig.c) <= 1` fails on java: its slowest path from sig.a to sig.c takes 4 ticks, 1 in buffer repeaters on the routed wire and 2 in the torches up to the actuator
reach.crn:10:3:   note: the path enters cell #0 (`RepeaterOR`) by port `a`: 1 buffer repeater on the wire, then 1 tick through the cell
reach.crn:7:3:   note: then the actuator reading sig.c: no buffer repeater on the wire to the torches under it, then 2 ticks up to it
  note: the path's wires carry 1 buffer repeater, one wherever a wire runs past 15 blocks of dust, so laying what reads a signal nearer what drives it needs fewer
  note: Fix: relax the bound to `<= 4`, or shorten the path; the logic is never rewritten to meet a bound
```

`assert truth` and `assert always` are checked by running the legalized circuit of each target
edition a tick at a time, each part taking the ticks [§14.4](#144-time-model) gives it. A run drives
the circuit only through its sensors. It starts from the state the circuit rests in with every
sensor off, switches on some of the sensors the assertion's signals depend on at tick 0, and holds
them, the others staying off; an `observer` is switched on for that one tick, the pulse it gives. A
check makes one run for each combination of those sensors, so `k` of them take `2^k` runs, counting
up with the sensors in source order, the first the highest bit. A run lasts until the circuit comes
to rest, at most twice the ticks of all the circuit's parts added together and one more: the logic
is a DAG ([§14.3](#143-the-logic-layer-is-a-dependency-dag)), so no change outlasts a pass through
every part, even with each repeater holding a pulse for its delay. A signal is read where what
drives it leaves it: a cell at its output, and a sensor at its pad, or as the run switches it where
nothing in the circuit reads it and so it has no pad. The consequent of an `always` is read where
`assert latency` ends a path instead: at the upper torch of each riser under an actuator that reads
it by that name, or, where none does, where it is driven.

- A **truth table** holds when, in each run, once the circuit rests, the output reads what every row
  whose pattern matches the inputs says. A row no run reaches, its pattern matching none of the
  combinations the inputs rest in, is not checked. Every combination is reached where each input is
  the signal of a sensor of its own other than an `observer`, which is off again by the time the
  circuit rests.
- **`always(<a> -> eventually <b> within N)`** holds when, in each run, every tick on which `<a>`
  reads on is followed, on that tick or within `N` after it, by one on which `<b>` reads on at each
  point it is read: where several actuators read `<b>`, the one it reaches last counts, as the
  slowest riser counts for a latency bound. Once the circuit rests nothing changes, so its last tick
  stands for every tick after it.

An assertion a run breaks is `E_SIM_ASSERTION_FAILED` on that edition, and the finding says which
sensors the run switched on and what it read. For a truth table, that is the first run to break it,
and the inputs and the output once the circuit rests, with a note at the row they contradict; the
circuit computes the logic as written, so the fix is the row or the logic. For an `always`, it is
the run in which `<b>` follows `<a>` latest, or the first in which it never does: the tick `<a>`
read on and the first tick after it on which `<b>` did, with a note at the actuator `<b>` reaches
last. The fix relaxes `within` to the ticks measured, which then holds in every run, or shortens the
path, as for a latency bound.

```text
door.crn:33:3: error[E_SIM_ASSERTION_FAILED]: struct=gatehouse: `assert always(sig.step -> eventually sig.open within 2)` fails on java: with sig.step switched on, sig.step reads on at tick 0 and sig.open not until tick 3, 3 ticks later
door.crn:18:3:   note: sig.open is read at the torches under this actuator
  note: Fix: relax the bound to `within 3`, or shorten the path; the logic is never rewritten to meet an assertion
```

An assertion no run judges is not checked, and is the warning `W_SIM_ASSERTION_UNCHECKED` on its
line. That is one in a scope that lays no circuit, its logic having no cell to build and no actuator
to drive; one whose signals depend on more than 10 sensors, since it would take more than 1024 runs;
and one no run breaks while a run is still changing when its ticks run out, which is a fault in the
circuit the compiler built. A truth table no run breaks with rows no run reaches is the same
warning, with a note at each of those rows. A `def` no `place` uses is built nowhere, so none of its
assertions is checked, and the def is `W_UNUSED_DEF` ([Lint](/spec/lint/#sites-and-placements)).
Every command that builds the circuit runs these checks once the latency bounds hold, and refuses to
build a circuit an assertion fails on ([§14.8](#148-connection-to-the-ir-and-phases)).

The self-correction loop (P5) is **synth → sim → diff → patch**, run per target edition. The patch
targets place-and-route hints, repeaters, and buffers only. **The Logic IR is never rewritten**,
because self-correction that auto-modifies logic is dangerous. The loop does not exist yet: an
assertion that fails is reported, and nothing patches the layout to meet it.

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
Placement IR        every tile at its voxels: cells, pads under sensors, risers under actuators
   ↓ logic_route
Placement IR        + each net's dust and buffer repeaters, then every voxel's block. Delay determined
   ↓
block-array IR      the struct, risen over the circuit laid under it
```

The Placement IR is filled pass by pass ([§14.5](#145-place-and-route)): placement lays every tile,
then routing adds each net's dust, delay insertion its buffer repeaters, and legalization the block
and state of every voxel. An Edition Cell fixes the ticks from each of its ports
([§14.6](#146-edition-differences)), but there is no wire to count before routing, so the delay is
determined in the Placement IR and no earlier, and every assertion is checked on the legalized
circuit ([§14.7](#147-verification)).

The passes run once the struct is lowered, since a pad or riser stands under a component the
lowering placed: synthesis, the order check of [§14.6](#146-edition-differences) on the struct as
built, the four stages of place-and-route, the latency check, and the runs that check `truth` and
`always`. The first that reports an error stops the rest, and the struct is left as it was.
Otherwise each legalized circuit is laid under its struct, which rises `void=` layers over it, and
a comparator's output strength goes into its block entity.

`cairn compile` builds every circuit for the edition it compiles, and an error the passes report
refuses the compile as an error of the lowering does. `cairn lower --edition` prints the struct with
its circuit laid, and `cairn lower` without one lays none, since every pass after synthesis is per
edition. `cairn check` and `cairn info` run the passes for each edition they read, and pinned to one
they report its findings. Otherwise they report the ones every edition raises at the same place,
once for each wording the editions give it, as an `E_SIM_ASSERTION_FAILED` that fails "on java" and
"on bedrock" is reported twice. `check` also reports an edition's own finding, whose span or one of
whose notes' spans is on a line under that edition's guard
([Lint](/spec/lint/#113-error-vs-warning)), and `info` reports each edition's own under that
edition. `cairn synth --experimental-logic-synth` prints each IR on the way.

The phase model ([Compilation Model](/spec/compilation/)) splits the step right after `fixtures` into
`logic_synth → logic_place → logic_route`, because the I/O port coordinates are not fixed until
sensors and actuators are placed in 3D.

## 14.9 Reverse conversion

Hand-built redstone imported from a schematic ([Ecosystem Interop](/spec/ecosystem-interop/)) is kept as
Tier 0 raw in v1. Reverse-synthesizing logic from a mass of dust is out of scope, consistent with
the generation-first, lossy approach.
