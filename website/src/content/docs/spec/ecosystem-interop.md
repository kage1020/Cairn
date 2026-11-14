---
title: "12. Ecosystem Interop and Reverse Conversion"
---

## 12.1 Forward direction

Serializing the block-array IR emits `.nbt`, `.litematic`, `.schem`, and `.mcstructure`
([Architecture](/spec/architecture/)). Each format is a serializer, and an existing format is one
more backend around the pivot. All four serializers exist today.

`cairn compile --edition bedrock` writes `.mcstructure`. `--edition java` writes `.nbt`, or the
format `--structure-format` names:

| `--structure-format` | Writes | A `site` becomes |
|---|---|---|
| `nbt` (the default) | a vanilla structure file per struct, placement and walkway | a file per placement and per walkway ([§9.3.4](/spec/components-editing-sites/#934-output-naming)) |
| `litematic` | a Litematica schematic per struct and per site ([§12.1.1](#1211-litematica)) | one file, with a region per placement and per walkway |
| `schem` | a Sponge Schematic per struct, placement and walkway ([§12.1.2](#1212-sponge-schematic)) | a file per placement and per walkway, each offset by its origin in the site |

The format changes the files and nothing else. The block array is the same, an artifact is named by
the same rule and refused on the same collisions, and the lockfile is the same, byte for byte,
whichever format is written. Bedrock has one structure format, so `--structure-format` with
`--edition bedrock` is a usage error, exit 2, refused before the source is read.

### 12.1.1 Litematica

A `.litematic` is the gzip-wrapped Java NBT that Litematica saves and loads. A struct is a file of
its own, named after the struct (`cottage.litematic`), with one region at the origin. A site is one
file named after the site (`hamlet.litematic`), with a region per placement and per walkway, each at
the origin the solver gave it, which the lockfile records. A region is named after the `.nbt` file
its array would be (`home1`, `hamlet_walkway_home1_entry__home2_entry`). Each region reads back as a
struct of its own, its name and corner kept as provenance
([§12.4](#124-import-stamping-and-pitfalls)). A site and a struct that share a name would write one
file, and the compile refuses the pair before writing anything, as it refuses two `.nbt` files of
one name.

The root holds:

- `MinecraftDataVersion`, the target's DataVersion, and `Version`, the schematic version a
  Litematica for that release reads: 6 below 1.20.5's DataVersion (3837), and 7 from it.
  `SubVersion` is 1.
- `Metadata`: `Name`, the file's stem; `Author`, `Cairn`, since a source names no author; an empty
  `Description`; `RegionCount`; `TotalVolume`, the regions' voxels; `TotalBlocks`, those that are
  not air, cave air or void air; and `EnclosingSize`, the box around every region. The times the
  file was created and modified and its preview image are left out, so a source always compiles to
  the same bytes.
- `Regions`, each with `Position`, its minimum corner, and `Size`, positive on every axis.
  `BlockStatePalette` lists the region's block states with air first, under `Name` and `Properties`
  at every DataVersion, since Litematica keeps those keys where structure files turn to `id` and
  `properties`. `BlockStates` packs each voxel's palette index, in `(y, z, x)` order, into as many
  bits as the largest index needs, and at least 2, from the low end of each long. An index that
  does not fit what is left of one long runs on into the next. `TileEntities` holds each block
  entity as the `.nbt` writer writes its `nbt`, with its `x`, `y` and `z` from the region's corner.
  `Entities`, `PendingBlockTicks` and `PendingFluidTicks` are empty.

### 12.1.2 Sponge Schematic

A `.schem` is the gzip-wrapped Java NBT of the Sponge Schematic Specification, which WorldEdit saves
and loads. Cairn writes version 3. Each struct, placement and walkway is a file of its own, named as
its `.nbt` file is (`cottage.schem`, `home1.schem`), so a site writes the files it writes as `.nbt`.
A file's `Offset` is where its array sits in its site, the origin the lockfile records, and
`[0, 0, 0]` for a struct: pasting every file of a site at one point rebuilds the site.

Version 3 is written rather than version 2 because:

- WorldEdit has read and written version 3 since 7.3.0, and a WorldEdit release from 7.3.0 on runs
  on every Java target Cairn ships, from 1.20.4.
- A version 3 block entity keeps its fields in a `Data` compound beside its `Id` and `Pos`, as the
  block array keeps a block entity's id apart from its fields. Version 2 lays the fields out beside
  `Id` and `Pos`, where a field of either name collides with them.
- Version 3 nests the schematic in an unnamed root, as Minecraft's own data files do.

A file from a tool that writes only version 2 still reads, since the `.schem` reader takes either
version.

The root holds one `Schematic` compound:

- `Version` 3, and `DataVersion`, the target's.
- `Metadata`: `Name`, the file's stem, and `Author`, `Cairn`. The date the file was saved is left
  out, so a source always compiles to the same bytes.
- `Width`, `Height` and `Length`, the array's extents along x, y and z. Each is stored as an
  unsigned short, so an array past 65535 on an axis is refused.
- `Offset`, as above.
- `Blocks`. `Palette` maps each block state, spelled `namespace:id[name=value,...]` with its
  properties sorted by name, to its index in the block array's palette, air first. A state whose
  spelling would read back as another, such as one with a comma in a property value, is refused.
  `Data` holds each voxel's index as a varint, in `(y, z, x)` order. `BlockEntities` holds each
  block entity as its `Pos`, its `Id`, and a `Data` holding the fields the `.nbt` writer writes in
  its `nbt`.

Biomes and entities are not written.

## 12.2 Reverse direction: the compiler transliterates, an LLM lifts

The compiler does not look at voxels and decide "this is a wall": that kind of computer vision
becomes unmaintainable. What it implements is robust, faithful transliteration, verification, and a
voxel diff. Lifting the result to meaning is an LLM's job, done as a refactor of the raw-centric
DSL. That dogfoods the language and matches P5's self-correction loop.

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block centric
(an LLM refactors house.raw.crn into a semantic DSL) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn             # voxel XOR → into self-correction
```

The first step is built. `cairn import FILE --mode raw` reads a `.nbt`, `.litematic`, `.schem` or
`.mcstructure` file and writes `<stem>.raw.crn` beside it, or the path `--out` names, replacing a
file already there. The extension, in any case, names the format, and the format the edition:
`.mcstructure` is Bedrock's and the other three are Java's. An extension that names none of them is
a usage error, exit 2, as are a file that is not there and an `--out` that names the file itself.
`raw` is the one mode, since L1 is the compiler's ceiling
([§12.3](#123-three-tiers-of-transliteration)) and the lift is a refactor of what it writes.

The source is checked before it is written. It is compiled against the target it is stamped with
([§12.4](#124-import-stamping-and-pitfalls)) and compared with the file cell by cell, so
`cairn compile` at that target builds the blocks the file holds; the note `cairn import` prints last
names the `--edition` and `--target` that do. Two blocks compare as the target builds them: a state
the target builds the same at any value is not compared, and on Bedrock a block compares as the
canonical block its state map reads it as. A file whose blocks the stamped target does not declare
is refused with the errors that compile raises, each with the line it would have written, and a note
that saving the file in that release renames a block an older one spelled otherwise. A cell no raw
member builds as the file holds it refuses the import rather than writing it wrong. Either refusal,
or bytes the format's reader refuses, is exit 1, and nothing is written.

The last step closes the loop. `cairn diff-blocks` builds the lift at the target the structure file
is stamped with and compares the two cell by cell ([§12.2.1](#1221-diffing-a-lift)), names each
region of cells that differ with its likely cause ([§12.2.2](#1222-regions-and-their-causes)), and
suggests `edit` lines that bring the build nearer the file
([§12.2.3](#1223-the-suggested-patch)). The refactor between the import and the diff is the LLM's,
and the loop runs until the lift converges:

```text
house.litematic against house.lifted.crn at java 1.21.4, the shipped target the file was saved at
block IoU       0.962  converges at >= 0.985
state_accuracy  1.000  converges at >= 0.995
residual raw    0%     converges at <= 5%
1 region differs:
  missing house (12,4,3)..(18,6,3): 12 cells of minecraft:glass_pane → likely `window[id=front_windows]` `repeat=3` too small
suggested patch, reaching block IoU 1.000, state_accuracy 1.000, residual raw 0%:
  edit window[id=front_windows] set repeat=4
error[E_DECOMPILE_DIFF]: block IoU = 0.962 < threshold 0.985
```

A lift converges at block IoU ≥ 0.985, `state_accuracy` ≥ 0.995, and residual raw ≤ 5%. An exact
match is not required, and the residue stays explicit as `raw_fill id=residual_* origin=imported`.

### 12.2.1 Diffing a lift

`cairn diff-blocks STRUCTURE FILE [--format text|json]` reads STRUCTURE as `cairn import` reads it:
its extension names its format and edition, and it is stamped by the rules of
[§12.4](#124-import-stamping-and-pitfalls). FILE is then built at that stamp, with every pass
`cairn compile` runs at it, and the blocks it builds are compared with the blocks STRUCTURE holds.
Building at the stamp keeps the comparison within one edition and one version.

Each side is a set of bodies. STRUCTURE's are the bodies an import writes: one for each Litematica
region, else one for the file, its structure voids read as air. FILE's are its structs and its sites,
each site laid as one body: its placements and walkways at their positions in the site, a later
array's block over an earlier one's and its air over nothing, moved so its lowest corner is
`(0, 0, 0)`. When each side holds one body, the two are compared whatever their names. Otherwise a
body pairs with the body of the other side that has its name, and a body with none is compared with
nothing. Two paired bodies are compared cell by cell from their `(0, 0, 0)` cells, over the box that
holds both.

A cell holds a block unless it lies outside its body or holds `minecraft:air`, `minecraft:cave_air`
or `minecraft:void_air`. Two blocks match when their ids are the same, and their states agree when
every property agrees as the stamped target builds it:

- A property both write agrees when the two values are the same.
- A property one side leaves out is read at the default the target builds for it: on Bedrock the
  default its state map gives, and `waterlogged` as `false` on either edition, since lowering leaves
  it out where no water stands.
- A property STRUCTURE writes and the build leaves out, with no such default, is not compared. A
  game-saved file writes every property of a block, and a source writes only the ones it sets.
- A property the build writes and STRUCTURE leaves out, with no such default, differs.
- A property the target builds the same at any value is not compared.

Over every pair, the diff counts the cells STRUCTURE holds a block at, the cells FILE builds a block
at, the cells both hold a block at, the cells where both hold the same id, which are *matched*, and
the matched cells whose states agree. A built cell is *residual* when the member that owns it once
the body is built is a `raw_block` or `raw_fill`, a `raw_repeat`'s copies included, whether or not
its line is marked `origin=imported`. From those counts:

- **block_iou.** The block IoU: matched cells over the cells either side holds a block at. A cell
  where the two hold different ids is in the union and not in the intersection. It is 1 when
  neither side holds a block.
- **state_accuracy.** Matched cells whose states agree over every matched cell, so a cell of two
  different ids counts against the block IoU and not here. It is 1 when no cell is matched.
- **residual_ratio.** Residual raw: residual cells over the cells FILE builds a block at. It is 0
  when FILE builds none.

A lift **converges** when all three meet their thresholds, each compared unrounded. Short of that,
the diff reports `E_DECOMPILE_DIFF` ([Lint §11.1](/spec/lint/#import)).

The metrics are defined on any two sets of bodies, and the library computes them for any caller,
such as an evaluation harness
([Evaluation §13.2](/spec/evaluation/#132-reverse-conversion-auxiliary-metrics)). Between two editions or two versions nothing is respelled: ids are compared as each side writes
them, so a block one version renamed, or one the other edition spells otherwise, is replaced, and a
property one side leaves out is read at the defaults of STRUCTURE's target. `cairn diff-blocks` does
not meet that case, since it builds FILE at STRUCTURE's stamp.

### 12.2.2 Regions and their causes

A cell that differs is *missing* when STRUCTURE holds a block there and the build none, *extra* when
the build holds one and STRUCTURE none, *replaced* when the two hold different ids, and *state* when
they hold one id whose states do not agree. Within a pair, the cells of one kind with the same ids,
STRUCTURE's and the build's, are one region when each stands within two cells of another on every
axis, so one cell of anything else between them does not split it.

A region reports its kind, its body (the build's name, else STRUCTURE's), the box from its lowest
corner to its highest, how many cells in it differ, its block (STRUCTURE's, or the build's for an
extra region), the build's block for a replaced region, and each property that differs, with both
values, for a state region. Regions are listed most cells first, then by pair, by kind in the order
above, by lowest corner (`y`, then `z`, then `x`) and by block.

Its cause is read from the members that own its cells once the build is laid, by the member that
owns the most of them, the one written first of those that own as many:

- **built_by.** An extra, replaced or state region names the member that built it.
- **cleared_by.** A missing region whose cells a member owns names that member, which left them
  air.
- **short_of.** A missing region no member owns names the member nearest it, within two cells, of
  those that build its block: a wall a row short, a fill a column narrow.
- **walkway.** A walkway of the site laid the region's block.
- **no_member.** No member builds the block there, or none within two cells of a missing region.
- **unpaired.** The region's body has no body on the other side to pair with.

A member is named by its keyword, its `id=` when its line writes one, its address in the body it is
written in ([Components, Editing, and Multi-building
§9.2](/spec/components-editing-sites/#92-editing-model)), and its line. A member of a `def` is named
in the `def`, wherever a site places it.

### 12.2.3 The suggested patch

The patch is up to three `edit` lines ([§9.2](/spec/components-editing-sites/#92-editing-model))
that bring the build nearer STRUCTURE when appended to FILE. Each is found by building with it, not
by inference alone:

1. The regions are taken in the order they are listed. A region whose cause names a member of FILE
   that no earlier region named is tried, and any other region is passed over.
2. The member's candidates are tried in this order: for a raw member's replaced or state region,
   its `mat=` set to the block most of the region's cells hold in STRUCTURE; for a state region,
   each state argument its keyword takes set to STRUCTURE's value; for a replaced region, its
   `mat_slot=` set to each other slot whose members build the region's block; each whole number
   its line writes, one more, one less, two more and two less, none below 0; `repeat=2` and
   `repeat=3` when its keyword takes `repeat=` and its line leaves it out; and its `from=` and `to=`
   grown over a missing region beside it, or cut back from an extra one that spans the box on two
   axes and lies at one end of the third.
3. Each candidate is appended as a line of its own to FILE, after the lines already kept, and the
   text is built at the stamp with every pass `cairn compile` runs. A candidate that does not parse,
   raises an error or drops a scope is discarded, and the build of every other is compared with
   STRUCTURE.
4. The candidate that leaves the fewest cells differing is kept, when that is fewer than the lines
   kept so far leave; of two that leave as many, the earlier. The cells counted are those only one
   side holds a block at, those where the two hold different ids, and the matched cells whose states
   disagree.
5. The search stops once three lines are kept, 48 candidates have been built, or no cell differs.

A kept line is reported with the region it was found for, and that region's cause then reads as the
change: `` `walls[0]` `height=3` too small `` for one whole number, `` `walls[0]` without
`repeat=2` `` for a key its line leaves out, and `` `raw_fill[0]` `mat=minecraft:granite` where
`mat=minecraft:stone` builds it `` otherwise. The patch lists its lines in order, with the metrics
FILE reaches with all of them appended. When no candidate helps, there is no patch.

### 12.2.4 The report

FILE is built first, and its findings print on stderr as `cairn compile` prints them. An error, a
scope the build drops ([Lint §11.1](/spec/lint/#lowering)), or a `@requires` floor the stamp falls
short of ([Versioning and Editions](/spec/versioning-editions/)) refuses the comparison. The floor's
refusal ends with a note naming the stamp, since the command takes no `--target`.

The text report prints on stdout:

- `STRUCTURE against FILE at EDITION VERSION, REASON`, the reason being the stamping rule of
  [§12.4](#124-import-stamping-and-pitfalls) that chose the version;
- each metric with its threshold, the block IoU and `state_accuracy` to three decimals rounded down
  and residual raw as a percentage to a tenth rounded up, so a printed value never reads as meeting
  a threshold its metric misses;
- when more than one pair is compared, each pair's metrics on a line of its own;
- `no cell differs`, or how many regions differ followed by the first ten, one per line, and how
  many more there are;
- the patch, after the metrics it reaches.

When the lift does not converge, stderr carries `error[E_DECOMPILE_DIFF]:` with each metric that
misses its threshold, a note giving the thresholds, and a note on what to do next: append the patch,
edit what the regions name, or, when every cell matches and only residual raw falls short, name what
the raw members build.

`--format json` writes one document to stdout ([Lint §11.2](/spec/lint/#112-machine-readable-payload)):

| Key | Value |
|---|---|
| `structure` | STRUCTURE's file name. |
| `target` | `edition` and `version`, the stamp FILE is built at. |
| `saved` | The version STRUCTURE was saved at, as an import's header names it. |
| `converged` | Whether the lift converges. |
| `code` | `"E_DECOMPILE_DIFF"`, present when it does not. |
| `block_iou`, `state_accuracy`, `residual_ratio` | The metrics, unrounded. |
| `thresholds` | The three thresholds, under the same keys. |
| `counts` | `structure_blocks`, `built_blocks`, `both_blocks`, `matched`, `states_matched` and `residual`, the cells the metrics are ratios of. |
| `bodies` | Each pair: `structure` and `source`, the name of each side's body, absent for a side with none; its metrics; its `counts`. |
| `regions` | Every region: `kind`, `body`, `from` and `to` as `[x, y, z]`, `cells`, `mat`; `built` for a replaced region; `states` for a state region, each `key` with its `structure` and `built` values, `null` for a side that writes none; `cause`, its `kind` and, for the three that name one, its `member` (`keyword`, `id` when written, `address`, `line`); and `patch`, the kept line found for it. |
| `patch` | Present when lines are kept: `edits` in order, the metrics reached, and their `counts`. |

Where FILE does not build at the stamp, the document is `{"diagnostics": [ ... ]}` with the
findings, as `cairn lower` writes it, and a refusal at the run level, a dropped scope or a floor,
writes `{"diagnostics": []}` and says the rest on stderr.

The exit code is 0 when the lift converges; 1 when it does not, when FILE does not parse, raises an
error, drops a scope or needs a later target than the stamp, or when STRUCTURE cannot be read as its
format or was saved past every shipped target; and 2 when either file cannot be located or
STRUCTURE's extension names no format the command reads.

## 12.3 Three tiers of transliteration

Naming is the boundary between transliteration and lift.

| Tier | What it is | Ceiling |
|---|---|---|
| **L0, raw cells** | One voxel per line. Too large for LLM context, so it is an intermediate only. | — |
| **L1, spatially compressed** | Fill aggregation, AABB palette compression, `resolved_state` inverted to the fewest states that build it again (`minecraft:oak_stairs[facing=east,half=top]`, a pane's sides left to the connection rules), a period folded into `raw_repeat`. **No naming.** | The compiler's ceiling. |
| **L2, semantically lifted** | fill → `wall`, repeat → `def` / `use`, concrete block → `mat_slot` + `theme`. | The LLM's ceiling. |

```
# L1: no naming, deterministic
raw_repeat count=5 step=3,0,0
  raw_fill mat=minecraft:glass_pane from=0,2,0 to=1,3,0 origin=imported
# L2: the LLM names it and gives it meaning
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

### 12.3.1 What an import writes

`cairn import --mode raw` writes L1, and one file always imports to one source:

- **Bodies.** Each structure the file holds is a body: the one structure of a `.nbt`, `.schem` or
  `.mcstructure`, and each region of a `.litematic`, in the file's order. A body is named after the
  file's stem, or the region's name, as an identifier: lower case, each run of characters other than
  ASCII letters and digits one `_`, none at either end. A name left empty is `structure`, one that
  would start with a digit takes a `structure_` prefix, one that is a top-level keyword or a name
  Windows reserves a `_structure` suffix, and a name an earlier body took ends in `_2`, then `_3`.
- **Header.** The source opens with three comment lines: the file and the version it was saved at,
  the target it is stamped with and why ([§12.4](#124-import-stamping-and-pitfalls)), and that what
  follows is blocks and their regularity rather than intent. `@cairn`,
  `@requires <edition> version>=<target>` and `@intended_targets ["<target>"]` follow, so the source
  builds at its stamp and is refused below it. A comment above a body keeps what the file says of
  where it stood: a region's name and minimum corner in the schematic, a `.schem`'s `Offset`, and a
  `.mcstructure`'s `structure_world_origin`, the last two only when they are not `0,0,0`.
- **Boxes.** A body's blocks are cut into boxes of one block each, a `raw_block` for a box of one
  cell and a `raw_fill` for any other. The cells are visited `x` fastest, then `z`, then `y`, and at
  each one no box has taken, a box grows along `x`, then `z`, then `y`, as far as every cell it
  would take holds its block. Air is not written, except for one `raw_block mat=minecraft:air` at
  the first cell of the top row when no box reaches that row, so the struct is as tall as the file.
- **Repeats.** Boxes of one block and one size whose corners stand a fixed step apart fold into a
  `raw_repeat`, which writes the box once ([Materials and Themes
  §7.4](/spec/materials-themes/#74-raw-blocks)). Each box not yet folded is tried, in the order of
  the cut, as the first copy of a run whose second is one of the next 32 boxes of its kind, and the
  longest run is folded. A step is never negative, so a run is found from its lowest copy. The boxes
  of every run of one count and step are written under one `raw_repeat`.
- **States.** A block is written with the states that build it again and no others. On Java that is
  every state the file stores but the connection states the connection rules derive back from the
  blocks beside it ([Blockstate Model §6.1](/spec/blockstate/#connections)), which `origin=imported`
  asks them to. On Bedrock a block is first read back as the canonical state its state map writes as
  it ([§12.4](#124-import-stamping-and-pitfalls)), and a state at the default the target builds, or
  one the target builds the same at any value, is left out as well. A cell whose connection states
  the rules would derive otherwise is written with every one of them, and the check is run again
  until every cell agrees.
- **Chunks.** A body wider or deeper than 48 blocks is a `site` of `def`s rather than a `struct`:
  columns of at most 48 by 48 at the body's full height, named `<body>_x<i>_z<j>` by their place
  along `x` and `z`. The `site` places them edge to edge, each where it stood, with `east_of=`,
  `north_of=` and `gap=0`, under a `theme imported` that binds nothing, since a `place` names a
  theme and no raw member reads one. Each chunk can be lifted alone
  ([§12.4](#124-import-stamping-and-pitfalls)).

## 12.4 Import stamping and pitfalls

On import, the `(edition, version)` pair and provenance are stamped onto the source the import
writes (`.nbt`, `.litematic` and `.schem` → java, `.mcstructure` → bedrock). This is what makes an
import reproducible ([Versioning and Editions](/spec/versioning-editions/)). The version is a target
the build ships, chosen by what the file says it was saved at:

| Format | Saved at |
|---|---|
| `.nbt` | `DataVersion` |
| `.litematic` | `MinecraftDataVersion` |
| `.schem` | `DataVersion` |
| `.mcstructure` | the newest `version` a block of its palette carries, the block-state version, which several releases share |

- A file saved at a shipped target's `DataVersion`, or at its block version, is stamped with that
  target. Of several that share a block version, it is the oldest whose state map has a canonical
  state for every block of the file, or the oldest when none has.
- A file saved between two shipped targets is stamped with the nearest after it. The game loads a
  file an older release saved, renaming what it has renamed since, so the later target reads the
  file as the game would. A block the file spells as that release no longer does is refused
  ([§12.2](#122-reverse-direction-the-compiler-transliterates-an-llm-lifts)), and saving the file in
  that release renames it.
- A file saved before every shipped target is stamped with the earliest, on the same terms.
- A file saved after every shipped target is refused, exit 1, since no target this build ships can
  check its blocks; the refusal names the newest.
- A `.mcstructure` whose palette carries no version is stamped with the edition's `latest`.

The header names the saved version as the file has it, `DataVersion 3955 (1.21.1)` with its release
when the build knows it, or a Bedrock block version as its four bytes, `block version 1.21.60.33`,
and names which of these rules chose the stamp.

**Never present import as "recovering author intent."** That is the biggest pitfall. Only voxels and
some regularity can be recovered, and the CLI says so: every import warns with `W_SEMANTIC_LOSS`,
first that it recovers blocks rather than intent, then once for each kind of thing the file holds
that the source does not carry, by the body it is in:

- block entity data, such as a chest's contents, a sign's text or a command block's command, which
  no raw member carries, with how many block entities of each id;
- entities, with how many of each id, since an import writes blocks only
  ([Entities](/spec/entities/));
- structure voids, the cells a `.nbt`'s `blocks` lists nothing at and the `-1` of a `.mcstructure`'s
  first layer, which a placement leaves as the world has them, and which the import writes as air,
  which a placement clears;
- the palettes of a `.nbt` past its first, of which a placement picks one at random;
- on Bedrock, a block its state map has no canonical state for, written by its id alone and built
  with its block's default states; water at a block that cannot hold it as `waterlogged`, which is
  not written; a block that always stands in water where the file's second layer holds none; and a
  second-layer block other than water, which no raw member writes.

Other rules:

- A line an import writes carries `origin=imported` ([Materials and Themes
  §7.4](/spec/materials-themes/#74-raw-blocks)), which keeps it apart from first-class design DSL:
  it sizes the volume and takes the connection states it leaves out from the blocks beside it, as no
  hand-written raw member does. A lift keeps the mark on what it leaves raw, so the residue stays
  explicit.
- Litematica's regions are kept as provenance rather than flattened. Each is a struct of its own,
  named after the region, with its name and its minimum corner in the schematic in a comment above
  it. A region does not read back as a `site` placement: a `place` row sets a body beside another,
  not at a corner a schematic gives it ([Components, Editing, and Multi-building
  §9.3.2](/spec/components-editing-sites/#932-origin-selectors)), and two regions may overlap.
- For entity-bearing schematics, do not mark success on block IoU alone. Keep a separate entity
  metric and extract only first-class entities ([Entities](/spec/entities/)).
- Huge schematics, wider or deeper than 48 blocks or whole villages, do not fit in an LLM's context
  if lifted at once. They need chunk split → per-chunk L1 → per-part lift → join with `site`, run
  over a streaming parse. The import does the first two: such a body is written as a `site` of
  chunks ([§12.3.1](#1231-what-an-import-writes)), and each reader streams its file, holding the
  grid rather than the NBT that encodes it.
- Legacy numeric-ID `.schematic` files from before 1.13 flattening are not supported in v1 ([Purpose
  and Scope](/spec/overview/)): `cairn import` refuses one as an extension it does not read.
