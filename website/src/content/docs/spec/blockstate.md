---
title: "6. Blockstate Model"
---

## 6.1 Derive by default, promote on override

By default the compiler derives blockstate from position and neighbours: stair facing, door
orientation, glass-pane and fence and wall connections, chest left/right, and so on.

Of these, stair geometry, the orientation of beams, doors, chests, trapdoors, signs, beds and
redstone components, pane, fence, wall and redstone dust connections, and chest pairing are derived
today. A
`roof` gets its stairs' `facing`, `half` and `shape` from where they sit. An eave `stair` gets only
its cardinal `facing`, from the side it sits on, and its `half` and `shape` are what its line
writes, or `top` and `straight`. Connections and pairs are set once every member is painted
([Connections](#connections)), and `waterlogged` where a water `fill` overlaps a block
([§6.3](#63-waterlogged)). Both substitutions below, a torch's and a sign's, are derived.

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
identifier for `facing=` or `size=` is `E_INVALID_MEMBER`, and for a compass direction under
`facing=` the message names the word that turns the chest that way from its wall. A value that is
no identifier, such as `size=2x3`, is `W_IGNORED_ARGUMENT` and read as if the key were not written. The chest's
`mat_slot=` names its block, and a chest without one is built from the registry pack's
`@chest.default` ([Materials and Themes §7.2](/spec/materials-themes/#tier-1-tokens)). The block
MUST be a chest: one the registry pack's `connections` component reads as a chest, a block whose
`type` is `single`, `left` or `right` ([Versioning and Editions
§10.3](/spec/versioning-editions/#103-backend--data-tables)), so an ender chest is not one. Where
the pack cannot say, a block whose id is `chest` or ends in `_chest` is taken as one. Any other is
`E_INCOMPATIBLE_MATERIAL` ([Lint](/spec/lint/)), and the chest is built from `@chest.default` beside
the error. A chest holds no contents ([Entities §8.3](/spec/entities/#83-where-the-line-is-drawn)).

A `lever`, `button`, `daylight`, `observer`, `lamp`, `piston` or `dispenser` takes its cell as a
chest does ([Redstone §14.2](/spec/redstone/#142-signal-binding)). A lever or button hangs on the
wall: `face=wall`, and a `facing` away from the wall, which nothing on its line overrides. An
observer, piston or dispenser faces away from the wall unless a `facing=` on its line turns it,
with `up` and `down` beside a chest's four words. Each is built unpowered, so its `powered`,
`lit`, `extended` or `triggered` is never written.

The Tier 0 parts, `repeater`, `comparator`, `redstone_torch` and `redstone_dust`, take their
cells and their turns as [Redstone §14.1](/spec/redstone/#tier-0-parts) says. A repeater's or
comparator's `facing=` names the way it drives, the way its front looks, and the game's `facing` is
the way its back looks, so the block takes the opposite one. A torch set by a wall, or at a triple
with a `facing=`, is the wall torch, its `facing` the way it points, and a torch at a triple with
none is the standing torch. A repeater's `delay` and a comparator's `mode` are what the line writes,
or `1` and `compare`. Each part is built with nothing powering it, so its `powered`, `locked`, `lit`
or `power` is never written and the block's own default stands: a repeater neither powered nor
locked, a comparator unpowered, a torch lit and dust at power 0.

A `trapdoor` sets one trapdoor block. By a wall, `at=<side>.outside` or `at=inside.<side>` with
`offset=` and `y=`, it takes the cell a chest with the same `at=`, `offset=` and `y=` would take,
and its line is refused or deferred wherever the chest's would be. Its `facing` points away from the
wall, so the trapdoor is hinged on the wall's side of its cell and, open, lies flat against the
wall, and a `facing=` of `in`, `out`, `left` or `right` turns it as it turns a chest. At a
coordinate triple, `at=x,y,z`, it takes the cell a Tier 0 part would ([Redstone
§14.1](/spec/redstone/#tier-0-parts)), and its `facing=` is a compass word, `north`, `south`, `east`
or `west`; with none, the block's own `facing` stands. A trapdoor is built closed in the bottom half
of its cell, `open=false` and `half=bottom`, and an `open=true` or `half=top` on its line replaces
either. It is built unpowered, so its `powered` is never written. Any other identifier for `facing=`
or `half=` is `E_INVALID_MEMBER`, as is a compass word by a wall and a way from a wall at a triple;
a value that is no identifier, and an `open=` that is no boolean, is `W_IGNORED_ARGUMENT` and read
as if the key were not written. The trapdoor's `mat_slot=` names its block, and a trapdoor without
one is built from the registry pack's `@trapdoor.default`. The block MUST be a trapdoor, one whose
id is `trapdoor` or ends in `_trapdoor`; any other is `E_INCOMPATIBLE_MATERIAL`, and the trapdoor is
built from `@trapdoor.default` beside the error.

A `beam` lays a run of pillar blocks on their side, and `kind=pillar` is its one kind.
`at=<side>.top` lays it along the top row of the walls under its `level`: the last row of the course
of `walls` that holds the level's first row, across the wall's cells between its two corners.
`at=<id>.top` lays it over the `door` or `window` of the same body that has that `id=`, on the row
above the opening's top row, across the opening's columns and one more on each side, still between
the wall's corners. A window's `repeat=` series and its `sym=true` mirror are each covered, as the
window's line cuts them. A side name is read before an id, and a beam's own `level` does not move a
beam laid over an opening. The beam's `axis` runs along its wall, `x` along the front and back and
`z` along the left and right, and an `axis=` of `x`, `y` or `z` on its line replaces it, so `axis=y`
stands the blocks upright in a band. A beam is laid in `envelope`, over the walls it rests on, and
an opening cut through it later is cut in `openings` ([Compilation Model
§4.1](/spec/compilation/#41-phase-evaluation)). A missing or unknown `kind=`, an `at=` of neither
form, an id no `door` or `window` of the body has, a wall too short to have a cell between its
corners, a row no `walls` member is written to reach, and any other identifier for `axis=` are
`E_INVALID_MEMBER`. A row a `walls` member is written to reach and does not paint, and an opening
whose own line does not place it, are `W_DEFERRED_MEMBER`, and an `axis=` that is no identifier is
`W_IGNORED_ARGUMENT`. The beam's `mat_slot=` names its block, and a beam without one is built from
the registry pack's `@beam.default`. The block MUST be a pillar, one whose `axis` takes `x`, `y` and
`z` at the pinned target. A lowering with no version pinned takes any block as one, and at a pinned
target any other is `E_INCOMPATIBLE_MATERIAL`, and the beam is built from `@beam.default` beside the
error. A beam does not size the volume.

A `note_block` sets one note block in the cell its coordinate triple names, `at=x,y,z` ([Syntax
§5.1.1](/spec/syntax/#511-literals-and-separators)); a note block turns no way, so it takes no wall
anchor. Its `note=`, a whole number from 0 to 24, and its `instrument=`, the name of an instrument,
are written into its block's state, and a state its line does not write is left out, so the block's
own default stands: `note` 0, and the `instrument` the game sets from the block under the note
block. The game sets the instrument again each time that block changes, so a written instrument is
the one played until then. Which instruments a note block takes is the target's: at a pinned version
an `instrument=` its note block does not take, such as one a later version adds, is `E_STATE_DOMAIN`
on the value, and no note block is built. A `note=` outside 0 to 24 and an `at=` that is no triple
or lies outside the struct are `E_INVALID_MEMBER`, and a value of the wrong kind is
`W_IGNORED_ARGUMENT`. A note block is built unpowered, so its `powered` is never written. Its block
is the registry pack's `@note_block.default`, and a `mat_slot=` on its line is `W_IGNORED_ARGUMENT`.
Bedrock keeps a note block's note in its block entity, which its writer fills ([Entities
§8.1](/spec/entities/#81-two-tiers)), and works its instrument out from the block below, so a
Bedrock build does not write an instrument, and says so with `W_INTENT_DEGRADED` where the line
writes one ([Versioning and Editions
§10.7](/spec/versioning-editions/#107-java--bedrock-portability)).

A `sign` sets one sign block. By a wall, `at=<side>.outside` or `at=inside.<side>` with `offset=`
and `y=`, it takes the cell a chest with the same `at=`, `offset=` and `y=` would take, and its
line is refused or deferred wherever the chest's would be. There it is a wall sign hung on the
wall's face, its `facing` away from the wall, so nothing is left for a `facing=` to choose, and one
on its line is `E_INVALID_MEMBER`. At a coordinate triple, `at=x,y,z`, it takes the cell a Tier 0
part would ([Redstone §14.1](/spec/redstone/#tier-0-parts)). With no `facing=` it is a standing
sign on the block under it, written `rotation=0`, its front toward the `front` wall. A `facing=` of
`north`, `south`, `east` or `west` makes it a wall sign hung on the block behind it, its front
looking that way. Any other identifier for `facing=` is `E_INVALID_MEMBER`, and a value that is no
identifier is `W_IGNORED_ARGUMENT` and read as if the key were not written. The text its line
writes is kept in the sign's block entity ([Entities §8.1](/spec/entities/#81-two-tiers)). The
sign's `mat_slot=` names a standing sign, and a sign without one is built from the registry pack's
`@sign.oak`. A hung sign is built from that sign's wall sign, the block whose id ends in
`wall_sign` where the standing sign's ends in `sign` or in `standing_sign`: `spruce_sign` hangs as
`spruce_wall_sign`, Bedrock's `spruce_standing_sign` as `spruce_wall_sign`, and the default hangs as
`@sign.oak_wall`. The block MUST be a standing sign whose wall sign the target has: its id ends in
`_sign` and in neither `wall_sign` nor `hanging_sign`, and the pinned target declares its wall sign,
or, with no version pinned, some version of the edition may. Any other is `E_INCOMPATIBLE_MATERIAL`,
and the sign is built from the default beside the error.

A `bed` lays a bed's two blocks: its head in the cell its `at=` names and its foot in the cell
behind the head, each with its `part`, `head` or `foot`, and the `facing` its head points. By a wall
its head takes the cell a chest would, and the bed lies with its head to the wall, its `facing`
toward the wall: out of the struct from inside a wall and into it from outside one. A `facing=` of
`left` or `right` turns it along the wall as it turns a chest, and the one that points its head away
from the wall, `in` from inside it or `out` from outside it, would put its foot in the wall and is
`E_INVALID_MEMBER`. At a coordinate triple its head takes the cell a Tier 0 part would, and its
`facing=` is a compass word, `north`, `south`, `east` or `west`, which it MUST write, since no wall
says which way it lies. Its foot MUST lie where its head may: inside the wall ring for a head inside
a wall, outside the struct's footprint and in its block array for a head outside one, and on the
footprint for a head at a triple. A foot anywhere else, a missing `facing=` at a triple and any
other identifier for `facing=` are `E_INVALID_MEMBER`, and no half of the bed is built. A `facing=`
that is no identifier is `W_IGNORED_ARGUMENT`, and a bed by a wall reads it as if the key were not
written. A bed is built unoccupied, so its `occupied` is never written. The bed's `mat_slot=` names
its block, and a bed without one is built from the registry pack's `@bed.default`, a red bed. The
block MUST be a bed, one whose id is `bed` or ends in `_bed`; any other is
`E_INCOMPATIBLE_MATERIAL`, and the bed is built from `@bed.default` beside the error. Its colour is
its block's, which Bedrock keeps in the block entity of each half ([Entities
§8.1](/spec/entities/#81-two-tiers)).

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
| Redstone | `redstone_dust connect`, `repeater delay`, `comparator mode`, `observer` / `piston` / `dispenser` / `repeater` / `comparator facing`, `note` / `instrument` |

A glazed terracotta's `facing`, a snow layer's `layers` and a candle's `candles` take no member:
nothing about where such a block stands derives them, so a member would have nothing to derive and
no more to say than the block's state literal does. They are written on the material that names the
block, `@white_glazed_terracotta[facing=east]`, `@snow[layers=3]` or `@candle[candles=3]`, which a
`floor`, `walls`, `fill` or raw block lays as written ([Materials and Themes
§7.4](/spec/materials-themes/#74-raw-blocks)), and at a pinned target `E_STATE_DOMAIN` checks each
pair.

Two easy-to-miss cases belong to derivation, not intent: `torch` ↔ `wall_torch` and `sign` ↔
`wall_sign` are substituted automatically by attachment face. Every member that places a torch or a
sign substitutes it: a `redstone_torch` and a `sign` take the form their anchor and `facing=` name,
and each torch a circuit lays hangs on the side of the block behind it ([Redstone
§14.5](/spec/redstone/#145-place-and-route)).

Each line of the example below writes an override.

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # eaves
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                             # horizontal beam
chest id=store  at=inside.back offset=2 size=single
trapdoor id=hatch at=inside.left offset=1 y=2 half=top open=true
note_block at=2,1,2 instrument=bit note=12
bed   id=cot    at=inside.back offset=1 y=1 facing=left
```

### Connections

Once every member of a body is painted, each glass pane, run of bars, fence, wall, fence gate and
redstone dust in it takes its connection states from the blocks beside it, as the game sets them
when a block is placed. Each chest in it pairs by the rule below, which is Cairn's own: the game
pairs chests in the order they are placed, which a build does not have. A face of a block is
**sturdy** when the block's model covers it whole: a full block's every face, a bottom slab's
bottom, an upright stair's bottom and back. Leaves, shulker boxes, barriers, melons, pumpkins,
carved pumpkins and jack o'lanterns are never joined by a face.

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
- A redstone dust's `north`, `east`, `south` and `west` are `side` toward the blocks beside it that
  the game joins dust to: dust, a repeater lying along that side, an observer whose back faces the
  dust, and the blocks that give off a signal from any side, such as a comparator, a redstone torch,
  a lever, a button, a pressure plate, a daylight detector and a block of redstone. Toward dust a
  row up in the cell beside it, standing on a block whose top is sturdy, a side is `up` when that
  block's face toward this dust is sturdy too and `side` when it is not; toward dust a row down in
  the cell beside it, a side is `side`. The game also lets dust climb onto a hopper or a trapdoor,
  whose tops are not sturdy, and that climb is not derived. A diagonal joins only through a cell
  that holds nothing: the cell above this dust for the one up, the cell beside it for the one down.
  A block standing there lets the dust past or cuts it off as the game decides by whether the block
  conducts, which no table records, so the diagonal is derived as if it were cut. A dust joined on
  neither side of one axis is then joined on both sides of the other, so a lone dust is a cross and
  one joined once runs straight through. `connect=none` writes all four `none`, a dot, and
  `connect=all` all four `side`, a cross, whatever stands beside it.

A state the block's material or line writes is kept, and only the others are derived:
`@glass_pane[north=true]` keeps `north=true` and joins its other sides as above. A raw block keeps
every state, written or not: its neighbours read it as they read any block, and none of its own is
derived ([Materials and Themes §7.4](/spec/materials-themes/#74-raw-blocks)). The states are
resolved state, never intent ([§6.2](#62-intent_state-and-resolved_state)).

The blocks beside are those of the same `struct` or `place` body, and nothing outside its array
counts. What each block is to these rules comes from the target's registry pack ([Versioning and
Editions §10.3](/spec/versioning-editions/#103-backend--data-tables)). A lowering with no version
pinned reads each thing the rules ask of a block, its kind, its sturdy faces, what its bottom covers
and the faces dust joins among them, as every version of the edition that declares it agrees, and as
nothing where they disagree, so a block they read as two kinds is nothing to the rules; without a
pack nothing is derived. A Bedrock target reads its block as the canonical blocks its state map
names that take the block's states, and its writer maps the derived states as it maps every other. A
block the pack cannot answer for is nothing to the rules: one no canonical block becomes, such as
Bedrock's `border_block`, or one the canonical blocks it may be disagree on. Bedrock keeps a chest's
pairing in its block entity rather than its states, and its writer pairs the two halves there
([Entities §8.1](/spec/entities/#81-two-tiers)). A version whose state map keeps
none of a block's sides, such as Bedrock's panes, fences and dust before 1.26.60, has the game work
them out from the blocks beside it: a side the lowering derived loses nothing there, and one a
material or line writes, `@glass_pane[north=true]` or a dust's `connect=`, is not kept, which the
build says with `W_INTENT_DEGRADED` too. The block array records, per palette entry, the states the
lowering derived at every cell that holds it, which is how the writer tells the two apart. No line
writes the blocks of a circuit laid under the struct ([Redstone
§14.5](/spec/redstone/#145-place-and-route)), so every state of an entry only the circuit holds is
derived.

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
`half`, `facing`, `shape` and `waterlogged`, on a `beam`, `axis`, on a `door`, `hinge` and `open`,
on a `chest`, `facing`, `size`, the line's spelling of its `type`, and `waterlogged`, on a
`trapdoor`, `facing`, `half`, `open` and `waterlogged`, on a `note_block`, `note` and `instrument`,
on an `observer`, `piston`, `dispenser`, `redstone_torch`, `sign` or `bed`, `facing`, on a
`repeater`, `facing` and
`delay`, on a `comparator`, `facing` and `mode`, and on a `redstone_dust`, `connect`. No other
keyword names one. Every other `key=value` the line writes is one of the member's arguments, apart
from `id`, `class` and `mat_slot`, which become fields of their own. Where a key goes depends on the
keyword: `shape=` on a `stair` is intent, and `shape=` on a `window` is an argument, the opening's
outline ([Entities §8.2](/spec/entities/#82-anchor-conventions)).

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

`bed` is treated as a block member rather than an entity, to keep the IR types clean, and so is
`sign`. What either block keeps in a block entity rides beside the block array's grid ([Entities
§8.1](/spec/entities/#81-two-tiers)).

## 6.3 `waterlogged`

**Subject to change.** Flowing water is not built yet: `water` is `E_UNKNOWN_KEYWORD`, and a `fill`
pours only source water ([Compatibility Tiers](/spec/compatibility/#subject-to-change)). In the
example below, `water` is `E_UNKNOWN_KEYWORD`.

The default is paint-derived: when `fill fluid=water` overlaps a waterloggable block, the compiler
sets `waterlogged`.

A `fill` names a box by two of its opposite corners, `from=` and `to=`, in either order and both in
the box. Each is a coordinate triple, a cell of the struct
([Syntax §5.1.1](/spec/syntax/#511-literals-and-separators)). With a `mat_slot=`, the fill paints
every cell of the box with that material in `massing`, and has no default block, so one whose
material does not resolve leaves its cells as they are. With `fluid=water`, it pours water once
the blocks it may hold are placed, in `fluids` ([Compilation Model
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
table. On a `stair`, a `chest` or a `trapdoor`, `waterlogged=true` and `waterlogged=false` are
written into the block's state and kept, and `waterlogged=auto`, like no `waterlogged=` at all, lets
a water fill set it. Any other word is `E_INVALID_MEMBER`. Flowing water is made explicit with
`flow=` and `level=`.

Like the overrides in [§6.1](#61-derive-by-default-promote-on-override), `waterlogged=` is written
on a member that places one block and names that block's state, and of the keywords the compiler
knows only `stair`, `chest` and `trapdoor` do so for a block that can hold water. The block of a
`door` or a `pressure_plate` holds none, and a `roof` or a `window` derives the state of every block
it lays and takes no override, so `waterlogged=` on any of the four is `E_UNKNOWN_ARGUMENT`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=inside.front offset=2 y=2 waterlogged=false  # an air window in a tank
water id=stream from=.. flow=east level=4
```
