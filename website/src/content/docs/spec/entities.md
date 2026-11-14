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
also uses syntax the parser does not accept yet: a decimal (`scale=1.5`) and an `nbt={...}` value
are `E_PARSE` ([Syntax §5.1.1](/spec/syntax/#511-literals-and-separators)).

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

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

**Subject to change.** `painting` and the anchor keys below (`anchor=`, `shape=`, `footprint=`,
`bounds=`) are not built yet ([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

Paintings, item frames, arch windows, stairwells, and overhanging roofs have a declared size that
differs from the AABB they occupy. Left ambiguous, edit stability, theme swapping, and
cross-implementation compatibility all break. This is the top open issue in this chapter.

Every primitive therefore carries four things in the IR: an `anchor` (reference point), a declared
bbox, an actual bbox, and a host face.

The IR carries none of the four yet. In the example below, `painting` is `E_UNKNOWN_KEYWORD`, and
the window's `anchor=` and `shape=` and the roof's `footprint=` and `bounds=` are accepted but read
by no pass, so each is `W_IGNORED_ARGUMENT` ([Lint](/spec/lint/)).

Overlapping AABBs resolve by a rule the spec fixes, either priority merge or a lint error
([Lint](/spec/lint/)). Neighbour-dependent blockstate (stairs, fences) breaks when overwritten without
interference detection: an inner-corner stair left as an outer corner, hanging in mid-air.
Re-resolving boundary blockstate is the IR layer's responsibility.

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
