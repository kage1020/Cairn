---
title: "6. Blockstate Model"
---

## 6.1 Derive by default, promote on override

**Subject to change.** Of the overrides below, only a stair's `facing`, `half` and `shape` and a
door's `hinge` and `open` are built. The others and the `beam`, `chest` and `note_block` keywords
are not ([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

By default the compiler derives blockstate from position and neighbours: stair facing, door
orientation, glass-pane and fence and wall connections, chest left/right, and so on.

Of these, stair geometry, door orientation and pane, fence and wall connections are derived
today. A `roof` gets its stairs' `facing`, `half` and `shape` from where they sit. An eave `stair`
gets only its cardinal `facing`, from the side it sits on, and its `half` and `shape` are what its
line writes, or `top` and `straight`. Connections are set once every member is painted
([Connections](#connections)). Chest pairing and the torch and sign substitution below are not
derived yet.

A `door` sets a door block two rows tall into the opening it cuts ([Syntax
§5.4](/spec/syntax/#54-selectors)): `half=lower` on the row it opens at and `half=upper` on the row
above. Its `facing` points into the struct, so the closed door sits on the outer face of its wall
and swings inward, and it is built closed, `open=false`. Its `hinge` is read from the wall beside it
once every opening is cut. Left and right are as seen from outside, so a door's left is toward
`at=left`. For each side, count the door's two rows at which the cell beside it still holds a block
of a `walls` member; a side has a door when another door's lower half stands beside the door's lower
half. The hinge is on the right when the door has a door on its left and none on its right, or more
wall on its right than on its left, and on the left otherwise: a door in an unbroken wall is hinged
on the left, one beside a window is hinged on the wall's side, and two side by side are hinged on
their outer sides, so they open as a pair. A `hinge=left` or `hinge=right`, and an `open=true` or
`open=false`, on the door's line replace what is derived.

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

### Connections

Once every member of a body is painted, each glass pane, run of bars, fence, wall and fence gate in
it takes its connection states from the blocks beside it, as the game sets them when a block is
placed. A face of a block is **sturdy** when the block's model covers it whole: a full block's
every face, a bottom slab's bottom, an upright stair's bottom and back. Leaves, shulker boxes,
barriers, melons, pumpkins, carved pumpkins and jack o'lanterns are never joined by a face.

- A pane's `north`, `east`, `south` and `west` are `true` toward a sturdy face, a pane or a wall.
- A fence's are `true` toward a sturdy face, a fence of its own group, or a fence gate facing
  across that side. A wooden fence's group is the wooden fences, and every other fence is in the
  other group, as the game's `wooden_fences` tag divides them.
- A wall's sides join a sturdy face, a wall, a pane, or a fence gate facing across them. A side
  that joins nothing is `none`. One that joins is `tall` where the block above covers it, and
  `low` otherwise.
- A block covers a part of the wall under it, the post or a side, when the boxes of its model that
  rest on its bottom cover that part seen from above, as the game's `WallBlock` tests them: the
  post is the two-by-two square in the middle of the cell, and a side the strip as wide from the
  middle to that edge. A full block or a bottom slab covers the post and every side, and a lantern
  or a flower pot only the post. A connecting block above is read by its connection states
  instead: a pane or a fence covers the post and each side it joins, a wall each side it joins and
  the post when its own post is up or a side joins, and a closed fence gate the post and the sides
  it spans. An open one covers nothing.
- A wall's `up`, its post, rises under a wall whose post is up, where no side joins, and where a
  side joins without the one opposite it, as at an end, a corner or a branch. Otherwise it stays
  down where two opposite sides stand tall, and anywhere else rises only under a block the game's
  `wall_post_override` tag lists, a torch or a sign among them, or one that covers the post.
- A fence gate's `in_wall` is `true` when a wall stands beside it, across the way it faces.

A state the block's material or line writes is kept, and only the others are derived:
`@glass_pane[north=true]` keeps `north=true` and joins its other sides as above. The states are
resolved state, never intent ([§6.2](#62-intent_state-and-resolved_state)).

The blocks beside are those of the same `struct` or `place` body, and nothing outside its array
counts. What each block is to these rules comes from the target's registry pack
([Versioning and Editions §10.3](/spec/versioning-editions/#103-backend--data-tables)). A lowering
with no version pinned reads a block as every version of the edition that declares it agrees, and
as nothing where they disagree; without a pack nothing is derived. A Bedrock target reads its block
as the canonical blocks its state map names that take the block's states, and its writer maps the
derived states as it maps every other. A block the pack cannot answer for is nothing to the rules:
one no canonical block becomes, such as Bedrock's `border_block`, or one the canonical blocks it
may be disagree on.

A face, and what a block covers, are read off the block's model as the release's client draws it,
while the game reads its collision shape. The two disagree for a few blocks. Honey, cactus and
powder snow are drawn whole and collide inset. A block the game draws in code, such as a chest or
a head, and a barrier, which has no model, cover nothing here. A block whose shape is one box
around the parts its model draws, such as a campfire or two candles, covers here only what those
parts do. One that rests on its bottom without colliding, such as a floor button or one layer of
snow, covers here what its model does.

## 6.2 `intent_state` and `resolved_state`

```yaml
member:
  id: eave
  type: block | block_entity | entity      # typed in the IR; the distinction is the compiler's job
  primitive: stairs
  intent_state:   { half: top, facing: out, shape: outer_left }  # author intent
  resolved_state:                                                # derived, including paint-derived state
    - state: { id: minecraft:oak_stairs, properties: { facing: south, half: top, shape: outer_left } }
      runs: [{ from: [0, 4, 0], len: 1 }]
```

The two are named apart from Minecraft's own term "blockstate" on purpose. Keeping resolved state
(derived or paint-origin) out of intent state (authored) is what makes edits stable.

A member's `intent_state` holds the blockstate keys its keyword names, and only those: on a
`stair`, `half`, `facing`, `shape` and `waterlogged`, and on a `door`, `hinge` and `open`. No other
keyword names one yet. Every other `key=value` the line writes is one of the member's arguments,
apart from `id`, `class` and `mat_slot`, which become fields of their own. Where a key goes depends
on the keyword: `shape=` on a `stair` is intent, and `shape=` on a `window` is an argument, the
opening's outline ([Entities §8.2](/spec/entities/#82-anchor-conventions)).

Lowering records each member's `resolved_state` beside the block array it builds: every state the
member's cells hold once the body is painted, with the cells that hold it as runs along `x`, each
from its first cell, in block-array order. A cell belongs to the member that painted it last, so a
door's block is recorded on the door and not on the wall it was set into, and a member none of
whose cells survive records nothing. `cairn lower --format json` prints the record under `members`,
keyed like `structures`, which is what lets two builds be compared member by member. A body lists
its members in the order of the first cell each owns, so reordering lines that share no cell leaves
the record as it was, as it leaves the grid ([Compilation Model
§4.8](/spec/compilation/#48-within-phase-conflicts-and-the-palette)). The record is made each time a
body is lowered: its block ids are those the lowering's target resolved, a theme's edition variant
and a pinned version's override included, and its property names are the canonical vocabulary's.

In place of `type`, a member carries the `role` its keyword classifies to.

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
knows only `stair` does so for a block that can hold water. A `door`'s block holds none, and a
`roof` or a `window` derives the state of every block it lays and takes no override, so
`waterlogged=` on any of the three is `E_UNKNOWN_ARGUMENT`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=.. waterlogged=false          # an air window in a tank
water id=stream from=.. flow=east level=4
```
