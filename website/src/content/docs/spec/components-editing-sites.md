---
title: "9. Components, Editing, and Multi-building"
---

## 9.1 `def` — components

`def` defines a slot-bearing Component, unified with `theme` and `site` by the same mechanism, so
the reference system does not fracture across editing, theming, and multi-building.

Parameterization (variable size and so on) is allowed; recursion is forbidden. A `def` may declare
`requires version>=X`, and the minimum version of a composite is the max of its parts
([Versioning and Editions](versioning-editions)).

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 Editing model

Important members carry `id=`. Members without one get a **meaning-based stable address** derived
from parent / role / side / level / offset rather than from generation order, so addresses stay
stable when you append to a struct.

Edits are a patch DSL against a selector or address:

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

Editing at the level of a concept — "make only the second-floor windows arched" — must be possible
without breaking the whole. Edit diffs look only at `intent_state`
([Blockstate Model](blockstate)), so a change in derived results does not harm edit stability.

## 9.3 Multi-building — `site`

Never make the AI do absolute-coordinate arithmetic. Placements are topological constraints;
resolving them to coordinates is the compiler's job.

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

Each struct exposes ports (position, normal, width) and `connect` joins them. Villages and castles
past the structure block's 48³ limit are expressed as a composition of several structs.

### 9.3.1 Coordinate convention

`east` advances along `+x` and `north` retreats along `-z`. This matches "front is `+z`" from §5.4:
a building whose `front` faces south sits with its facade on `+z`, and `north_of=X` puts the next
placement behind it.

The Y axis is unaffected by topological selectors; every placement currently lands at `y = 0`.

### 9.3.2 Origin selectors

Each `place` carries **exactly one** of `at`, `east_of`, `north_of`:

| Selector | Effect | Notes |
|---|---|---|
| `at=origin` | Anchors at world `(0, 0, 0)`. | The only legal `at=` value. The first `place` in a site must use it — there is no implicit default. |
| `east_of=ID gap=N` | New origin = prior `(x + dims.x + N, y, z)`. | `ID` must name a place declared earlier in the same `site`. `gap` is in blocks, edge to edge (`0` → walls touch), defaulting to `0`. |
| `north_of=ID gap=N` | New origin = prior `(x, y, z − dims.z − N)`. | Same `ID` and `gap` rules as `east_of`. |

Combining selectors, or using `at=` with anything other than `origin`, is
`E_INVALID_PLACE_ORIGIN`.

### 9.3.3 Cross-scope references

Every `place` row declares `id=`, `use=`, and `theme=`. A row short of any of them cannot become a
placement — there is no name for its `.nbt`, no `def` to instantiate, or no theme to resolve its
`mat_slot=` members against — so it is `E_INCOMPLETE_PLACE`, the message names every missing key,
and the row is dropped. A key that is present but not a label (`use=3`) is
`E_TYPE_MISMATCH_LABEL` instead.

`id=` is required rather than auto-assigned, unlike the geometry members of §9.2, because it is the
name `east_of=` and `connect` refer to and the name its `.nbt` is written under (§9.3.4).

| Code | Cause |
|---|---|
| `E_UNRESOLVED_PLACE_REF` | `use=NAME` does not name a top-level `def`. Carries a nearest-match suggestion. |
| `E_UNRESOLVED_THEME_REF` | `theme=NAME` does not name a `theme` in the same file. Carries a nearest-match suggestion. |
| `E_DUPLICATE_PLACE_ID` | Two `place` rows in one site share an `id=`. The diagnostic points back to the first declaration. |
| `W_UNUSED_DEF` | A `def` no `place use=NAME` references — advisory, so a typo on the `use=` side does not silently produce an empty build. |

### 9.3.4 Output naming

The compiler writes one `.nbt` per `place`, named after the `id=` (`home1.nbt`, `home2.nbt`). The
world-space origin and the `(site, def, theme)` provenance of every placement is recorded in
`build.cairn.lock` under `placements`, so a downstream consumer can rebuild the layout without
re-running the coordinate solver.

### 9.3.5 Ports and `connect`

`connect FROM.PORT to TO.PORT path=@MATERIAL` lays a 1-block-wide walkway between two named ports on
placements within the same `site`.

**What a port is.** A port is the `(place, member_id)` pair that `PLACE.PORT` resolves to. Ports are
exposed on `door` and `window` members of the referenced `def`; stair and roof ports are reserved
for a future extension.

**Where a port sits.** One block outside the member's `side=` wall, at the placement's ground row
(`place_origin.1`). `front` / `back` / `left` / `right` map to `+z` / `-z` / `-x` / `+x` (§9.3.1).
The wall-local offset comes from:

- a `door`'s `at=` value — `center`, `left`, or `right` (§5.4). Numeric offsets are reserved.
- a `window`'s geometric centre, `offset + size.w / 2`.

The placement's overhang shifts the port out into the overhang ring beyond the outer face. A
`window`'s authored `y=` does **not** lift the port off the ground row: the walkway is a 1-voxel
flat strip whose Y must agree with the other endpoint. A `sym=true` window contributes a single port
at the primary `offset` side — the mirrored cut still appears in the wall, but the `id=` resolves to
one coordinate.

**A window has to fit its wall** to anchor a port, horizontally and vertically:

```
offset + size.w ≤ wall_length            # horizontal
y ≥ 1  and  y + size.h ≤ H + 1           # vertical, for walls height=H
```

`walls height=H` fills world rows `1 … H`; the floor slab owns row `0`. A window whose rows fall
outside the wall cannot anchor a walkway, and the row drops with a `W_DEFERRED_MEMBER` whose notes
list the door, window, and reserved-role contracts in turn.

One case where the port and the opening disagree today: the port reads the rows the `walls` members
*declare*, while the openings pass reads the rows they will *paint*. On `walls` whose `mat_slot=`
does not resolve, the cut is deferred and the port still anchors.

**How the path runs.** A Manhattan L — x-axis leg, then z-axis leg — at the two ports' shared Y. 3D
path search (staircases, multi-level walkways) is deliberately out of scope.

When that L would cross an existing structure floor, the compiler searches the ground plane for a
detour: the shortest route around the obstacle, and among equal-length routes the one with the
fewest turns, with deterministic tie-breaking so the same source always lays the same strip.

Only when no unobstructed route exists at all — a port buried under another placement's floor, a
fully enclosed target, or a site past the router's search-area cap — does the row fall back to the
straight L with the colliding cells skipped. That earns one `W_WALKWAY_BLOCKED` naming the concrete
cause and its remedy (move the buried door or window, widen the gap, or bring the structures
closer), with `data: { kind: "walkway_blocked", skipped: N }` in `--format json`.

**Material.** `path=@TOKEN` lifts through the same `mat_slot=` pipeline as member materials.
Concrete tokens like `@gravel` work without a registry pack; abstract tokens like `@path.gravel`
need the pack's materials catalog and surface `W_ABSTRACT_TOKEN_DEFERRED` or
`E_UNKNOWN_ABSTRACT_TOKEN` on a miss.

**Output.** Each `connect` row writes one `.nbt` named after its site and ports
(`hamlet_walkway_home1_entry__home2_entry.nbt`) and records a `walkways:` entry in the lockfile with
the world origin, dims, and resolved path material.

**Diagnostics.**

| Code | Cause |
|---|---|
| `E_CONNECT_ARITY` | The row's shape is not `FROM.PORT to TO.PORT`. Enforced before resolution, since an unreadable endpoint costs the row its walkway. |
| `E_UNRESOLVED_PORT` | The right-of-dot port id does not name a member of the referenced def. Carries a nearest-match note. |
| `E_AMBIGUOUS_PORT` | The def exposes the same `id=` on more than one member. Rename the collision. |
| `E_MISSING_PATH_MATERIAL` | The row omits `path=`, so walkway lowering has nothing to lay. |
| `E_UNRESOLVED_PLACE_REF` | The head place id does not name a prior place in this site (shared with §9.3.3). |
| `W_WALKWAY_BLOCKED` | No unobstructed route exists; the row falls back to the straight L and the rest of the strip still lays. |
| `W_DUPLICATE_WALKWAY` | The same `(from, to)` port pair is already laid in this site; the duplicate row is dropped. |
