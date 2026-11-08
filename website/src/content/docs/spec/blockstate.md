---
title: "6. Blockstate Model"
---

## 6.1 Derive by default, promote on override

**Subject to change.** Of the overrides below, only a stair's `facing`, `half` and `shape`, a door's
`hinge` and `open`, and a chest's `facing` and `size` are built. The others and the `beam` and
`note_block` keywords are not ([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

By default the compiler derives blockstate from position and neighbours: stair facing, door
orientation, glass-pane and fence and wall connections, chest left/right, and so on.

Of these, stair geometry, door and chest orientation, pane, fence and wall connections, and chest
pairing are derived today. A `roof` gets its stairs' `facing`, `half` and `shape` from where they
sit. An eave `stair` gets only its cardinal `facing`, from the side it sits on, and its `half` and
`shape` are what its line writes, or `top` and `straight`. Connections and pairs are set once every
member is painted ([Connections](#connections)), and `waterlogged` where a water `fill` overlaps a
block ([§6.3](#63-waterlogged)). The torch and sign substitution below is not derived yet.

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

A `chest` sets one chest block against a wall. It takes the cell a `pressure_plate` with the same
`at=`, `offset=` and `y=` would take: one voxel inside the wall for `at=inside.<side>` and one
outside it for `at=<side>.outside`. Its line is refused or deferred wherever the plate's would be
([Redstone §14.2](/spec/redstone/#142-signal-binding)). Its `facing` points away from the wall it
stands against, into the struct from the inside and out of it from the outside. A `facing=` on its
line turns it: `in` toward the struct's inside, `out` toward its outside, and `left` or `right`
along the wall, as seen from outside it, so `left` is toward `offset=0`. A chest pairs with the
chest beside it ([Connections](#connections)), and `size=single` keeps it single. Any other
identifier for `facing=` or `size=` is `E_INVALID_MEMBER`, and a value that is no identifier, such
as `size=2x3`, is `W_IGNORED_ARGUMENT` and read as if the key were not written. The chest's
`mat_slot=` names its block, and a chest without one is built from the registry pack's
`@chest.default` ([Materials and Themes §7.2](/spec/materials-themes/#tier-1-tokens)). The block
MUST be a chest: one the registry pack's `connections` component reads as a chest, a block whose
`type` is `single`, `left` or `right` ([Versioning and Editions
§10.3](/spec/versioning-editions/#103-backend--data-tables)), so an ender chest is not one. Where
the pack cannot say, a block whose id is `chest` or ends in `_chest` is taken as one. Any other is
`E_INCOMPATIBLE_MATERIAL` ([Lint](/spec/lint/)), and the chest is built from `@chest.default` beside
the error. A chest holds no contents ([Entities §8.3](/spec/entities/#83-where-the-line-is-drawn)).

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

The example below uses keywords the compiler does not know yet. Only its `stair` and `chest` lines
are accepted: `beam` and `note_block` are `E_UNKNOWN_KEYWORD`.

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # eaves
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                             # horizontal beam
chest id=store  at=inside.back offset=2 size=single
note_block at=2,1,2 instrument=bit note=12
```

### Connections

Once every member of a body is painted, each glass pane, run of bars, fence, wall and fence gate in
it takes its connection states from the blocks beside it, as the game sets them when a block is
placed. Each chest in it pairs by the rule below, which is Cairn's own: the game pairs chests in the
order they are placed, which a build does not have. A face of a block is **sturdy** when the block's
model covers it whole: a full block's every face, a bottom slab's bottom, an upright stair's bottom
and back. Leaves, shulker boxes, barriers, melons, pumpkins, carved pumpkins and jack o'lanterns are
never joined by a face.

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
- A chest's `type` pairs it with a chest of the same block and `facing` beside it, on its own left
  or right. A row of such chests pairs two by two from its end on the chests' own left, the end
  toward `offset=0` for a row facing into the struct from inside a wall, and an odd chest at its
  other end stays `single`. The chest whose partner is on its own right is `left`, and its partner
  is `right`. A chest that writes its `type`, as `size=single` does, pairs with none.

A state the block's material or line writes is kept, and only the others are derived:
`@glass_pane[north=true]` keeps `north=true` and joins its other sides as above. The states are
resolved state, never intent ([§6.2](#62-intent_state-and-resolved_state)).

The blocks beside are those of the same `struct` or `place` body, and nothing outside its array
counts. What each block is to these rules comes from the target's registry pack ([Versioning and
Editions §10.3](/spec/versioning-editions/#103-backend--data-tables)). A lowering with no version
pinned reads a block as every version of the edition that declares it agrees, and as nothing where
they disagree; without a pack nothing is derived. A Bedrock target reads its block as the canonical
blocks its state map names that take the block's states, and its writer maps the derived states as
it maps every other. A block the pack cannot answer for is nothing to the rules: one no canonical
block becomes, such as Bedrock's `border_block`, or one the canonical blocks it may be disagree on.
Bedrock keeps a chest's pairing in its block entity, which no writer fills yet, so a Bedrock build
writes each half of a pair as a single chest and says so with `W_INTENT_DEGRADED` ([Versioning and
Editions §10.7](/spec/versioning-editions/#107-java--bedrock-portability)).

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

A member's `intent_state` holds the blockstate keys its keyword names, and only those: on a `stair`,
`half`, `facing`, `shape` and `waterlogged`, on a `door`, `hinge` and `open`, and on a `chest`,
`facing`, `size`, the line's spelling of its `type`, and `waterlogged`. No other keyword names one
yet. Every other `key=value` the line writes is one of the member's arguments, apart from `id`,
`class` and `mat_slot`, which become fields of their own. Where a key goes depends on the keyword:
`shape=` on a `stair` is intent, and `shape=` on a `window` is an argument, the opening's outline
([Entities §8.2](/spec/entities/#82-anchor-conventions)).

Lowering records each member's `resolved_state` beside the block array it builds: every state the
member's cells hold once the body is painted, with the cells that hold it as runs along `x`, each
from its first cell, in block-array order. A cell belongs to the member that painted it last, so a
door's block is recorded on the door and not on the wall it was set into, and a member none of whose
cells survive records nothing. `cairn lower --format json` prints the record under `members`, keyed
like `structures`, which is what lets two builds be compared member by member. A body lists its
members in the order of the first cell each owns, so reordering lines that share no cell leaves the
record as it was, as it leaves the grid ([Compilation Model
§4.8](/spec/compilation/#48-within-phase-conflicts-and-the-palette)). The record is made each time a
body is lowered: its block ids are those the lowering's target resolved, a theme's edition variant
and a pinned version's override included, and its property names are the canonical vocabulary's.

In place of `type`, a member carries the `role` its keyword classifies to.

`bed` is treated as a block member rather than an entity, to keep the IR types clean.

## 6.3 `waterlogged`

**Subject to change.** Flowing water is not built yet: `water` is `E_UNKNOWN_KEYWORD`, and a `fill`
pours only source water ([Compatibility Tiers](/spec/compatibility/#subject-to-change)). In the
example below, `trapdoor` and `water` are `E_UNKNOWN_KEYWORD`.

The default is paint-derived: when `fill fluid=water` overlaps a waterloggable block, the compiler
sets `waterlogged`.

A `fill` names a box by two of its opposite corners, `from=` and `to=`, in either order and both in
the box. Each is a coordinate triple, a cell of the struct
([Syntax §5.1.1](/spec/syntax/#511-literals-and-separators)). With a `mat_slot=`, the fill paints
every cell of the box with that material in `massing`, and has no default block, so one whose
material does not resolve leaves its cells as they are. With `fluid=water`, it pours water once
every other member is painted, in `fluids` ([Compilation Model
§4.1](/spec/compilation/#41-phase-evaluation)). A cell of the box that holds no block takes a water
source, `minecraft:water[level=0]`. A waterloggable block whose state does not write `waterlogged`
takes `waterlogged=true` and stays its own member's ([§6.2](#62-intent_state-and-resolved_state)).
Any other block is left as it was painted, so water fills only around what stands in the box.

A block is waterloggable when the target's registry pack gives it a `waterlogged` state that takes
`true` and its state leaves room for water. A slab written `type=double` fills its cell and is not
waterloggable, as the game never puts water in one. A block whose states the pack does not carry,
such as a Bedrock block with no Java counterpart, is not waterloggable either, and water leaves it
as it was painted. A lowering with no version pinned reads a block as every version of the edition
that declares it agrees, as [Connections](#connections) do, and without a pack no block is
waterloggable. The derived `waterlogged` is resolved state, never intent. A Bedrock backend holds
the water of a waterlogged block in the structure's second layer ([Versioning and Editions
§10.7](/spec/versioning-editions/#107-java--bedrock-portability)).

`water` is the one fluid and `kind=source` the one kind, which a fill with no `kind=` pours too. A
`fill` that writes both `mat_slot=` and `fluid=`, another fluid or kind, a `from=` or `to=` that is
missing or not a triple, or a corner outside the struct is `E_INVALID_MEMBER`; a `kind=` on a fill
with no `fluid=` is `W_IGNORED_ARGUMENT`, and a fill with neither `mat_slot=` nor `fluid=` is
`E_MISSING_MATERIAL` ([Lint](/spec/lint/)). A fill does not size the volume
([Compilation Model §4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)): one that
reaches a row the struct as written reaches and does not build is `W_DEFERRED_MEMBER`, waiting on
the member that would build the row.

A three-valued `waterlogged=auto|true|false` is allowed, for leaving an air pocket inside a tank
(explicit `false`), distinguishing source from flowing, and version differences in the waterloggable
table. On a `stair` or a `chest`, `waterlogged=true` and `waterlogged=false` are written into the
block's state and kept, and `waterlogged=auto`, like no `waterlogged=` at all, lets a water fill set
it. Any other word is `E_INVALID_MEMBER`. Flowing water is made explicit with `flow=` and `level=`.

Like the overrides in [§6.1](#61-derive-by-default-promote-on-override), `waterlogged=` is written
on a member that places one block and names that block's state, and of the keywords the compiler
knows only `stair` and `chest` do so for a block that can hold water. The block of a `door` or a
`pressure_plate` holds none, and a `roof` or a `window` derives the state of every block it lays
and takes no override, so `waterlogged=` on any of the four is `E_UNKNOWN_ARGUMENT`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=.. waterlogged=false          # an air window in a tank
water id=stream from=.. flow=east level=4
```
