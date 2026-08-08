---
title: "7. Materials and Themes"
---

## 7.1 Slots as dependency injection
The structure side never writes a concrete block name; it only carries a `mat_slot` (an injection
point). A `theme` binds values to slots and selectors (analogous to CSS / dependency injection). This
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
  walls[class=outer]  -> trim=@spruce_log     # inject part detailing via a selector
  window[class=small] -> frame=@spruce_wood
```

A member collects the bindings of **every** selector row it matches, taken in source order, so a key
two rows both bind keeps the later value — the cascade CSS applies to two rules of equal weight. Rows
whose attributes merely overlap rely on that: `window[class=small,side=front]` refines
`window[class=small]` for the members it selects, and the members only the wider row selects keep the
wider row's binding.

Two rows that select the *same* members are a different thing. Same keyword and same attributes means
they match member for member, so a key they both bind is read by nothing on the earlier row; that is
`E_DUPLICATE_SELECTOR` ([Lint and Diagnostics](lint) §11.1). Sameness is by meaning: attribute order
is not part of it, and a `class=` / `id=` / `mat_slot=` value compares as label text, so
`window[class=small]` and `window[class="small"]` are one selector. Rows that coincide but bind
different keys are not reported — they compose, and splitting a long binding list over two lines is
allowed.

`def`, `theme`, and `site` are unified by the same slot-bearing Component mechanism
([Components, Editing, and Multi-building](components-editing-sites)).

## 7.2 Canonical vocabulary (canonical token)
The values a theme/slot binds are **canonical tokens**, not raw block IDs. The backend resolves the
ID, state names, state values, and serialization per `(edition, version)`
([Versioning and Editions](versioning-editions)). An LLM never needs to know `pillar_axis`,
little-endian NBT, or Bedrock's weirdo_direction.

Canonical tokens come in **two tiers**:
- **canonical block token** (a meaning in Minecraft): `@oak_planks` `@water_cauldron` `@oak_log[axis=x]`.
  Silent meaning-breaking downgrades are **forbidden** (e.g. `@water_cauldron` → `cauldron` is not allowed).
- **abstract material token** (an aesthetic choice): `@floor.wood.broadleaf` `@roof.dark_wood`. Theme
  policy MAY downgrade these (e.g. oak↔birch).

```
theme cottage:
  slot floor -> @floor.wood.broadleaf   # abstract: resolved to a concrete material by target/policy
theme exact_oak:
  slot floor -> @oak_planks             # canonical: pinned 1:1
```

## 7.3 Mappings across version and edition
A canonical token absorbs the following five patterns (the resolution table structure is in
[Versioning and Editions](versioning-editions)):

| Pattern | Example | Policy |
|---|---|---|
| rename 1:1 | `@dirt_path` (grass_path→dirt_path) | auto-resolve |
| split 1:N | `@cauldron[fluid=water]` (cauldron→water_cauldron) | separate by meaning token |
| merge N:1 | `@oak_slab` (wooden_slab{variant}→oak_slab) | resolve per target |
| new | `@cherry_planks` | requires `requires >=` |
| deleted | (absent in the target version) | hard error + alternatives |

**Only ID/state/serialization differences may be absorbed.** "Concept absence" and "game-behavior
differences" are not absorbed ([Versioning and Editions](versioning-editions)).
