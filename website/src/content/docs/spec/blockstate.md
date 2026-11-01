---
title: "6. Blockstate Model"
---

## 6.1 Derive by default, promote on override

**Subject to change.** Of the overrides below, only a stair's `facing`, `half` and `shape` are
built. The others, the `beam`, `chest` and `note_block` keywords, and coordinate triples are not
([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

By default the compiler derives blockstate from position and neighbours: stair facing, door
orientation, glass-pane and fence and wall connections, chest left/right, and so on.

Of these, only stair geometry is derived today. A `roof` gets its stairs' `facing`, `half` and
`shape` from where they sit. An eave `stair` gets only its cardinal `facing`, from the side it sits
on, and its `half` and `shape` are what its line writes, or `top` and `straight`. A `door` cuts its
opening and places no door block. Pane, fence and wall connections, chest pairing and the torch and
sign substitution below are not derived yet, so `cairn lower examples/cottage.crn` writes a bare
`minecraft:glass_pane`.

Any blockstate that can be architectural intent is overridable, and **the moment you write it, it is
promoted to intent**. Cairn does not adopt the strong reading, "if it can be derived, don't let
the author write it". The rule is "derive by default; any blockstate that can be intent is
overridable".

Cases that MUST stay overridable, because each can be intent:

| Group | States |
|---|---|
| Stairs | `facing` (a stair as a chair or decoration), `half=top` (upside-down eaves), `shape` |
| Furniture | `chest size=single` (auto-merging from adjacency is forbidden), `bed facing`, `door hinge` / `open` |
| Orientation | `log` / `pillar axis` (a horizontal beam), `trapdoor open` / `half`, `glazed_terracotta` rotation |
| Counts | `snow layers`, `candle count` |
| Redstone | `redstone_dust connect`, `repeater delay`, `observer` / `piston` / `dispenser facing`, `note` / `instrument` |

Two easy-to-miss cases belong to derivation, not intent: `torch` ↔ `wall_torch` and `sign` ↔
`wall_sign` are substituted automatically by attachment face.

The example below uses syntax the parser does not accept yet. Only its `stair` line is accepted:
`beam`, `chest` and `note_block` are `E_UNKNOWN_KEYWORD`, and a coordinate triple such as
`at=2,1,2` is `E_PARSE`.

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # eaves
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                             # horizontal beam
chest id=store  at=inside.back size=single
note_block at=2,1,2 instrument=bit note=12
```

## 6.2 `intent_state` and `resolved_state`

```yaml
member:
  id: eave
  type: block | block_entity | entity      # typed in the IR; the distinction is the compiler's job
  primitive: stairs
  intent_state:   { half: top, facing: out, shape: outer_left }  # author intent. Edit diffs look only here
  resolved_state: { facing: south, waterlogged: false }          # derived, including paint-derived state
```

The two are named apart from Minecraft's own term "blockstate" on purpose. Keeping resolved state
(derived or paint-origin) out of intent state (authored) is what makes edits stable.

The IR does not hold this shape yet. `resolved_state` is never populated: the blockstate that
lowering derives goes straight into the block array's palette. `intent_state` carries every
`key=value` the line writes, blockstate or not, except `id`, `class` and `mat_slot`, which become
fields of their own. In place of `type`, a member carries the `role` its keyword classifies to.

`bed` is treated as a block member rather than an entity, to keep the IR types clean.

## 6.3 `waterlogged`

**Subject to change.** Nothing in this section is built yet
([Compatibility Tiers](/spec/compatibility/#subject-to-change)). No pass sets or reads
`waterlogged`: on a `stair`, `waterlogged=false` is `E_UNKNOWN_ARGUMENT`. In the example below,
`fill`, `trapdoor` and `water` are `E_UNKNOWN_KEYWORD`, and a coordinate triple such as
`from=1,1,1` is `E_PARSE`.

The default is paint-derived: when `fill fluid=water` overlaps a waterloggable block, the compiler
sets `waterlogged`.

A three-valued `waterlogged=auto|true|false` is allowed, for leaving an air pocket inside a tank
(explicit `false`), distinguishing source from flowing, and version differences in the waterloggable
table. Flowing water is made explicit with `flow=` and `level=`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=.. waterlogged=false          # an air window in a tank
water id=stream from=.. flow=east level=4
```
