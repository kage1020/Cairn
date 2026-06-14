---
title: "Cairn Tutorial"
---

A guided walk through the [Examples](../examples) directory. Each section maps every line of
an example to the spec chapter behind it, so the tutorial doubles as an annotated reading list.

This tutorial assumes you have already read [Purpose and Scope](../spec/overview) and
[Design Principles](../spec/principles). If terminology trips you up, the
[glossary](../spec/glossary) is the fastest jump table.

> The reference compiler is not implemented yet. The `cairn compile` invocations are aspirational
> but match the spec exactly, so reading them is still the right way to build intuition for the
> CLI surface.

## 1. A minimum useful build — [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn)

The "Hello, world!" of Cairn: a cottage with a door, a window, and a gable roof.

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

What to notice:

1. **Headers are optional but cheap.** `@cairn` records the Cairn language version the file was
   written against; `@requires` is a hard capability floor on the Minecraft target. The Minecraft
   version itself is **never** written in the source — only in `--target` at compile time.
   ([syntax §5.3](../spec/syntax))
2. **`theme` separates "what" from "where".** The structure carries `mat_slot` injection points;
   the theme binds them to canonical block tokens (`@oak_planks`, etc.). Switching themes never
   touches the structure. ([materials-themes §7.1](../spec/materials-themes))
3. **One line, one command, key=value.** The leading keyword (`floor`, `walls`, `door`…) is the
   only positional token; everything else is `key=value`. This is deliberate: it stabilizes LLM
   generation by giving every parameter an attention anchor.
   ([principles P3](../spec/principles), [syntax §5.1](../spec/syntax))
4. **Selectors are semantic, not coordinates.** `side=front`, `offset=2`, `y=2`, `at=center`
   refer to wall position and offsets along the wall. No absolute coordinates appear in the
   author surface. ([principles P4](../spec/principles),
   [syntax §5.4](../spec/syntax))
5. **Phase order, not source order.** The `window` is written after the `roof` but is still cut
   as an opening in the wall — the compiler sorts commands into a fixed phase pipeline before
   evaluating them. ([compilation §4.1](../spec/compilation), [principles
   P2](../spec/principles))
6. **Blockstate is derived by default.** No one writes `facing=south` for the door, the
   `north=tall` for the wall, or the `connected` state for the glass pane. The compiler derives
   them from position and neighbors. ([blockstate §6.1](../spec/blockstate))

Compile (aspirational; the reference compiler is still a skeleton):

```sh
cairn compile examples/cottage.crn --edition java    --target 1.21.4
cairn compile examples/cottage.crn --edition bedrock --target 1.21.40
```

## 2. Themes, abstract tokens, override-promotion — [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn)

A two-floor stone keep introduces three new ideas: **abstract material tokens**, **levels**, and
**override-promotion**.

```
theme keep_dark:
  slot floor -> @floor.wood.broadleaf   # abstract token
  slot wall  -> @wall.stone.cobble
  slot trim  -> @wood.dark
  slot roof  -> @roof.dark_wood

struct keep size=11x9
  ...
  level id=floor2 y=5
    walls  id=upper class=outer mat_slot=wall height=4
    window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2 shape=slit
    stair  id=eave kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left
```

What to notice:

1. **Two tiers of canonical token.**
   ([materials-themes §7.2](../spec/materials-themes))
   - `@oak_planks` is a *canonical block token*: a meaning. Silent downgrades are forbidden.
   - `@floor.wood.broadleaf` is an *abstract material token*: an aesthetic choice that theme
     policy MAY downgrade (oak ↔ birch) depending on the target.
2. **`level`** gives you a per-floor local `y=0`, so the second floor's window stays at `y=2`
   from its own floor rather than from the world floor. ([open-issues §15.2](../spec/open-issues)
   reserves the right to refine this surface, but the present syntax is stable enough to teach.)
3. **Override-promotion.** The `stair id=eave` line writes `half=top facing=out
   shape=outer_left` explicitly — those values are now *intent*, not derived. The blockstate
   model is "derive by default; any blockstate that can be intent is overridable." Read
   [blockstate §6.1](../spec/blockstate) for the full list of cases that must remain
   overridable.
4. **`shape=slit` window primitive.** Every primitive carries `anchor`, declared bbox, and host
   face in the IR, so an arrow slit with a non-rectangular shape still composes cleanly with the
   wall blockstate around it. ([entities §8.2](../spec/entities))

## 3. Logical redstone — [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn)

The redstone surface is the most spec-leaning part of Cairn: instead of placing dust and
repeaters, you declare a *signal graph* and the compiler synthesizes, places, and routes the
circuit.

```
pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step
pressure_plate id=inner at=inside.front  offset=0 y=0 -> sig.exit

logic sig.open = sig.step or sig.exit
door[id=front] opened_by=sig.open

circuit region=floor void=2

assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }
assert always(sig.step -> eventually sig.open within 2)
```

What to notice:

1. **The signal graph is the IR.** `sig.*` names a dataflow node. Sensors emit signals,
   actuators consume them, and `logic` writes the dependencies between them.
   ([redstone §14.2–14.3](../spec/redstone))
2. **No tick arithmetic.** The logic expression contains no time. The `within 2` in the assertion
   is the *only* place a number-as-ticks appears. Delay is determined for the first time in the
   Placement IR. ([redstone §14.4, §14.8](../spec/redstone))
3. **`circuit region=…`** reserves space for place-and-route. If routing congestion exceeds the
   reserved area, you get an `E_ROUTE_CONGESTION` error with a suggested fix; the compiler will
   never silently overflow.
4. **Three assertion kinds.** `truth(…)` for combinational, `latency(in → out) <= N` for
   bounded delay, and `always(in -> eventually out within N)` for bounded temporal. There is no
   full LTL by design — only what a per-tick simulator can decide cheaply.
   ([redstone §14.7](../spec/redstone))
5. **Edition difference is in the cell library, not the language.** The same logic compiles to a
   `ComparatorAND` cell on Java and a `TorchAND` cell on Bedrock; QC/BUD-dependent circuits are a
   compile error rather than a silent footgun. ([redstone §14.6](../spec/redstone))

## 4. Multi-building — [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn)

Once one cottage works, you reuse it on a site. The site never asks you to compute absolute
coordinates.

```
def cottage class=house size=9x7:
  ...

site hamlet:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  place id=home3 use=cottage theme=medieval north_of=home1 gap=5

  connect home1.entry to home2.entry path=@gravel
  connect home1.entry to home3.entry path=@gravel
```

What to notice:

1. **`def` is a slot-bearing component.** Same mechanism as `theme` and `site`, so the reference
   system never fractures across editing, theming, and multi-building.
   ([components-editing-sites §9.1](../spec/components-editing-sites))
2. **Topological placement.** `east_of=home1 gap=4` is a constraint; absolute coordinates are the
   compiler's job. This sidesteps the worst class of LLM arithmetic errors.
   ([principles P4](../spec/principles),
   [components-editing-sites §9.3](../spec/components-editing-sites))
3. **Each struct exposes ports.** `home1.entry` refers to the door member declared in the `def`;
   `connect` joins two ports through a path slot.
4. **The 48³ structure-block limit dissolves.** Villages and castles too large for a single
   structure block are expressed as the composition of several `def`s placed on a `site`.

## Next steps

- **Editing.** See [components-editing-sites §9.2](../spec/components-editing-sites) for the
  patch DSL (`edit window[class=vent] set shape=arch`). Edit diffs look only at
  `intent_state`, so deriving the resolved state again across an edit is safe.
- **Targeting.** See [versioning-editions §10.5](../spec/versioning-editions) for
  `cairn info`, which reports the registry-compatible range and the semantic-sensitive members
  for a file.
- **Import.** See [ecosystem-interop §12](../spec/ecosystem-interop) for the
  `cairn import` workflow: faithful transliteration first, LLM-driven semantic lift second,
  voxel-diff to drive the loop.
- **Evaluation metrics.** If you want to push back on the spec, the four metrics in
  [evaluation §13.1](../spec/evaluation) are the language Cairn argues in.
