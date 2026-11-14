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
its array would be (`home1`, `hamlet_walkway_home1_entry__home2_entry`). This is the shape the
reverse direction keeps as provenance ([§12.4](#124-import-stamping-and-pitfalls)): a site's regions
read back as its placements. A site and a struct that share a name would write one file, and the
compile refuses the pair before writing anything, as it refuses two `.nbt` files of one name.

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

The reverse direction is not implemented yet. Neither `cairn import` nor `cairn diff-blocks`
exists, so nothing in this section or the two after it runs today.

The compile → diff → patch loop reports like this:

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

`E_DECOMPILE_DIFF` is not implemented, and the `edit` statement its patch suggests does not parse
yet ([§9.2](/spec/components-editing-sites/#92-editing-model)).

Convergence thresholds are block IoU ≥ 0.985, `state_accuracy` ≥ 0.995, and residual raw ≤ 5%. An
exact match is not required, and the residue stays explicit as `raw_fill id=residual_*
origin=imported`.

## 12.3 Three tiers of transliteration

Naming is the boundary between transliteration and lift.

| Tier | What it is | Ceiling |
|---|---|---|
| **L0, raw cells** | One voxel per line. Too large for LLM context, so it is an intermediate only. | — |
| **L1, spatially compressed** | Fill aggregation, AABB palette compression, `resolved_state` → `intent_state` inversion (`stair facing=east half=top`), symmetry and period folded into `raw_repeat`. **No naming.** | The compiler's ceiling. |
| **L2, semantically lifted** | fill → `wall`, repeat → `def` / `use`, concrete block → `mat_slot` + `theme`. | The LLM's ceiling. |

```
# L1: no naming, deterministic
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=minecraft:glass_pane from=0,2,0 to=1,3,0
# L2: the LLM names it and gives it meaning
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 Import stamping and pitfalls

On import, the `(edition, version)` pair and provenance are stamped onto the block-array IR
(`.litematic` → java, `.mcstructure` → bedrock, `.schem` → java). This is what makes an import
reproducible ([Versioning and Editions](/spec/versioning-editions/)).

**Never present import as "recovering author intent."** That is the biggest pitfall. Only voxels
and some regularity can be recovered, and the CLI says so with `W_SEMANTIC_LOSS`. With no import
yet, nothing is stamped and `W_SEMANTIC_LOSS` is not implemented.

Other rules:

- Import-origin `raw_fill` is isolated with `origin=imported` / `residual` and is not treated as
  first-class design DSL.
- Litematica's multiple regions and sub-region offsets are preserved as provenance rather than
  flattened, and regions map to a `site` or several structs.
- For entity-bearing schematics, do not mark success on block IoU alone. Keep a separate entity
  metric and extract only first-class entities ([Entities](/spec/entities/)). Chest contents and command
  blocks are dropped.
- Huge schematics (over 48³, or whole villages) do not fit in an LLM's context if lifted at once.
  They need chunk split → per-chunk L1 → per-part lift → join with `site`, run over a streaming
  parse.
- Legacy numeric-ID `.schematic` files from before 1.13 flattening are not supported in v1
  ([Purpose and Scope](/spec/overview/)).
