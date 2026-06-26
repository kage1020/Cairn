---
title: "9. Components, Editing, and Multi-building"
---

## 9.1 def (components)
`def` defines a slot-bearing Component, unified with `theme` and `site` by the same mechanism. This
keeps the reference system from fracturing across editing, theming, and multi-building connection.

- Parameterization (variable size, etc.) is allowed. Recursion is forbidden.
- A `def` may declare `requires version>=X`; the minimum version of a composite is the max of its
  parts ([Versioning and Editions](versioning-editions)).

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 Editing model
**Explicit IDs + auto stable addresses, combined.** Important members carry `id=`; unspecified members
get a **meaning-based stable address** auto-assigned by the compiler. Addresses derive from
parent / role / side / level / offset rather than generation order, so they stay stable under
appends.

Edits are patch DSL against a selector/address:

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

Editing at the level of a concept ("make only the second-floor windows arched") must be possible
without breaking the whole. Edit diffs look only at `intent_state` ([Blockstate Model](blockstate)),
so a change in derived results does not harm edit stability.

## 9.3 Multi-building (site)
Do not make the AI do absolute-coordinate arithmetic. Place via topological relational constraints;
resolving to absolute coordinates is the compiler's responsibility.

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

Each struct exposes ports (position / normal / width), and `connect` joins them. Villages and castles
that exceed the structure block's 48³ limit are expressed as the composition of multiple structs.

### 9.3.1 Coordinate convention
- `east` advances along `+x`; `north` retreats along `-z`. This matches the `front` is `+z`
  convention from §5.4 — a building whose `front` faces south sits with its facade on `+z`, and
  `north_of=X` puts the next placement behind it.
- The Y axis is unaffected by topological selectors; every placement currently lands at `y = 0`.

### 9.3.2 Origin selectors
Each `place` carries **exactly one** of `at`, `east_of`, `north_of`:

| Selector | Effect | Notes |
|---|---|---|
| `at=origin` | Anchors the placement at world `(0, 0, 0)`. | The only legal `at=` value. The first `place` in a site must use this anchor — there is no implicit `at=origin` default. |
| `east_of=ID gap=N` | New origin = prior `(x + dims.x + N, y, z)`. | `ID` must name a place declared earlier in the same `site` body. `gap` is in blocks and is edge-to-edge (0 → walls touch). Defaults to `0` when omitted. |
| `north_of=ID gap=N` | New origin = prior `(x, y, z − dims.z − N)`. | Same `ID` and `gap` rules as `east_of`. |

Combining selectors (`at` + `east_of`, or `east_of` + `north_of`) is rejected with
`E_INVALID_PLACE_ORIGIN`; using `at=` with anything other than `origin` is the same error.

### 9.3.3 Cross-scope references
- `use=NAME` must name a top-level `def`. Unknown names fail with `E_UNRESOLVED_PLACE_REF`, with a
  nearest-match suggestion when one fits the standard spell cap (§10.6 of `versioning-editions.md`).
- `theme=NAME` must name a `theme` declared in the same file. Unknown themes fail with
  `E_UNRESOLVED_THEME_REF`, again carrying a nearest-match note.
- A `def` that no `place use=NAME` references is reported as `W_UNUSED_DEF` (advisory) so a typo on
  the `use=` side does not silently produce an empty build.
- Two `place` rows in one site cannot share an `id=`; the duplicate is flagged with
  `E_DUPLICATE_PLACE_ID` and the diagnostic carries a span pointer back to the first declaration.

### 9.3.4 Output naming
The compiler writes one `.nbt` per `place`, named after the `id=` (e.g. `home1.nbt`,
`home2.nbt`). The world-space origin and the `(site, def, theme)` provenance for every placement is
recorded in `build.cairn.lock` under `placements`, so a downstream consumer can rebuild the layout
without re-running the coordinate solver.

### 9.3.5 Ports and `connect` (deferred to M3-PR4)
`connect` lines parse and validate as `place` siblings, but the **port model** (the
`position / normal / width` triple a structure exposes through a named anchor like `door.entry`) is
not yet specified in detail, and walkway voxelisation is intentionally deferred so the port surface
can land in one piece. Until then `connect` rows emit `W_DEFERRED_MEMBER` and are otherwise inert;
the cross-port reference validation (`E_UNRESOLVED_PORT`) lands with the port model.
