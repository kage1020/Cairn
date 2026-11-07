---
title: "6. Blockstate Model"
---

## 6.1 Derive by default, promote on override

**Subject to change.** Of the overrides below, only a stair's `facing`, `half` and `shape` are
built. The others and the `beam`, `chest` and `note_block` keywords are not
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

The example below uses keywords the compiler does not know yet. Only its `stair` line is accepted:
`beam`, `chest` and `note_block` are `E_UNKNOWN_KEYWORD`.

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

A member's `intent_state` holds the blockstate keys its keyword names, and only those: on a
`stair`, `half`, `facing`, `shape` and `waterlogged`. No other keyword names one yet. Every other
`key=value` the line writes is one of the member's arguments, apart from `id`, `class` and
`mat_slot`, which become fields of their own. Where a key goes depends on the keyword: `shape=` on a
`stair` is intent, and `shape=` on a `window` is an argument, the opening's outline
([Entities §8.2](/spec/entities/#82-anchor-conventions)).

Lowering records each member's `resolved_state` beside the block array it builds: every state the
member's cells hold once the body is painted, with the cells that hold it. A cell belongs to the
member that painted it last, so a door's cut is recorded on the door and not on the wall it cut, and
a member none of whose cells survive records nothing. `cairn lower --format json` prints the record
under `members`, keyed like `structures`, which is what lets two builds be compared member by
member. It is written once, in the canonical vocabulary, and each backend maps it to its own edition
as it writes ([Versioning and Editions §10.7](/spec/versioning-editions/#107-java--bedrock-portability)).

In place of `type`, a member carries the `role` its keyword classifies to. The two answer different
questions, so `type` will sit beside `role` rather than replace it: `role` is the architectural
function a selector matches, and `type` the kind of object the compiler places for it. `type` is
added with the first keyword that places a block entity or an entity.

`bed` is treated as a block member rather than an entity, to keep the IR types clean.

## 6.3 `waterlogged`

**Subject to change.** Nothing in this section is built yet
([Compatibility Tiers](/spec/compatibility/#subject-to-change)). No pass sets or reads
`waterlogged`: on a `stair`, `waterlogged=false` is an unreached key, reported as
`W_IGNORED_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)), and the stair is built as if
the key were not written. In the example below, `fill`, `trapdoor` and `water` are
`E_UNKNOWN_KEYWORD`.

The default is paint-derived: when `fill fluid=water` overlaps a waterloggable block, the compiler
sets `waterlogged`.

A three-valued `waterlogged=auto|true|false` is allowed, for leaving an air pocket inside a tank
(explicit `false`), distinguishing source from flowing, and version differences in the waterloggable
table. Flowing water is made explicit with `flow=` and `level=`.

Like the overrides in [§6.1](#61-derive-by-default-promote-on-override), `waterlogged=` is written
on a member that places one block and names that block's state, and of the keywords the compiler
knows only `stair` does. A `roof` or a `window` derives the state of every block it lays and takes
no override, so `waterlogged=` on either is `E_UNKNOWN_ARGUMENT`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=.. waterlogged=false          # an air window in a tank
water id=stream from=.. flow=east level=4
```
