---
title: "Tutorial"
description: Build up from a single cottage to a themed tower, a redstone gatehouse, and a village.
---

This walks through the [examples](/examples/), one idea at a time. Read it top to bottom; each
section assumes the one before it.

> The reference compiler is still a skeleton, so the `cairn compile` commands below do not run yet.
> They match the spec exactly, which makes them the right way to build intuition for the CLI.

## 1. A cottage

Here is a complete Cairn build —
[`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn).

```
@cairn 2026.06
@requires version>=1.20

theme medieval:
  slot floor -> @oak_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  window[class=small] -> frame=@spruce_wood

struct cottage size=9x7
  floor  mat_slot=floor
  walls  class=outer mat_slot=wall height=4
  door   side=front at=center
  window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass
  roof   kind=gable mat_slot=roof overhang=1
```

Compile it for either edition:

```sh
cairn compile examples/cottage.crn --edition java    --target 1.21.4
cairn compile examples/cottage.crn --edition bedrock --target 1.21.40
```

**The Minecraft version is never in the source.** It lives in `--target`, and `--edition` is
required alongside it. What the source *may* declare is `@requires`, a floor on what the target has
to support, and `@cairn`, the language version the file was written against.

**The struct says where; the theme says what.** `mat_slot=wall` is an injection point, not a block
name. The theme binds it. Swap `theme medieval` for another and the geometry never changes — this is
the CSS idea, applied to blocks.

**Positions are semantic.** `side=front`, `offset=2`, `y=2`, `at=center` are positions along a wall.
No absolute coordinates appear anywhere you write.

**Order does not matter.** The `window` is written after the `roof` and is still cut as an opening
in the wall. The compiler sorts commands into fixed phases before evaluating them, so you can write
lines in whatever order reads best.

**Blockstate is derived.** Nobody writes `facing=south` for the door, `north=tall` for the wall, or
the pane's `connected` state. The compiler works them out from position and neighbours.

## 2. Abstract materials and levels

[`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) adds
three ideas to the same shape.

```
theme keep_dark:
  slot floor -> @floor.wood.broadleaf   # abstract token
  slot wall  -> @wall.stone.cobble
  slot trim  -> @wood.dark
  slot roof  -> @roof.dark_wood

struct keep size=11x9
  floor  id=base   mat_slot=floor
  walls  id=shell  mat_slot=wall height=5
  roof   id=roof   kind=gable mat_slot=roof overhang=1

  level id=floor1 y=0
    ...

  level id=floor2 y=5
    walls  id=upper class=outer mat_slot=wall height=4
    window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2 shape=slit
    stair  id=eave kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left
```

**Two kinds of token.** `@oak_planks` is a *canonical block token* — a specific meaning that can
never be silently downgraded. `@floor.wood.broadleaf` is an *abstract material token* — an aesthetic
choice a theme may resolve to oak or birch depending on the target.

**`level y=5` gives the upper floor its own `y=0`.** The second-floor window stays at `y=2` from its
own floor rather than from the ground.

**Writing a blockstate promotes it to intent.** The `stair id=eave` line writes `half=top facing=out
shape=outer_left` explicitly, so those values are now yours, not the compiler's. The rule is "derive
by default; any blockstate that *can* be intent is overridable".

Read on: [Materials and Themes](/spec/materials-themes/),
[Blockstate Model](/spec/blockstate/).

## 3. Redstone as a signal graph

Instead of placing dust and repeaters, you declare what depends on what.
[`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn):

```
struct gatehouse size=7x5
  floor mat_slot=wall
  walls class=outer mat_slot=wall height=3
  door  id=front side=front at=center mat_slot=door

  pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step
  pressure_plate id=inner at=inside.front  offset=0 y=0 -> sig.exit

  logic sig.open = sig.step or sig.exit
  door[id=front] opened_by=sig.open

  circuit region=floor void=2

  assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }
  assert always(sig.step -> eventually sig.open within 2)
```

**Sensors emit, actuators consume.** `-> sig.step` is a sensor's output; `opened_by=sig.open` is an
actuator's input. `logic` writes the dependency between them.

**There is no tick arithmetic.** The logic expression carries no time at all. The `within 2` in the
assertion is the only place a number means ticks, because delay is not known until the circuit has
been placed and routed.

**`circuit region=…` reserves space** for place-and-route. If routing does not fit you get
`E_ROUTE_CONGESTION` with a suggested fix — never a silent overflow.

**Assertions come in three kinds:** `truth(…)` for combinational logic, `latency(in → out) <= N` for
bounded delay, and `always(in -> eventually out within N)` for bounded temporal. There is
deliberately no full LTL — only what a per-tick simulator can decide cheaply.

**The edition difference is in the cell library, not the language.** The same logic becomes a
`ComparatorAND` cell on Java and a `TorchAND` on Bedrock. Circuits that depend on quasi-connectivity
or block-update order are a compile error rather than a silent footgun.

Read on: [Redstone](/spec/redstone/).

## 4. Several buildings

Once one cottage works, reuse it.
[`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn):

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=entry class=entry side=front at=center
  window id=front side=front y=2 offset=2 size=2x2 mat_slot=glass
  roof   id=roof  kind=gable mat_slot=roof overhang=1

site hamlet:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  place id=home3 use=cottage theme=medieval north_of=home1 gap=5

  connect home1.entry to home2.entry path=@gravel
  connect home1.entry to home3.entry path=@gravel
```

**`def` is a reusable component** built on the same slot mechanism as `theme` and `site`, so
references work the same way across editing, theming, and multi-building.

**Placement is relational.** `east_of=home1 gap=4` is a constraint; turning it into coordinates is
the compiler's job. That removes the single worst class of LLM arithmetic error.

**Structs expose ports.** `home1.entry` is the door member declared in the `def`, and `connect`
joins two ports with a walkway.

**The 48³ structure-block limit dissolves.** A village too large for one structure block is just
several `def`s composed on a `site`.

Read on: [Components, Editing, and Multi-building](/spec/components-editing-sites/).

## Where to go next

| If you want to… | Read |
|---|---|
| Change part of a build without rewriting it | [Editing model §9.2](/spec/components-editing-sites#92-editing-model) — `edit window[class=vent] set shape=arch` |
| Know which Minecraft versions a file works on | [Versioning and Editions §10.5](/spec/versioning-editions#105-which-version-is-it-for-has-three-answers) — the `cairn info` report |
| Bring an existing schematic into Cairn | [Ecosystem Interop](/spec/ecosystem-interop/) — transliterate, lift, voxel-diff |
| Try the other roof kinds | The `roof-shed`, `roof-hip`, and `roof-flat` [examples](/examples/) |
| Look up a term | [Glossary](/spec/glossary/) |
