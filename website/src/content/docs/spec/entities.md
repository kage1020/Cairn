---
title: "8. Entities"
---

## 8.1 Two tiers

**Subject to change.** Of the members below, `sign` and `bed` are built, and the others are not yet
([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

Opening up `nbt={}` for everything would destroy theming, editing, linting, and version tolerance.
So important entities are **structured**, which lets them ride the named-member editing model.

The promotion criterion: an entity with attributes you want to edit, or attributes that absorb
version differences, is structured. Only one-off special NBT escapes through `nbt={}`.

- **Structured, first-class**: `sign`, `painting`, `item_frame`, `armor_stand`, `villager` (plus
  `trade`), `display` (text / block / item), and `bed` (treated as a block).
- **Generic fallback**: `spawn id=.. type=<entity> at=<selector> [nbt={...}]` for other mobs.

Of these, `sign` and `bed` are keywords, set as [Blockstate Model
§6.1](/spec/blockstate/#61-derive-by-default-promote-on-override) says, and the rest, `spawn`
included, are not yet: each is `E_UNKNOWN_KEYWORD`, so no build places an entity. The example below
parses: a decimal (`scale=1.5`) and an `nbt={...}` compound are values
([Syntax §5.1.1](/spec/syntax/#511-literals-and-separators)), and each keyword in it other than
`sign` is `E_UNKNOWN_KEYWORD`.

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
@edition java
  spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
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
to be `E_NBT_OUTSIDE_EDITION_GUARD`, a reserved code. The build that keeps the line writes the
compound, as given, into the entity or block entity the line places: the compiler checks the
compound's syntax and nothing in it.

A villager trading hall is a staple build and display entities are core to modern decoration.
Sending them to `nbt={}` would cost generation quality and edit stability, so they are structured.

Block entities (signs) and true entities (paintings) are different things in NBT but share one
selector grammar in the DSL. Telling them apart is the compiler's job.

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
one.

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

**Subject to change.** `painting` and a window's `shape=` below are not built yet ([Compatibility
Tiers](/spec/compatibility/#subject-to-change)).

Paintings, item frames, arch windows, stairwells, and overhanging roofs have a declared size that
differs from the AABB they occupy. Left ambiguous, edit stability, theme swapping, and
cross-implementation compatibility all break.

Every primitive therefore carries four things in the IR: an `anchor` (reference point), a declared
bbox, an actual bbox, and a host face. Lowering derives the four for every member that builds a
cell, and records them beside its `resolved_state` ([Blockstate Model
§6.2](/spec/blockstate/#62-intent_state-and-resolved_state)):

- The **anchor** is the cell the member's line places it by.
- The **declared bbox** is the box around every cell the line names, before any other member
  paints.
- The **actual bbox** is the box around every cell the member holds once its body is painted. It is
  smaller than the declared bbox where another member took cells at its edge, and larger where the
  member builds past what its line names, as a roof's overhang does.
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
| `chest`, `trapdoor`, `sign`, sensor, actuator, Tier 0 part by a wall | its one cell | its one cell | the face of its wall it stands by |
| `bed` | its head | both halves | the face of its wall its head is against, none at a coordinate triple |
| `trapdoor`, `sign`, `note_block`, Tier 0 part at a coordinate triple, `raw_block` | its one cell | its one cell | none |
| `fill`, `raw_fill` | the low corner of its box | the box `from=` and `to=` span | none |
| `circuit` | the low corner of its box | every layer it reserves | none |

A window's rectangles are each one a `repeat=` series stamps and, with `sym=true`, the mirror, so a
mirror skipped for overlapping the window stays in the declared bbox and out of the actual one
([Syntax §5.4](/spec/syntax/#54-selectors)). A fixture by a wall stands against its outer face
from outside the wall (`at=front.outside`) and its inner face from inside (`at=inside.front`). A
`circuit` that lays no block holds the layers it reserves, which it writes as air.

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
the cell keeps its declared bbox, and its actual bbox is what it still holds. Two `place` rows
share no array: each body is its own structure, written with air in every cell it does not fill, so
placing one over the other erases what was there and there is nothing to merge. Their overlap is an
error ([Lint](/spec/lint/#semantic-categories)), which no command checks yet.

Neighbour-dependent blockstate (stairs, fences) breaks when overwritten without interference
detection: an inner-corner stair left as an outer corner, hanging in mid-air. Re-resolving boundary
blockstate is the IR layer's responsibility.

In the example below, `painting` is `E_UNKNOWN_KEYWORD` and the window's `shape=arch` is accepted
and read by no pass, so it is `W_IGNORED_ARGUMENT` ([Lint](/spec/lint/)). The window's
`anchor=bottom_center` puts its first column at 3, and the roof's `footprint=` and `bounds=` name
what it builds without them.

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 Where the line is drawn

Signs, paintings, item frames, and beds contribute to a build's architectural feel, so they are
adopted. Chest contents, villager inventory, and other information that does not contribute to
architectural precision is not structured. It goes to the generic `spawn` `nbt={}` or the escape
hatch.
