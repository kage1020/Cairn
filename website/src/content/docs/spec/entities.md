---
title: "8. Entities"
---

## 8.1 Two tiers

Opening up `nbt={}` for everything would destroy theming, editing, linting, and version tolerance.
So important entities are **structured**, which lets them ride the named-member editing model.

The promotion criterion: an entity with attributes you want to edit, or attributes that absorb
version differences, is structured. Only one-off special NBT escapes through `nbt={}`.

- **Structured, first-class**: `sign` and `bed`, which are blocks with a block entity, `painting`
  and `item_frame`, which hang on a face, `armor_stand` and `villager` (plus `trade`), which stand
  on a floor, and the displays `text_display`, `block_display` and `item_display`. A `hologram` is
  built as a text display or a sign, whichever its edition's theme binds ([Versioning and Editions
  §10.7](/spec/versioning-editions/#writing-an-alternative)).
- **Generic fallback**: `spawn type=<entity> at=... [nbt={...}]` for any other entity.

Each is a keyword. `sign` and `bed` are set as [Blockstate Model
§6.1](/spec/blockstate/#61-derive-by-default-promote-on-override) says, and the entities as the
sections below say. Every entity is placed by its line's `at=`, the way a fixture is: by a wall,
`at=<side>.outside` or `at=inside.<side>` with `offset=` and `y=`, or at a coordinate triple,
`at=x,y,z`. A line that places none, one whose `at=` is missing or of another form, is
`E_INVALID_MEMBER`, and an `at=` the wall or the struct cannot hold is refused or deferred as a
chest's is ([Blockstate Model §6.1](/spec/blockstate/#61-derive-by-default-promote-on-override)).
Each member places one entity, which a later member does not build over: an entity takes no cell
from a block, nor a block from it.

```
villager      id=trader at=inside.back offset=3 y=1 profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=2,2,2 mat_slot=lamp scale=0.5
item_display  id=trophy at=5,1,2 facing=east item=diamond_sword
@edition java
  spawn id=cat type=cat at=1,1,1 nbt={variant:"minecraft:black"}
```

A key that reads a decimal, such as `scale=`, is written as a float or a double, whichever the NBT
field it fills holds.

An `nbt={...}` compound is edition-specific by nature. Java and Bedrock name an entity's and a block
entity's fields apart, and no registry pack holds those fields, so nothing checks a compound against
the target the way a raw id is checked ([Materials and Themes
§7.4](/spec/materials-themes/#74-raw-blocks)). A line that writes `nbt=` therefore stands under the
`@edition` guard of the edition whose field names it spells, which holds it as it holds a raw member
([Versioning and Editions
§10.7](/spec/versioning-editions/#an-edition-guard-holds-the-escape-hatch)). Outside a guard it is
`E_NBT_OUTSIDE_EDITION_GUARD`, at the compound. The keywords that read one are the entities',
`painting`, `item_frame`, `armor_stand`, `villager`, the three displays and `spawn`, and
`raw_block`; on any other line `nbt=` is `E_UNKNOWN_ARGUMENT`. The build that keeps the line puts
the compound over what it writes for the entity, key by key at the top level: a key the compound
names replaces the writer's own, and every other key is added. A `raw_block`'s compound is the
block entity of its block, written as given. The compiler checks the compound's syntax and nothing
in it; a writer refuses a compound whose text holds a character outside ASCII, which the NBT it
writes does not carry, and names the entity's cell.

A villager trading hall is a staple build and display entities are core to modern decoration.
Sending them to `nbt={}` would cost generation quality and edit stability, so they are structured.

Block entities (signs) and true entities (paintings) are different things in NBT but share one
selector grammar in the DSL. Telling them apart is the compiler's job: a block entity is kept beside
its block, and an entity is written into the structure's list of entities. Java writes each as an
entry of its `.nbt`'s `entities`: `pos`, the entity's point as three doubles, `blockPos`, its cell
as three ints, and `nbt`, which holds its `id` and the fields the sections below give. Bedrock
writes each in its `.mcstructure`'s `entities` with its `identifier`, its point as `Pos`, three
floats, `Rotation`, a still `Motion`, a `UniqueID` that is its place in the list counted from 1, and
`definitions` naming the entity, except an item frame, which Bedrock builds as a block. Each point
and cell is in the structure's coordinates, counted from its low corner, so the game moves them with
the structure as it places it. The list runs in the order of the entities' cells, x fastest, then z,
then y, and two in one cell in the order their lines are written.

No registry pack holds items, enchantments or entity types. An item's id, an enchantment's and a
`spawn`'s `type=` are written as given, each a word the build namespaces as `minecraft:` or a
namespaced id, and the game drops what it does not have when it places the structure.

### Hanging entities

A `painting` and an `item_frame` hang on the face of the block behind them. By a wall, an entity
takes the cell a chest with the same `at=`, `offset=` and `y=` would take, and hangs on the wall's
face, looking away from it, so nothing is left for a `facing=` to choose, and one on its line is
`E_INVALID_MEMBER`. At a coordinate triple it takes the cell a Tier 0 part would ([Redstone
§14.1](/spec/redstone/#tier-0-parts)), and its `facing=` MUST name the way it looks: it hangs on
the block on the other side of the cell, behind it. A painting looks `north`, `south`, `east` or
`west`, and an item frame also `up`, lying on the block under it, or `down`, under the block above
it. A missing `facing=` at a triple and any other identifier for it are `E_INVALID_MEMBER`, and a
value that is no identifier is `W_IGNORED_ARGUMENT`, read as if the key were not written. An
entity's point is its cell's middle moved 15/32 of a block toward the face it hangs on, where the
game keeps a hanging entity 1/16 of a block thick, and its cell is the one the game finds it by.
What the game requires behind it is the constraint checks' ([Lint §11.4](/spec/lint/#entity-aabb)).

```
painting   id=hall_art at=inside.front offset=2 y=1 variant=kebab
painting   id=mural    at=3,2,1 facing=south variant=pointer
item_frame id=relic    at=inside.back offset=4 y=2 item=diamond glowing=true
item_frame id=floor    at=2,1,2 facing=up
```

A `painting` MUST name its painting with `variant=`, the name the target's `paintings` table gives
it ([Versioning and Editions §10.3](/spec/versioning-editions/#103-backend--data-tables)), such as
`kebab`. A painting covers its variant's `width` by `height` cells: its line's cell is its
bottom-left one, seen from in front of it, and it runs right and up from there. The game keeps a
painting by one of those cells, `(width - 1) / 2` right of the bottom-left and `(height - 1) / 2`
above it, rounded down, so that cell is its cell, and its point is that cell's. A painting whose
cells leave the body's block array is not hung, `W_DEFERRED_MEMBER`. A `variant=` the pinned
target has no painting of is `E_INVALID_MEMBER`, with the paintings it has, or, where other
versions have it, those versions. With an edition and no version pinned, a variant some version
has hangs at the size every version that has it agrees on; with no pack, the variant is not
checked and the painting covers its one cell. A missing `variant=` and one that is no identifier
are `E_INVALID_MEMBER`.

An `item_frame` holds the item its `item=` names, one of it, and with none it hangs empty.
`glowing=true` makes it a glow item frame. An `item=` that is no id and a `glowing=` that is no
`true` or `false` are `W_IGNORED_ARGUMENT`.

Java writes a painting's `variant`, its namespaced id, and its `facing` as a byte, south 0, west 1,
north 2 and east 3. It writes an item frame, `item_frame` or `glow_item_frame`, with its `Facing` as
a byte, down 0, up 1, north 2, south 3, west 4 and east 5, and its `Item`. Bedrock writes a painting
as `minecraft:painting` with its `Motive`, the spelling the target's `paintings` table gives, and
its `Direction`, numbered as Java's `facing`. Bedrock builds an item frame as a block, `frame` or
`glow_frame`, whose `facing_direction` is numbered as Java's `Facing`, with `item_frame_map_bit`
and `item_frame_photo_bit` off. Its block entity, `ItemFrame` or `GlowItemFrame`, holds its `Item`,
an `ItemRotation` of 0 and an `ItemDropChance` of 1, and takes the line's `nbt=`. The block takes
the frame's cell, so the writer refuses a frame in a cell a member has built a block in, and names
the cell and the block.

### Standing entities

An `armor_stand`, a `villager` and a `spawn` stand on their cell's floor, at the middle of it. By a
wall, an entity takes the cell a chest would and looks away from the wall, so a `facing=` on its
line is `E_INVALID_MEMBER`. At a coordinate triple it takes the cell a Tier 0 part would, and looks
the way its `facing=` names, `north`, `south`, `east` or `west`, or `south` without one. Any other
identifier for `facing=` is `E_INVALID_MEMBER`, and a value that is no identifier is
`W_IGNORED_ARGUMENT`. Each writer turns the entity by its `Rotation`, a yaw of 0 looking south, 90
west, 180 north and 270 east.

An `armor_stand` takes no key of its own, and is written without arms.

A `villager` takes a `profession=`, one of `none`, `armorer`, `butcher`, `cartographer`, `cleric`,
`farmer`, `fisherman`, `fletcher`, `leatherworker`, `librarian`, `mason`, `nitwit`, `shepherd`,
`toolsmith` and `weaponsmith`, and a `level=`, one of `novice`, `apprentice`, `journeyman`, `expert`
and `master`. Without them it is a `none` villager, a `novice`. Any other value of either is
`E_INVALID_MEMBER`, and no villager is placed. A villager is from the plains. Its experience is
the least its level holds, 0, 10, 70, 150 and 250 from `novice` up, and 1 for a `novice`, since a
villager with none loses its profession to the first job site it finds.

A `trade` line is one offer of the villager whose `id=` its `villager=` names, in the same body; its
villager offers its trades in the order their lines are written. It buys `count=` of the item its
`buy=` names and sells `sell_count=` of the item its `sell=` names, each a whole number from 1 to
64 and 1 without one. `enchant=` puts an enchantment on the item it sells, at `enchant_level=`, a
whole number from 1 to 255 and 1 without one: an enchanted book stores it, and any other item
carries it. A `villager=` that names no villager of the body, and a missing or malformed `buy=`
or `sell=`, are `E_INVALID_MEMBER`, and the trade is not offered. A count or a level out of range or
of another kind, an `enchant=` that is no id, and an `enchant_level=` without an `enchant=`, are
`W_IGNORED_ARGUMENT`. A trade sells 12 times before its villager restocks, and its price moves 0.05
with demand. A `trade` places nothing of its own.

```
villager id=smith at=inside.left offset=2 y=1 profession=weaponsmith level=expert
trade villager=smith buy=emerald count=12 sell=iron_sword enchant=sharpness enchant_level=3
armor_stand id=stand at=4,1,2 facing=west
```

A `spawn` places the entity its `type=` names, which it MUST write, with what its `nbt=` gives.
`spawn` is the escape hatch for the entities no keyword structures, so an `@edition` guard holds it
whatever its line writes ([Versioning and Editions
§10.7](/spec/versioning-editions/#an-edition-guard-holds-the-escape-hatch)).

Java writes a villager's `VillagerData`, its `type`, its namespaced `profession` and its `level`
from 1, its `Xp`, and its trades under `Offers` as `Recipes`, each with its `buy`, its `sell`, a
`maxUses` of 12, `rewardExp`, an `xp` of 1 and a `priceMultiplier` of 0.05. Bedrock writes a
villager as `minecraft:villager_v2`, its profession as the `Variant` the game numbers it by and as
its `PreferredProfession`, its level as a `TradeTier` from 0 with the `TradeExperience` Java's `Xp`
gives, its trades under `Offers` as `Recipes`, each with its `buyA`, `buyCountA`, `sell`, `tier`
and `maxUses`, and its profession in its `definitions` beside the entity's own.

Each writer saves an item as an item stack in the shape the target reads. Java before 1.20.5 saves
a `Count` byte and the enchantment in its `tag`, under `Enchantments`, or `StoredEnchantments` for a
book, as an `id` and a `lvl`; from 1.20.5 a `count` and data components, `minecraft:enchantments`
or `minecraft:stored_enchantments`, which map each enchantment's id to its level, under a `levels`
map before 1.21.5. Bedrock saves an item's `Name`, `Count`, `Damage` and `WasPickedUp`, and its
enchantment under `tag` as `ench`, by the number Bedrock gives it, so the writer refuses an
enchantment it has no number for, and names the ones it has.

### Displays

A `text_display`, a `block_display` and an `item_display` draw text, a block or an item where they
stand, and stand as an armor stand does: on their cell's floor, looking the way their wall or their
`facing=` says. Each takes a `scale=`, a number above 0 and 1 without one, which scales what it
draws along each axis; any other value is `W_IGNORED_ARGUMENT`.

- A `text_display` shows its `text=`, a string, one line, or a list of strings, its lines from the
  top, and turns to the viewer as its `billboard=` says: `fixed`, which it does without one, never;
  `vertical` about the vertical axis; `horizontal` about the horizontal one; and `center` about
  both. A value of another kind for `text=` and any other `billboard=` are `W_IGNORED_ARGUMENT`.
- A `block_display` shows the block its `mat_slot=` binds, as a `floor` takes its block from its
  slot.
- An `item_display` shows the item its `item=` names, which it MUST write: a missing `item=` and
  one that is no id are `E_INVALID_MEMBER`, and no display is placed.

Java writes each display's `Rotation`, its `billboard`, and a `transformation` that turns nothing,
moves nothing and scales by `scale=`. A text display's `text` is its lines joined by line breaks,
a JSON string before 1.21.5 and a plain string from it, as each release reads one; a block
display's `block_state` is its block, as the structure's palette writes it; and an item display's
`item` is its item stack.

Bedrock has no display entity. At a Bedrock target each display is `E_PARITY_UNSUPPORTED`, with the
notes [Versioning and Editions](/spec/versioning-editions/#writing-an-alternative) gives for an
alternative, and the Bedrock writer refuses one that reaches it. Floating text that both editions
build is a `hologram`.

### Block entities

A build writes a block entity for each block that keeps there something the build sets: a sign's
text, a bed's colour, and the strength a circuit comparator outputs ([Redstone
§14.5](/spec/redstone/#145-place-and-route)), which only a Java circuit lays. The block array holds
each beside its block's cell, in no edition's shape, and each writer saves it in its own: Java as
the `nbt` of the block's entry in the `blocks` of its `.nbt`, and Bedrock under the
`block_position_data` of its `.mcstructure`, keyed by the block's place in `block_indices`. A block
entity is kept only while the member that set it holds its block's cell, so a member that builds
over a sign later takes the sign's text with it. Every other block that has a block entity, such as
the one a `chest`, a `dispenser` or a `daylight` sets ([Blockstate Model
§6.1](/spec/blockstate/#61-derive-by-default-promote-on-override), [Redstone
§14.2](/spec/redstone/#142-signal-binding)), is placed with no data, and the game gives it an empty
one, unless a `raw_block` that places it writes its `nbt=`.

```
sign id=menu at=front.outside offset=1 y=2 text=["The Inn", "rooms"] glowing=true
sign id=shut at=2,1,2 facing=north back="Closed"
bed  id=cot  at=inside.back offset=1 y=1 mat_slot=quilt
```

A sign's `text=` writes its front, the face toward the way the sign faces, and `back=` its back.
Each is a string, one line, or a list of up to four strings, its lines from the top. A face its line
does not write is blank, and `glowing=true` lights the text of both. A face of more than four lines
is `E_INVALID_MEMBER`, and no sign is built; a value of another kind, an item of a list included, is
`W_IGNORED_ARGUMENT`, which leaves that face blank or its text unlit. Text is black. Java saves each
face as four messages with `has_glowing_text`, each message a JSON string before 1.21.5 and a plain
string from it, as each release reads one. Bedrock saves each face as one `Text`, its lines joined
by line breaks, with `IgnoreLighting` for glowing. A writer refuses a sign whose text holds a
character outside ASCII, which the NBT it writes does not carry, and names the sign's cell.

Java names a bed's colour in its block, as `blue_bed`, and keeps nothing for it in a block entity.
Bedrock has one bed block, `bed`, and keeps the colour in the block entity of each half, as the
number the game gives the dye. Bedrock's `@bed.default` and `@bed.<colour>` tokens resolve to `bed`
and record the Java bed each stands for ([Versioning and Editions
§10.3](/spec/versioning-editions/#103-backend--data-tables)), whose colour the writer saves. A
Bedrock bed no such token names is saved white, as the game saves one that names no colour.

A Bedrock writer also fills the block entity of a block whose Java states Bedrock keeps there
rather than in the block's own. Each half of a chest pair names its other half's cell, in the
structure's coordinates as its own position is, and the left half leads the pair; a half whose other
half, the same block facing the same way, does not stand beside it stands single. A note block
keeps its `note` there, where it is not 0.

## 8.2 Anchor conventions

**Subject to change.** A window's `shape=` below is not built yet ([Compatibility
Tiers](/spec/compatibility/#subject-to-change)).

Paintings, item frames, arch windows, stairwells, and overhanging roofs have a declared size that
differs from the AABB they occupy. Left ambiguous, edit stability, theme swapping, and
cross-implementation compatibility all break.

Every primitive therefore carries four things in the IR: an `anchor` (reference point), a declared
bbox, an actual bbox, and a host face. Lowering derives the four for every member that builds a
cell or places an entity, and records them beside its `resolved_state` ([Blockstate Model
§6.2](/spec/blockstate/#62-intent_state-and-resolved_state)):

- The **anchor** is the cell the member's line places it by.
- The **declared bbox** is the box around every cell the line names, before any other member
  paints.
- The **actual bbox** is the box around every cell the member holds once its body is painted. It is
  smaller than the declared bbox where another member took cells at its edge, and larger where the
  member builds past what its line names, as a roof's overhang does. An entity takes no cell from a
  block, so its actual bbox is its declared one.
- The **host face** is the face of the wall or wall top the member is set against, named for the way
  it looks: `north` is `-z`, `south` is `+z`, and `up` is the wall top's. A member set against no
  face has none.

Each is a cell, or a box of cells with both corners included, of the body's block array, counted
from its low corner. An overhang moves them as it moves the member, and a `circuit` laid under the
base plane raises them with the struct ([Redstone §14.5](/spec/redstone/#145-place-and-route)).

| Member | Anchor | Declared bbox | Host face |
|---|---|---|---|
| `floor` | the low corner of its box | the `size=` footprint on row 0 | none |
| `walls` | the low corner of its box | the footprint over every row the walls fill | none |
| `roof` | the low corner of its box | its `footprint=` from the row above the walls to its apex | `up` |
| `door` | its lower half | both halves | the outer face of its wall |
| `window` | the cell `offset=` and `y=` place | every rectangle its line places | the outer face of its wall |
| eave `stair` | the band's cell at the wall's left end | the band | the outer face of its wall |
| `beam` | the run's cell at the wall's left end | the run | the outer face of its wall |
| `chest`, `trapdoor`, `sign`, sensor, actuator, Tier 0 part, standing entity or display by a wall | its one cell | its one cell | the face of its wall it stands by |
| `bed` | its head | both halves | the face of its wall its head is against, none at a coordinate triple |
| `trapdoor`, `sign`, `note_block`, Tier 0 part, standing entity or display at a coordinate triple, `raw_block` | its one cell | its one cell | none |
| `item_frame` | its one cell | its one cell | the face it hangs on |
| `painting` | its bottom-left cell | every cell it covers | the face it hangs on |
| `fill`, `raw_fill` | the low corner of its box | the box `from=` and `to=` span | none |
| `circuit` | the low corner of its box | every layer it reserves | none |

A window's rectangles are each one a `repeat=` series stamps and, with `sym=true`, the mirror, so a
mirror skipped for overlapping the window stays in the declared bbox and out of the actual one
([Syntax §5.4](/spec/syntax/#54-selectors)). A fixture by a wall stands against its outer face
from outside the wall (`at=front.outside`) and its inner face from inside (`at=inside.front`). A
`circuit` that lays no block holds the layers it reserves, which it writes as air. A painting's
cells run right from its anchor as seen from in front of it, so one inside the front wall runs
toward `-x` and one outside it toward `+x` ([§8.1](#hanging-entities)). A `trade` places nothing,
so it has none of the four.

**`anchor=` on a window** names the cell of the window that `offset=` and `y=` place, seen from
outside the wall: one of `bottom_left`, `bottom_center`, `bottom_right`, `middle_left`, `center`,
`middle_right`, `top_left`, `top_center` and `top_right`. Without it they place `bottom_left`, the
window's first column and lowest row ([Syntax §5.4](/spec/syntax/#54-selectors)).

**`footprint=` and `bounds=` on a roof** name the cells the roof covers and what its overhang does
to its body. Each takes one value: `footprint=struct`, the struct's `size=` footprint, and
`bounds=expand`, which widens the body's block array past the footprint by `overhang=` on every side
([Compilation Model §4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)). Writing
either builds what the line builds without it. Any other value is `E_INVALID_MEMBER`, and the roof
draws nothing, while the struct as written still rises as its shape does
([Lint](/spec/lint/#lowering)).

**Overlapping AABBs** resolve by priority merge inside one body, and are an error between bodies.
The members of a body paint one block array, so a cell two of them name goes to the later phase,
and inside one phase to the line written later, which `W_PHASE_CONFLICT` reports ([Compilation
Model §4.8](/spec/compilation/#48-within-phase-conflicts-and-the-palette)). The member that loses
the cell keeps its declared bbox, and its actual bbox is what it still holds. An entity is not
merged: what its box reaches into, a block, a door or another entity, is the constraint checks'
`W_ENTITY_CLIPPING` ([Lint §11.4](/spec/lint/#entity-aabb)). Two `place` rows share no array: each
body is its own structure, written with air in every cell it does not fill, so placing one over the
other erases what was there and there is nothing to merge. Their overlap is an error
([Lint](/spec/lint/#semantic-categories)), which no command checks yet.

Neighbour-dependent blockstate (stairs, fences) breaks when overwritten without interference
detection: an inner-corner stair left as an outer corner, hanging in mid-air. Re-resolving boundary
blockstate is the IR layer's responsibility.

In the example below, the window's `shape=arch` is accepted and read by no pass, so it is
`W_IGNORED_ARGUMENT` ([Lint](/spec/lint/)). The painting's bottom-left cell is the one `offset=2`
and `y=2` place inside the front wall, the window's `anchor=bottom_center` puts its first column at
3, and the roof's `footprint=` and `bounds=` name what it builds without them.

```
painting id=hall_art at=inside.front offset=2 y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 Where the line is drawn

Signs, paintings, item frames, and beds contribute to a build's architectural feel, so they are
adopted. Chest contents, villager inventory, and other information that does not contribute to
architectural precision is not structured. It goes to the generic `spawn` `nbt={}` or the escape
hatch.
