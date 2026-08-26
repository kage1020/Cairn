---
title: "7. Materials and Themes"
---

## 7.1 Slots as dependency injection

The structure never writes a concrete block name. It carries `mat_slot` injection points, and a
`theme` binds values to slots and selectors, the way CSS or dependency injection does. That
separates structure (where the walls are) from style (which blocks, what detailing).

```
def cottage class=house size=9x7:
  floor  id=floor  mat_slot=floor
  walls  id=walls  class=outer mat_slot=wall height=4
  roof   id=roof   kind=gable  mat_slot=roof
  window id=front_windows class=small side=front y=2 repeat=2 mat_slot=glass

theme medieval:
  slot wall  -> @cobblestone
  slot roof  -> @spruce_stairs
  walls[class=outer]  -> trim=@spruce_log     # part detailing, via a selector
  window[class=small] -> frame=@spruce_wood
```

**The cascade.** A member collects the bindings of every selector row it matches, in source order,
so when two rows bind the same key the later value wins. CSS applies the same rule to two rules of
equal weight.

Rows whose attributes partly overlap rely on that: `window[class=small,side=front]` refines
`window[class=small]` for the members it selects, and the members only the wider row selects keep
the wider row's binding.

Two rows that select the *same* members are different. Same keyword and same attributes means they
match member for member, so a key they both bind is read by nothing on the earlier row. That is
`E_DUPLICATE_SELECTOR` ([Lint §11.1](lint#111-diagnostic-codes)). Sameness is by meaning: attribute
order does not count, and `class=` / `id=` / `mat_slot=` values compare as label text, so
`window[class=small]` and `window[class="small"]` are one selector. Rows that coincide but bind
different keys are not reported. They compose, and splitting a long binding list over two lines is
allowed.

`def`, `theme`, and `site` are unified by the same slot-bearing Component mechanism
([Components, Editing, and Multi-building](components-editing-sites)).

## 7.2 Canonical vocabulary

A theme binds **canonical tokens**, not raw block IDs. The backend resolves the ID, state names,
state values, and serialization per `(edition, version)`
([Versioning and Editions](versioning-editions)). An LLM never needs to know `pillar_axis`,
little-endian NBT, or Bedrock's `weirdo_direction`.

Tokens come in two tiers:

| Tier | Example | What it means |
|---|---|---|
| **Canonical block token** | `@oak_planks`, `@water_cauldron`, `@oak_log[axis=x]` | A specific meaning in Minecraft. Silent meaning-breaking downgrades are **forbidden**, so `@water_cauldron` may never become `cauldron`. |
| **Abstract material token** | `@floor.wood.broadleaf`, `@roof.dark_wood` | An aesthetic choice. Theme policy MAY downgrade these (oak ↔ birch). |

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # abstract: resolved by target and policy
theme exact_oak:
  slot floor -> @oak_planks             # canonical: pinned 1:1
```

## 7.3 Mappings across version and edition

A canonical token absorbs five patterns. The resolution table's structure is in
[Versioning and Editions](versioning-editions).

| Pattern | Example | Policy |
|---|---|---|
| Rename 1:1 | `@dirt_path` (was `grass_path`) | Auto-resolve. |
| Split 1:N | `@cauldron[fluid=water]` → `water_cauldron` | Separate by meaning token. |
| Merge N:1 | `@oak_slab` (was `wooden_slab{variant}`) | Resolve per target. |
| New | `@cherry_planks` | Needs a `requires >=` floor. |
| Deleted | Absent in the target version | Hard error plus alternatives. |

**Only ID, state, and serialization differences may be absorbed.** Concept absence and
game-behaviour differences are not.
