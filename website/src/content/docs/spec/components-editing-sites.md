---
title: "9. Components, Editing, and Multi-building"
---

## 9.1 `def`, the component construct

`def` defines a slot-bearing Component, unified with `theme` and `site` by the same mechanism, so
the reference system does not fracture across editing, theming, and multi-building.

Parameterization (variable size and so on) is allowed; recursion is forbidden. A `def` may declare
`requires version>=X`, and the minimum version of a composite is the max of its parts
([Versioning and Editions](/spec/versioning-editions/)).

Until a parameter mechanism is specified, the header vocabulary is closed. A `def` header takes
`size=`, which lowering reads, and `class=`, which no pass reads yet, and a `struct` header takes
the same two. Any other key on either is refused as `E_UNKNOWN_ARGUMENT`, and a header `class=` is
reported as `W_IGNORED_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)), so the sample
below carries that warning.

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

Editing at the level of a concept, such as "make only the second-floor windows arched", must be
possible without breaking the whole. Edit diffs look only at `intent_state`
([Blockstate Model](/spec/blockstate/)), so a change in derived results does not harm edit stability.

## 9.3 Multi-building with `site`

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

`east` advances along `+x` and `north` retreats along `-z`. This matches "front is `+z`" from
[§5.4](/spec/syntax/#54-selectors): a building whose `front` faces south sits with its facade on `+z`, and
`north_of=X` puts the next placement behind it.

The Y axis is unaffected by topological selectors; every placement currently lands at `y = 0`.

### 9.3.2 Origin selectors

Each `place` carries **exactly one** of `at`, `east_of`, `north_of`. In the formulas below,
`prior` is the placement `ID` names and `new` is the one being placed. An origin is a placement's
low-`x`, low-`z` corner, and `dims` is its full extent: the `size=WxH` footprint plus the roof's
`overhang=` columns on each side, the same inflated extent the lockfile records.

| Selector | Effect | Notes |
|---|---|---|
| `at=origin` | Anchors at world `(0, 0, 0)`. | The only legal `at=` value. The first `place` in a site must use it, since there is no implicit default. |
| `east_of=ID gap=N` | New origin = `(prior.x + prior.dims.x + N, prior.y, prior.z)`. | `ID` must name a place declared earlier in the same `site`. `gap` is in blocks between the two facing bounding-box faces (each the wall plus its `overhang=` columns; `0` → the boxes touch), defaulting to `0`. |
| `north_of=ID gap=N` | New origin = `(prior.x, prior.y, prior.z − new.dims.z − N)`. | Same `ID` and `gap` rules as `east_of`. Because an origin is the low-`z` corner, the step back is the new placement's own depth: that is what puts its `+z` face `N` blocks from the prior's `−z` face whichever of the two is deeper. |

Combining selectors, or using `at=` with anything other than `origin`, is
`E_INVALID_PLACE_ORIGIN`.

`gap=` belongs to the two relative selectors. An `at=origin` row is anchored absolutely and reads
no distance, so a `gap=` written beside it is read by nothing and reported as `W_IGNORED_ARGUMENT`
([Lint §11.3](/spec/lint/#113-error-vs-warning)) — the argument is real, and which of the two the author
meant is theirs to say.

On a relative row, `gap=` takes an integer. Any other value (`gap=wide`, `gap="4"`) is an
unreadable value: the row is placed as `gap=0` places it, and the value is reported as
`W_IGNORED_ARGUMENT`. An origin is recorded as a 32-bit signed coordinate, so a row whose origin
works out past that range is not placed at all and is reported as `W_DEFERRED_MEMBER`, as is every
row placed relative to it. It is refused rather than clamped to the edge: a clamped origin is not
the one the source asks for, and two rows clamped to one edge land on one coordinate. The refused
row still reports what its `def` body raises — an `E_INCOMPATIBLE_MATERIAL` or `W_NO_THEME_BOUND`
is a defect in the `def` or the theme wherever the row lands — and an unreadable `gap=` on any row
that is not placed is reported with a note saying so.

### 9.3.3 Cross-scope references

Every `place` row declares `id=`, `use=`, and `theme=`. A row short of any of them cannot become a
placement: there is no name for its `.nbt`, no `def` to instantiate, or no theme to resolve its
`mat_slot=` members against. That is `E_INCOMPLETE_PLACE`. The message names every missing key, and
the row is dropped. A key that is present but not a label (`use=3`) is `E_TYPE_MISMATCH_LABEL`
instead.

`id=` is required rather than auto-assigned, unlike the geometry members of
[§9.2](#92-editing-model), because it is the name `east_of=` and `connect` refer to and the name its
`.nbt` is written under ([§9.3.4](#934-output-naming)).

| Code | Cause |
|---|---|
| `E_UNRESOLVED_PLACE_REF` | `use=NAME` does not name a top-level `def`. Carries a nearest-match suggestion. |
| `E_UNRESOLVED_THEME_REF` | `theme=NAME` does not name a `theme` in the same file. Carries a nearest-match suggestion. |
| `E_DUPLICATE_PLACE_ID` | Two `place` rows in one site share an `id=`. The diagnostic points back to the first declaration. |
| `W_UNUSED_DEF` | A `def` no `place use=NAME` references. Advisory, so a typo on the `use=` side does not silently produce an empty build. |

### 9.3.4 Output naming

The compiler writes one `.nbt` per `place`, named after the `id=` (`home1.nbt`, `home2.nbt`), directly
into the output directory. An id carrying `/` or `\` would name a path rather than a file, so it is
`E_INVALID_PLACE_ID` ([Lint](/spec/lint/)). The site is not part of the name, and the same
directory holds every `struct`, written under its own name, and every walkway
([§9.3.5](#935-ports-and-connect)). Two artifacts that would share a file name, compared ignoring
case, are `E_OUTPUT_NAME_COLLISION`: a `place` named after a `struct`, or one `id=` placed in two
sites, is refused rather than written over the other. The
world-space origin and the `(site, def, theme)` provenance of every placement is recorded in
`build.cairn.lock` under `placements`, so a downstream consumer can rebuild the layout without
re-running the coordinate solver.

### 9.3.5 Ports and `connect`

`connect FROM.PORT to TO.PORT path=@MATERIAL` lays a 1-block-wide walkway between two named ports on
placements within the same `site`.

**What a port is.** A port is the `(place, member_id)` pair that `PLACE.PORT` resolves to. Ports are
exposed on `door` and `window` members of the referenced `def`; stair and roof ports are reserved
for a future extension.

**Where a port sits.** One block outside the member's `side=` wall, at the placement's ground row (`place_origin.1`).
`front` / `back` / `left` / `right` map to `+z` / `-z` / `-x` / `+x` ([§9.3.1](#931-coordinate-convention)).
The wall-local offset comes from:

- a `door`'s `at=` value of `center`, `left`, or `right` ([§5.4](/spec/syntax/#54-selectors)). Numeric
  offsets are reserved.
- a `window`'s geometric centre, `offset + size.w / 2`, with an absent `offset=` read as `0` the
  way the cut reads it.

The placement's overhang shifts the port out into the overhang ring beyond the outer face. A
`window`'s authored `y=` does **not** lift the port off the ground row: the walkway is a 1-voxel
flat strip whose Y must agree with the other endpoint. A `sym=true` window contributes a single port
at the primary `offset` side. The mirrored cut still appears in the wall, but the `id=` resolves to
one coordinate.

**A port is somewhere a wall was opened.** Both port roles are openings cut through masonry, so a
placement whose walls do not reach the rows the opening needs anchors neither: a `door` needs the
row it opens at inside one course, and a `window` needs its whole rectangle inside one.

```
row 1 in one course                              # door (a port's door is a def-body member,
                                                 #       so the row it opens at is always 1)
offset + size.w ≤ wall_length                    # window, horizontal
every row of y … y + size.h - 1 in one course    # window, vertical
```

Those are the rules a port reads for itself. It also anchors only on an opening the openings pass
actually cut: a `window` deferred by an argument only the cut reads — `repeat=`, `step=`, `sym=` —
or whose own `mat_slot=` resolves to no block leaves its wall standing, and its port is refused
with it.

A `walls height=H` member under `level y=N` paints world rows `N + 1 … N + H` — one above the
level's base row, which is where that level's floor slab would go, though a level-scoped `floor` is
deferred today and the struct's own slab owns row `0`. Courses that touch merge, so `walls height=5` plus a `level y=5 walls
height=4` is one wall from row 1 to row 9 and a window spanning the seam is inside it; courses with
air between them do not, and a window hung in the gap is outside every one of them. A `door` member
of the `def` body opens at row `1`, so a placement whose only masonry is a `level y=6 walls` is a
wall the doorway never reaches — the same body's `window` is refused for the same reason.

The rows a port is judged against are the rows the placement **painted**, which is the same value
the openings pass cut against — not a second reading of the `def`. A `walls` whose `mat_slot=` does
not resolve paints nothing, so the cut is deferred and the port is refused with it; a `walls`
declared inside a `level` paints its rows, so the cut happens and the port stands on it. A port
that cannot be placed drops its row with a `W_DEFERRED_MEMBER` carrying one note per refused
endpoint, naming the one rule that endpoint broke: a role that cannot anchor a port, the `side=` or
`at=` it was written with, the window argument it lacks or wrote in the wrong shape, how far its
rectangle runs past the wall, which rows the masonry does and does not cover, or that the openings
pass did not cut it. The note points at the member's line; when the fault is `side=`, an argument or
the masonry, that line carries the member's own finding too — worded from the same reading of the
argument, so the two do not disagree about what was written. A port pushed past the coordinate range by its `place`
has no member line to point at and says so on the row alone.

**How the path runs.** A Manhattan L, x-axis leg then z-axis leg, at the two ports' shared Y. 3D
path search over staircases and multi-level walkways is out of scope by design.

When that L would cross an existing structure floor, the compiler searches the ground plane for a
detour: the shortest route around the obstacle, and among equal-length routes the one with the
fewest turns, with deterministic tie-breaking so the same source always lays the same strip.

The row falls back to the straight L, with the colliding cells skipped, only when no unobstructed
route exists at all: a port buried under another placement's floor or under a block its own
placement lays on the port cell, a fully enclosed target, or a search past the router's area cap.
The router searches the box spanning both ports and every placement floor on the walk plane, so
the cap can be reached by a distant pair of ports or by a widely spread floor plan.

That earns one `W_WALKWAY_BLOCKED` naming the concrete cause and its remedy: move the buried door
or window (or the block on its port cell), widen the gap, bring the two structures closer, or bring
the placements that stretch the box closer. `--format json` carries `data: { kind:
"walkway_blocked", skipped: N }`.

**Material.** `path=@TOKEN` lifts through the same `mat_slot=` pipeline as member materials.
Concrete tokens like `@gravel` work without a registry pack; abstract tokens like `@path.gravel`
need the pack's materials catalog and surface `W_ABSTRACT_TOKEN_DEFERRED` or
`E_UNKNOWN_ABSTRACT_TOKEN` on a miss.

**Output.** Each `connect` row that lays its walkway writes one `.nbt` named after its site and
ports (`hamlet_walkway_home1_entry__home2_entry.nbt`) and records a `walkways:` entry in the lockfile
with the world origin, dims, and resolved path material.

A row that lays no block has lost a walkway the source asked for: an endpoint whose `place` was
refused upstream (`W_DEFERRED_MEMBER`), a port that cannot be placed, an id the walkway's name
cannot carry, a path material that does not resolve, a straight L that is itself past the router's
search-area cap, or a straight L whose every cell overlaps a placement, which leaves the strip all
air. `cairn compile` then refuses the build with `E_PARTIAL_BUILD`, as it does for a lost scope, and
`cairn check --edition E --target V` refuses with it too ([Lint](/spec/lint/)). A `cairn check`
without `--target` does not lower, so it never sees this loss. The loss is named
`site::SITE::FROM ↔ TO`, with the ports as the row wrote them. A `W_DUPLICATE_WALKWAY` row loses
nothing, since the earlier row laid its pair, and two rows naming one pair that neither laid are one
loss. One mistake can cost more than one loss: a `place` whose def has no `size=` is a lost scope,
and every walkway with an endpoint on it is lost too, so each adds a note to `E_PARTIAL_BUILD` and
one to the count of scopes that did not lower.

**Diagnostics.**

| Code | Cause |
|---|---|
| `E_CONNECT_ARITY` | The row's shape is not `FROM.PORT to TO.PORT`. Enforced before resolution, since an unreadable endpoint costs the row its walkway. |
| `E_UNRESOLVED_PORT` | The right-of-dot port id does not name a member of the referenced def. Carries a nearest-match note. |
| `E_AMBIGUOUS_PORT` | The def exposes the same `id=` on more than one member. Rename the collision. |
| `E_MISSING_PATH_MATERIAL` | The row omits `path=`, so walkway lowering has nothing to lay. |
| `E_UNRESOLVED_PLACE_REF` | The head place id does not name a prior place in this site (shared with [§9.3.3](#933-cross-scope-references)). |
| `W_WALKWAY_BLOCKED` | No unobstructed route exists; the row falls back to the straight L and the rest of the strip still lays. When every cell of the straight L overlaps a placement, or the straight L alone is past the router's search-area cap, nothing is laid and the walkway is lost. |
| `W_DUPLICATE_WALKWAY` | The same `(from, to)` port pair is already laid in this site; the duplicate row is dropped. |
| `W_INVALID_WALKWAY_IDENT` | A site, place or port id cannot be carried in the walkway's name ([Lint](/spec/lint/)); the row is dropped. |
