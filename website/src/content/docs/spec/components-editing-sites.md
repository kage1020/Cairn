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
Every `place` row declares `id=`, `use=`, and `theme=`. A row short of any of them cannot become a
placement — there is no name for its `.nbt` (§9.3.4), no `def` to instantiate, or no theme to
resolve its `mat_slot=` members against — so it is `E_INCOMPLETE_PLACE` and the row is dropped from
the build. The message names every key the row is short of. A key that is present but not a label
(`use=3`) is `E_TYPE_MISMATCH_LABEL` instead: it is on the line, just not usable.

`id=` is required rather than auto-assigned, unlike the geometry members of §9.2. An auto-address
derives from parent / role / side / level / offset and names nothing outside the body it sits in; a
`place`'s `id=` is the name `east_of=` and `connect` refer to, and the name its `.nbt` is written
under (§9.3.4), so an invented one would be a name the author never wrote and cannot point at.

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

### 9.3.5 Ports and `connect`
A `connect FROM.PORT to TO.PORT path=@MATERIAL` row lays a 1-block-wide walkway between two named
ports on placements within the same `site`.

**Ports.** A port is the `(place, member_id)` pair `PLACE.PORT` resolves to. Ports are exposed on
`door` and `window` members of the referenced `def`; stair / roof ports are reserved for a future
extension. The port's world position is "one block outside the member's `side=` wall, at the
placement's ground row (`place_origin.1`)" — `front`/`back`/`left`/`right` map to `+z`/`-z`/`-x`/`+x`
(§9.3.1). The wall-local offset is taken from the door's `at=` value (one of `center`, `left`, or
`right` — see §5.4) for a `door` and from the rectangle's geometric centre
(`offset + size.w / 2`) for a `window`; either way, the placement's overhang
shifts the port out into the overhang ring beyond the structure's outer face. Numeric door
offsets (`at=N`) are reserved for a future extension. A `window` must fit both horizontally
(`offset + size.w ≤ wall_length`) and vertically: `walls height=H` fills the world rows `1 … H`
— the floor slab owns row `0` — so every row of `y … y + size.h − 1` has to be one of them
(`y ≥ 1` and `y + size.h ≤ H + 1`). A window whose
rows fall outside the wall cannot anchor a walkway either, and the row drops with a
`W_DEFERRED_MEMBER` whose notes list the door / window / reserved-role contracts in turn.
The port reads the rows the `walls` members *declare*, while the openings pass reads the rows
they will *paint*, so the two currently part company on a `walls` whose `mat_slot=` does not
resolve: the cut is deferred and the port still anchors. A
`window`'s authored `y=` does not lift the port off the ground row — the walkway is a 1-voxel-thick
flat strip whose Y must agree with the other endpoint — and a `sym=true` window contributes a
single port at the primary `offset` side (the mirrored cut still appears in the wall, but the
`id=` resolves to one coordinate).

**Path.** The walkway runs as a Manhattan L (x-axis leg, then z-axis leg) at the two ports' shared
Y — 3D path search (staircases, multi-level walkways) is intentionally out of scope so the port
surface can land in one piece. When the L would cross an existing structure floor, the compiler
searches the ground plane for a detour instead: the shortest route around the obstacle, and among
equal-length routes the one with the fewest turns, with deterministic tie-breaking so the same
source always lays the same strip (a lockfile requirement). Only when no unobstructed route exists
at all — a port buried under another placement's floor, a fully enclosed target, or a site past
the router's search-area cap — does the row fall back to the straight L with the colliding cells
skipped, earning one `W_WALKWAY_BLOCKED` warning whose note names that concrete cause and the
matching remedy (move the buried door/window, widen the placement gap, or bring the structures
closer together). The warning also carries a machine-readable
payload (`data: { kind: "walkway_blocked", skipped: N }`) in the `--format json` output so LSP
quick-fixes and CI annotators can read the skip count without re-parsing the human-readable
message — see §11.2 of `spec/lint.md`.

**Material.** The `path=@TOKEN` value lifts through the same `mat_slot=` pipeline used for member
materials — concrete tokens like `@gravel` work without a registry pack; abstract tokens like
`@path.gravel` require the pack's materials catalog and surface `W_ABSTRACT_TOKEN_DEFERRED` /
`E_UNKNOWN_ABSTRACT_TOKEN` on a miss.

**Output.** Each `connect` row writes one `.nbt` named after its site and ports (e.g.
`hamlet_walkway_home1_entry__home2_entry.nbt`) and records a `walkways:` entry in the lockfile
carrying the world origin, dims, and resolved path material — enough to rebuild the strip without
re-running the resolver.

**Diagnostics.**

- `E_CONNECT_ARITY` — the row's positional shape is not `FROM.PORT to TO.PORT` (a half is missing,
  the literal `to` keyword is missing or replaced by another token, extra positionals trail
  the row past `TO.PORT`, or an endpoint slot is not a one-dot `PLACE.PORT` reference). Anchored at
  the missing-positional cursor, the offending separator, the offending endpoint, or the run of
  trailing extras so the `file:L:C` pointer lands where the fix goes. An endpoint the resolver
  cannot read costs the row its walkway, so the shape is enforced before resolution rather than
  left to `E_UNRESOLVED_PORT`, which presumes a port id is there to resolve.
- `E_UNRESOLVED_PORT` — the right-of-dot port id does not name a member of the referenced def
  (with a nearest-match `did you mean` note when one fits the standard spell cap).
- `E_AMBIGUOUS_PORT` — the def exposes the same `id=` on more than one member; rename the
  collision so the reference is unique.
- `E_MISSING_PATH_MATERIAL` — the row omits `path=`; walkway lowering has nothing to lay.
- `E_UNRESOLVED_PLACE_REF` — the head place id (left of the dot) does not name a prior place in
  this site, shared with §9.3.3.
- `W_WALKWAY_BLOCKED` — no unobstructed route exists between the two ports; the row falls back
  to the straight L with the colliding cells skipped, and the rest of the strip still lays. The
  note names the cause the detour search hit — a buried port, an enclosed target, or the
  search-area cap — with the remedy that actually fixes it. JSON payload exposes the skip count
  as `data.skipped` so tooling does not need to re-parse the message text.
- `W_DUPLICATE_WALKWAY` — the same `(from, to)` port pair has already been laid in this site;
  the duplicate row is dropped.
