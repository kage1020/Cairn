---
title: "7. Materials and Themes"
---

## 7.1 Slots as dependency injection

The structure never writes a concrete block name. It carries `mat_slot` injection points, and a
`theme` binds values to slots and selectors, the way CSS or dependency injection does. That
separates structure (where the walls are) from style (which blocks, what detailing).

```
def cottage size=9x7:
  floor  id=floor  mat_slot=floor
  walls  id=walls  class=outer mat_slot=wall height=4
  roof   id=roof   kind=gable  mat_slot=roof
  window id=front_windows class=small side=front y=2 offset=1 size=2x2 repeat=2 step=5 mat_slot=glass

theme medieval:
  slot floor -> @spruce_planks
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  slot glass -> @glass_pane
  walls[class=outer]  -> trim=@spruce_log     # part detailing, via a selector
  window[class=small] -> frame=@spruce_wood
```

**What a row selects.** A selector is a member keyword and its attributes in brackets, and it
selects the members of that keyword that match each attribute. `level=NAME` matches a member that
stands under a `level` whose `id=` is `NAME`, and any other attribute matches the value its key
holds on the member's line. Brackets may follow one another: `window[class=small][side=front]` is
`window[class=small,side=front]`. An `edit` line selects by the same grammar ([Components, Editing,
and Multi-building §9.2](/spec/components-editing-sites/#92-editing-model)).

**The cascade.** A member collects the bindings of every selector row it matches, in source order,
so when two rows bind the same key the member keeps the later value. CSS applies the same rule to
two rules of equal weight.

Rows whose attributes partly overlap rely on that: `window[class=small,side=front]` refines
`window[class=small]` for the members it selects, and the members only the wider row selects keep
the wider row's binding.

Two rows that select the *same* members are different. Same keyword and same attributes means they
match member for member, so no member keeps the earlier row's value of a key they both bind: that
value is built for none of them. That is
`E_DUPLICATE_SELECTOR` ([Lint §11.1](/spec/lint/#111-diagnostic-codes)), an error for the reason
`E_DUPLICATE_ARG` refuses one key written twice inside one row
([Lint §11.3](/spec/lint/#113-error-vs-warning)). Sameness is by meaning: attribute
order does not count, `class=` / `id=` / `mat_slot=` / `level=` values compare as label text, and
numbers compare by the number they write ([Syntax
§5.1.1](/spec/syntax/#511-literals-and-separators)), so `window[class=small]` and
`window[class="small"]` are one selector, and so are `walls[height=4]` and `walls[height=4.0]`. Rows
that coincide but bind different keys are not reported. They compose, and splitting a long binding
list over two lines is allowed.

`def`, `theme`, and `site` are unified by the same slot-bearing Component mechanism
([Components, Editing, and Multi-building](/spec/components-editing-sites/)).

### Selector bindings

A selector row binds the block of a **part** of each member it selects: cells the member builds, or
cells of the wall round what it cuts, laid in a block of their own. Each keyword binds the keys
below and no other.

| Keyword | Key | The part it lays |
|---|---|---|
| `walls` | `trim=` | The corner posts: the four cells where two sides of the wall meet, in every row the member paints. |
| `door`, `window` | `frame=` | A ring one cell wide round each opening the member cuts, in the plane of its wall. |

No other keyword binds a key yet. A key the row's keyword does not bind lays nothing and is
`E_UNKNOWN_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)), naming the keys that keyword
binds, on every row whose keyword the compiler knows, whether the row matched and whether its theme
was applied. A row whose keyword it does not know gets `E_UNKNOWN_KEYWORD` instead.

**A bound key and `mat_slot=`.** A binding names the block of its part and nothing else. The
member's own `mat_slot=`, or the default block its role falls back to, still builds every other
cell, and a binding never changes which slot the member reads. A part needs its member: a member
that builds nothing, refused or with a `mat_slot=` that resolves to no block, lays no part either.
A `window` written without a `mat_slot=`, cut to air, is still framed.

**The part is the member's.** It is laid in its member's phase
([Compilation §4.1](/spec/compilation/#41-phase-evaluation)) and its cells are that member's, so a
later phase builds over them and a member of the same phase that meets them is judged as any two
members are ([Compilation §4.8](/spec/compilation/#48-within-phase-conflicts-and-the-palette)). The
corner posts are painted with the walls, in `massing`. A frame is laid in `openings` once every
opening is cut and every door hung, so the order the openings are written in moves no frame:

- The ring is the column on each side of the opening and the row above and the row below it,
  corners included, clipped to the wall course that holds the opening.
- It is laid only on cells a `walls` member still holds then: never on another opening, and never
  on a cell another member took from the walls. A ring left with no such cell lays nothing, and is
  not reported.
- It never lies on the wall's two end columns, which the wall shares with the walls round each
  corner, so a frame neither turns a corner nor covers a corner post.
- Each rectangle a `repeat=` series or a `sym=true` mirror cuts gets its own ring.
- A door's derived `hinge` reads the wall as the openings left it, before any frame
  ([Blockstate Model §6.1](/spec/blockstate/#61-derive-by-default-promote-on-override)).

Two frames of different blocks that meet on a cell are `W_PHASE_CONFLICT`, and the later line wins;
of one block, they are not reported.

**The value is a block.** A binding's value is resolved as a `slot` rule's value is
([§7.2](#72-canonical-vocabulary), [§7.3](#73-mappings-across-version-and-edition)), against the
`(edition, version)` the compile targets and nothing else. A value that is no material token is
`E_UNKNOWN_SLOT_TARGET`. A value the target cannot build earns the finding a slot's value would, at
the binding, and its member is built without the part. It is judged wherever its member is lowered,
even when the member's own line is refused, so one compile reports both.

The two selector rows in the example above build a spruce-log post at each corner of the cottage's
walls and a ring of spruce wood round each small window; without them the walls are cobblestone to
the corners and each window is cut straight into them.

## 7.2 Canonical vocabulary

A theme binds **canonical tokens**, not raw block IDs. The backend resolves the ID, state names,
state values, and serialization per `(edition, version)`
([Versioning and Editions](/spec/versioning-editions/)). An LLM never needs to know `pillar_axis`,
little-endian NBT, or Bedrock's `weirdo_direction`. The canonical vocabulary spells state names and
state values as Java does: `@oak_stairs[facing=east]` writes Java's `facing`, and the Bedrock backend
maps it to its own `weirdo_direction`.

A canonical token is written as `minecraft:<token>` where the pinned target declares that ID, and
through the target's tables where it does not ([§7.3](#73-mappings-across-version-and-edition)), so
`@water_cauldron` is written on Bedrock as its `cauldron` holding water. Its states are the ones the
token's Java block takes, and the Bedrock writer maps them through the pack's state map for the
target version: `@oak_log[axis=x]` is written as `pillar_axis=x` on every Bedrock version, while
`@cherry_wood[axis=x]` is `E_STATE_UNMAPPED` at 1.21.40, whose map has no form for it.

Tokens come in two kinds:

| Kind | Example | What it means |
|---|---|---|
| **Canonical block token** | `@oak_planks`, `@water_cauldron`, `@oak_log[axis=x]` | A specific meaning in Minecraft. Silent meaning-breaking downgrades are **forbidden**, so `@water_cauldron` may never become an empty `cauldron`. |
| **Abstract material token** | `@floor.wood.broadleaf`, `@roof.dark_wood` | An aesthetic choice. Theme policy MAY downgrade these (oak ↔ birch). |

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # abstract: resolved by target and policy
theme exact_oak:
  slot floor -> @oak_planks             # canonical: pinned 1:1
```

An abstract token resolves through the registry pack's `materials` catalog, which maps it to a
block, and an entry's `overrides` name the versions that spell that block differently
([Versioning and Editions §10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference)).
A token the catalog does not declare is `E_UNKNOWN_ABSTRACT_TOKEN` ([Lint](/spec/lint/)) wherever a
pack is offered: at a pinned target, and in `cairn lower` and `cairn info`.

### Tier-1 tokens

The abstract tokens the built-in `materials` catalogs declare are the **tier-1** vocabulary, which
[Compatibility Tiers](/spec/compatibility/#c1-tier-definitions) holds Stable. Both editions'
catalogs declare the same tokens, so a theme that binds only these resolves on either:

| Tokens | For |
|---|---|
| `@floor.wood.broadleaf`, `@floor.wood.conifer`, `@floor.stone.cobble`, `@floor.stone.smooth` | Floors |
| `@wall.wood.broadleaf`, `@wall.wood.conifer`, `@wall.stone.cobble`, `@wall.stone.smooth` | Walls |
| `@wood.dark`, `@wood.light`, `@wood.warm`, `@wood.cool` | Planks, by tone |
| `@roof.dark_wood`, `@roof.light_wood`, `@roof.warm_wood`, `@roof.cool_wood` | Roof stairs, by tone |
| `@beam.default` | The pillar block a `beam` lays when no `mat_slot=` binds one |
| `@sign.oak`, `@sign.oak_wall` | The standing sign a `sign` stands, and the wall sign it hangs, when no `mat_slot=` binds one |
| `@bed.default` | The bed a `bed` lays when no `mat_slot=` binds one, a red one |
| `@bed.white`, `@bed.orange`, `@bed.magenta`, `@bed.light_blue`, `@bed.yellow`, `@bed.lime`, `@bed.pink`, `@bed.gray`, `@bed.light_gray`, `@bed.cyan`, `@bed.purple`, `@bed.blue`, `@bed.brown`, `@bed.green`, `@bed.red`, `@bed.black` | The bed of each dye's colour |
| `@door.default` | The door block a `door` places when no `mat_slot=` binds one |
| `@chest.default` | The chest block a `chest` places when no `mat_slot=` binds one |
| `@trapdoor.default` | The trapdoor a `trapdoor` sets when no `mat_slot=` binds one |
| `@note_block.default` | The note block a `note_block` sets |
| `@pressure_plate.default` | The block a `pressure_plate` places when no `mat_slot=` binds one |
| `@lever.default` | The lever a `lever` hangs on its wall |
| `@button.default` | The button a `button` hangs on its wall when no `mat_slot=` binds one |
| `@daylight.default` | The daylight detector a `daylight` sets |
| `@observer.default` | The observer an `observer` sets |
| `@lamp.default` | The redstone lamp a `lamp` sets |
| `@piston.default` | The piston a `piston` sets |
| `@piston.sticky` | The sticky piston a `piston` with `sticky=true` sets |
| `@dispenser.default` | The dispenser a `dispenser` sets |
| `@repeater.default` | The repeater a `repeater` sets |
| `@comparator.default` | The comparator a `comparator` sets |
| `@redstone_torch.default`, `@redstone_torch.wall` | The standing torch and the wall torch a `redstone_torch` sets |
| `@redstone_dust.default` | The dust a `redstone_dust` lays |
| `@circuit.block` | The block a struct's circuit stands its dust, torches, repeaters and comparators on, under the floor |

The block a tier-1 token resolves to is **tier-2**, which is Evolving: it is the catalog's answer
for the target, and a release may change it. Adding a token to the catalogs breaks nothing.
Renaming or retiring one is a break to a Stable surface, so the old token keeps resolving, with
`W_DEPRECATED`, for the deprecation window. A canonical block token is neither tier-1 nor tier-2.
It names a block through the mappings of [§7.3](#73-mappings-across-version-and-edition), and the
names a target accepts follow its registry pack.
[Compatibility Tiers](/spec/compatibility/#evolving) holds them Evolving.

## 7.3 Mappings across version and edition

A canonical token absorbs five patterns. The resolution table's structure is in
[Versioning and Editions](/spec/versioning-editions/).

| Pattern | Example | Policy |
|---|---|---|
| Rename 1:1 | `@chain` (Java 26.3's `iron_chain`) | Auto-resolve per target. |
| Split 1:N | `@cauldron[fluid=water]` → `water_cauldron` | Separate by meaning token. |
| Merge N:1 | `@stone_bricks` (Bedrock 1.21.0's `stonebrick[stone_brick_type=default]`) | Resolve per target. |
| New | `@pale_oak_planks` | Refused below the first target that declares it. |
| Deleted | Absent in the target version | Hard error plus alternatives. |

A canonical token is written as `minecraft:<token>` and checked against the pinned target
([Versioning and Editions
§10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference)). Three of the
target's registry pack tables answer the token first, in this order:

1. **Meaning token**, for a split. The pack's `meanings` table lists each token whose block split
   into several, with its meaning token, a property the block does not have whose value picks one of
   them, and the block each value names. `@cauldron[fluid=water]` is `water_cauldron`, with `fluid`
   dropped and every other state kept. A value the row does not list is `E_STATE_DOMAIN`, which
   names the values it does list.
2. **Rename**, where the pinned target does not declare the token's ID. When some version of the
   edition declares it and its `aliases` group is one block renamed across the edition's range, so
   that no version declares two of its spellings, the token is written under the one spelling the
   target declares: Java 26.3 writes `@chain` as `iron_chain`, and Java 1.21.4 writes `@iron_chain`
   as `chain`.
3. **State map**, on Bedrock, where the pinned version does not declare the ID. A Java block the
   version's state map writes as a Bedrock block is written as that block when no other Java block
   mapped onto it takes any of the same forms there, so what is written still says which block it
   was. `@oak_sign` is written as `standing_sign`, and `@stone_bricks` at 1.21.0 as
   `stonebrick[stone_brick_type=default]`, a merge resolved per target. The palette keeps the Java
   block and its states, which the writer maps as it maps any other's. A bed is not written so:
   Bedrock keeps every bed's colour, and every flower pot's plant, in the block entity, so its
   blocks cannot tell them apart and `@black_bed` stays `E_UNKNOWN_ID` there.

A token none of them answers is `E_UNKNOWN_ID`, with a note naming the target's spelling from the
pack's `aliases` where it has one: `@stonebrick` on Bedrock 1.21.60 is answered with the four blocks
1.21.40 split it into, since choosing one would be the silent substitution [Versioning and Editions
§10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference) forbids. With no
target pinned, a token some targetable version writes under another spelling lowers as itself, so
the range-wide passes of `cairn check` and `cairn info`, and the language server, neither refuse
what a pinned build writes nor miss what every pinned build raises on it: a `shed` roof bound to
`@oak_sign` is `E_INCOMPATIBLE_MATERIAL` there too. A raw block names the ID it writes and is not
read through these tables ([§7.4](#74-raw-blocks)), so a meaning token is no state of it and
`raw_block mat=minecraft:cauldron[fluid=water]` is `E_STATE_DOMAIN`, since `cauldron` has no
`fluid`. An abstract token resolves through its catalog entry and that entry's `overrides`
([§7.2](#72-canonical-vocabulary)). `cairn info` names each token a version writes under another ID,
with the ID it writes ([Versioning and Editions](/spec/versioning-editions/#the-respelled-row)).

No floor is inferred for a new block: `@pale_oak_planks` with no `@requires` checks clean on Java
1.21.4 and is `E_UNKNOWN_ID` on a target that lacks it, naming 1.21.4 as the first target that
declares it, and `cairn info` starts Java's inferred range there ([Versioning and
Editions](/spec/versioning-editions/#the-inferred-range-row)). A deleted id is `E_UNKNOWN_ID`,
naming the last target that declares it, and ends the inferred range there, so a floor above that
target that some target meets is `E_REQUIRES_CONFLICT`; an id no version declares names the nearest
block the target has when one is near enough.

**Only ID, state, and serialization differences may be absorbed.** Concept absence and
game-behaviour differences are not.

## 7.4 Raw blocks

A theme binds canonical tokens, and a member is built from what its slot resolves to. Where that is
not enough, two members name the block themselves. They are the **escape hatch**, built in the
`raw` phase ([Compilation Model §4.1](/spec/compilation/#41-phase-evaluation)):

```
raw_block mat=minecraft:light[level=15] at=4,3,2
raw_fill  mat=minecraft:tinted_glass from=1,2,0 to=3,3,0
```

`mat=` is a **block id**, `namespace:path` with an optional state literal touching it
([Syntax §5.1.1](/spec/syntax/#511-literals-and-separators)). A `raw_block` puts that block in the
one cell its `at=` names. A `raw_fill` puts it in every cell of the box between `from=` and `to=`,
two opposite corners written in either order, as a `fill`'s are
([Blockstate Model §6.3](/spec/blockstate/#63-waterlogged)). Each is a coordinate triple measured
from the footprint's low corner, and from the level under `level y=N`.

A raw block skips the theme and the slot. No `mat_slot=` binds it, so one written on the line is
`W_IGNORED_ARGUMENT`; no abstract token or catalog default stands in for it; and a struct with no
theme bound still builds it. It does not skip the target. A build that pins a version checks the
id and its states as it checks any block's: an id the target does not declare is `E_UNKNOWN_ID`,
and a state outside the block's is `E_STATE_DOMAIN` ([Versioning and Editions
§10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference)). On Bedrock the
states are the canonical vocabulary, which the state map writes as a canonical token's
([§7.2](#72-canonical-vocabulary)), so a state the map has no form for is `E_STATE_UNMAPPED`.

The block is written as given. The `raw` phase runs after every other, so a raw block replaces
whatever another member put in its cell, air included, and none of its states is derived: water
poured before it does not waterlog it, and the connection rules neither join it to its neighbours
nor pair it with a chest beside it. A neighbour still joins it as it would any block, reading the
connection states it writes and the block's defaults for the rest, so a pane that writes no side
stands as a lone post to the wall under it
([Blockstate Model §6.1](/spec/blockstate/#connections)).
A raw block over one half of a `door` replaces that half, and the game then drops the other; the
line asks for that block, so nothing warns. Two raw members writing one cell contest it in their
phase, and the later line wins with `W_PHASE_CONFLICT`. A raw member does not size the volume. As
with a `fill`, a cell outside the footprint, or above every row the struct as written reaches, is
`E_INVALID_MEMBER`, and a cell in a row the struct reaches but does not build is
`W_DEFERRED_MEMBER`.

A `mat=` that is missing or not a block id is `E_INVALID_MEMBER`. That includes `@stone`, a
canonical token, which a slot binds. So is an `at=`, `from=` or `to=` that is missing or not a
triple. The `mat=` is judged whether or not the cells build, so a raw member deferred for its row,
or refused for a cell outside the struct, still gets the finding its `mat=` earns.

A raw member may stand outside an `@edition` guard. Raw ids are edition-specific by nature, and the
check at the pinned target keeps an unguarded one honest: `minecraft:light` builds on Java and is
`E_UNKNOWN_ID` on Bedrock, which spells the block `light_block`, and from 1.21.40 `light_block_0` …
`light_block_15`. A file built for both editions writes an id both declare, or one branch per
edition under `@edition` ([Versioning and Editions
§10.7](/spec/versioning-editions/#writing-an-alternative)).

`raw_repeat` and the `origin=imported` that marks what import writes belong to import
([Ecosystem Interop §12.3](/spec/ecosystem-interop/#123-three-tiers-of-transliteration)), which
does not exist yet. Until it does, `raw_repeat` is `E_UNKNOWN_KEYWORD` and `origin=` is
`E_UNKNOWN_ARGUMENT`.
