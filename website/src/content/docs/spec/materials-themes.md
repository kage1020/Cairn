---
title: "7. Materials and Themes"
---

## 7.1 Slots as dependency injection

**Subject to change.** Selector bindings, the `key=value` pairs right of a selector row's arrow, are
reserved and not built ([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

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
  walls[class=outer]  -> trim=@spruce_log     # part detailing, via a selector (reserved)
  window[class=small] -> frame=@spruce_wood
```

**The cascade.** A member collects the bindings of every selector row it matches, in source order,
so when two rows bind the same key the member keeps the later value. CSS applies the same rule to
two rules of equal weight. Bindings are reserved (below), so today the cascade decides what a member
collects and nothing that is built.

Rows whose attributes partly overlap rely on that: `window[class=small,side=front]` refines
`window[class=small]` for the members it selects, and the members only the wider row selects keep
the wider row's binding.

Two rows that select the *same* members are different. Same keyword and same attributes means they
match member for member, so no member keeps the earlier row's value of a key they both bind: that
value is dead text whatever a binding comes to mean. That is
`E_DUPLICATE_SELECTOR` ([Lint §11.1](/spec/lint/#111-diagnostic-codes)), an error even while
bindings are reserved, for the reason `E_DUPLICATE_ARG` refuses one key written twice inside one row
([Lint §11.3](/spec/lint/#113-error-vs-warning)). Sameness is by meaning: attribute
order does not count, and `class=` / `id=` / `mat_slot=` values compare as label text, so
`window[class=small]` and `window[class="small"]` are one selector. Rows that coincide but bind
different keys are not reported. They compose, and splitting a long binding list over two lines is
allowed.

**Selector bindings are reserved.** Which keys a row may bind on each keyword, and what each one
paints, is not specified yet, and no lowering reads a binding. The two selector rows above build the
same cottage as a theme without them, and no member's block depends on them.

Until that is specified, the compiler reports each binding as an unreached key,
`W_IGNORED_ARGUMENT` ([Lint](/spec/lint/)). It does so whatever the key or value, on every row whose
keyword it knows, whether the row matched and whether its theme was applied. A row whose keyword it
does not know gets `E_UNKNOWN_KEYWORD` instead. A binding's value is not resolved as a block either,
because what the key would paint is not specified: `frame=@no_such_block` is reported exactly as
`frame=@spruce_wood` or `frame=42` is, with or without `--target`, and no binding makes `check`,
`compile` or `info` refuse a build.

Binding one key twice for the same members does refuse a build, since the earlier value is dead
text whatever a binding comes to mean. Within one row that is `E_DUPLICATE_ARG`; across two rows
that select the same members it is `E_DUPLICATE_SELECTOR`. The rest of a row is still checked:
`E_THEME_SELECTOR_UNMATCHED` applies to it exactly as its own entry says.

`def`, `theme`, and `site` are unified by the same slot-bearing Component mechanism
([Components, Editing, and Multi-building](/spec/components-editing-sites/)).

## 7.2 Canonical vocabulary

A theme binds **canonical tokens**, not raw block IDs. The backend resolves the ID, state names,
state values, and serialization per `(edition, version)`
([Versioning and Editions](/spec/versioning-editions/)). An LLM never needs to know `pillar_axis`,
little-endian NBT, or Bedrock's `weirdo_direction`. The canonical vocabulary spells state names and
state values as Java does: `@oak_stairs[facing=east]` writes Java's `facing`, and the Bedrock backend
maps it to its own `weirdo_direction`.

The backend resolves less than that today. A canonical token is written as `minecraft:<token>` and
checked against the pinned target ([§7.3](#73-mappings-across-version-and-edition)), so
`@water_cauldron` is `E_UNKNOWN_ID` on every Bedrock version. Its states are the ones the token's
Java block takes, and the Bedrock writer maps them through the pack's state map for the target
version: `@oak_log[axis=x]` is written as `pillar_axis=x` on every Bedrock version, while
`@cherry_wood[axis=x]` is `E_STATE_UNMAPPED` at 1.21.40, whose map has no form for it.

Tokens come in two kinds:

| Kind | Example | What it means |
|---|---|---|
| **Canonical block token** | `@oak_planks`, `@water_cauldron`, `@oak_log[axis=x]` | A specific meaning in Minecraft. Silent meaning-breaking downgrades are **forbidden**, so `@water_cauldron` may never become `cauldron`. |
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
| `@sign.oak`, `@sign.oak_wall` | A standing sign and a wall sign |
| `@door.default` | The door block a `door` places when no `mat_slot=` binds one |
| `@chest.default` | The chest block a `chest` places when no `mat_slot=` binds one |
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
| Rename 1:1 | `@dirt_path` (was `grass_path`) | Auto-resolve. |
| Split 1:N | `@cauldron[fluid=water]` → `water_cauldron` | Separate by meaning token. |
| Merge N:1 | `@oak_slab` (was `wooden_slab{variant}`) | Resolve per target. |
| New | `@cherry_planks` | Needs a `requires >=` floor. |
| Deleted | Absent in the target version | Hard error plus alternatives. |

Today only the last row holds as written. A canonical token is written as `minecraft:<token>` and
checked against the pinned target ([Versioning and Editions
§10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference)). A rename is
`E_UNKNOWN_ID`, with a note naming the target's spelling from the registry pack's `aliases`
(`@dirt_path` on Bedrock is answered with `minecraft:grass_path`); only an abstract token resolves
across a rename, through its catalog entry's `overrides` ([§7.2](#72-canonical-vocabulary)). A split
is not separated: `@cauldron[fluid=water]` names a state `minecraft:cauldron` does not have, so a
pinned target refuses it as `E_STATE_DOMAIN` rather than reading it as `water_cauldron`. A merged id
is checked per target like any other. No floor is inferred for a new block: `@pale_oak_planks` with
no `@requires` checks clean on Java 1.21.4 and is `E_UNKNOWN_ID` on a target that lacks it, naming
1.21.4 as the first target that declares it. A deleted id is `E_UNKNOWN_ID`, naming the last target
that declares it; an id no version declares names the nearest block the target has when one is near
enough.

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
