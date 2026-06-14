# 10. Versioning and Edition Strategy

## 10.1 The target is a compile-time parameter
The target is the two axes `(edition, version)`. The version/edition is not written in the DSL source
([compilation.md](compilation.md)). The only layer that knows the version/edition is the backend.

**Version strings are treated as opaque labels.** A Minecraft version may be the legacy semver-ish form
(`1.21.4`) or, from the latest release onward, **date-based**. Cairn does not parse and compare version
strings; it uses **DataVersion (the monotonically increasing integer Mojang assigns) as the canonical
ordering key**. This keeps ordering/range logic (`since/until`, Vmin/Vmax, `@requires`, the boundaries
of `semantic_sensitivity`) from breaking across the semver→date-based transition. The backend holds a
"version string ↔ DataVersion" table, so `--target` may receive either a semver or a date-based string
that resolves to the same DataVersion. (Bedrock likewise resolves its version string to an internal
monotonic key.)

## 10.2 Language contract: recompile, don't transcode

> The language spec does **not** guarantee NBT portability across version or edition. The only
> guarantee is "the result of compiling the same DSL to a given target".

- DSL = blueprint / NBT = a target-pinned build output (the equivalent of a binary). To use it on a new
  version or another edition, **recompile the DSL** rather than converting the NBT.
- DataFixerUpper (DFU) is forward-only, lossy, and incomplete (loss is common in items, signs,
  paintings, block entities). It is a **rescue tool, kept out of the language semantics**.
- State the unsolvable residue explicitly: meaning changes across versions (cauldron split, item
  `tag`→`components`), game behavior not in data tables (fluids/gravity/attachment/redstone), visual
  consistency (color-temperature drift), physics rule changes (1.21 wind charge breaking old traps).
  "Geometrically correct NBT is emitted, but the gameplay experience is not guaranteed."

## 10.3 Backend = data tables (machine-extracted + hand-written catalog)
- **Machine-extracted (the game's `--reports` / registry dumps) = the truth about syntax and domains**:
  block/entity IDs (existence checks), blockstate property/domain (validating `north=none/low/tall`),
  item/component schema, DataVersion, tags. Taking the game itself — not our or an LLM's memory — as the
  source of truth structurally solves the knowledge gap about new versions.
- **Not in the data = a hand-written, version-tagged constraint catalog** (connects to the §5.4
  constraints): attachment (a frame can't go on glass), gravity/support (gravel, hanging lantern),
  fluid behavior, entity AABB, redstone (out of the model in principle). Defined once per new version,
  every user benefits.

```yaml
constraints:
  minecraft:item_frame:
    type: entity_attachment
    since: "1.13"
    targets: { solid_full_face: true, glass_pane: false }
    error: "item_frame requires a solid attachable face"
  minecraft:lantern:
    type: support
    states:
      hanging=true:  { requires_above: solid_or_chain }
      hanging=false: { requires_below: solid_top }
```

### Folding the `(edition, version)` matrix
- Use the canonical token as the primary key; each token has a per-edition mapping (id + state_map).
- Fold versions with `inherits + diffs`. Define **Java as the base, Bedrock as overriding diffs**.
- Separate machine-extracted facts from the hand-written semantic catalog; the hand-written part only
  records the points that differ.

```yaml
"@oak_stairs":
  base: { states: { half: [bottom,top], shape: [straight,inner_left,inner_right,outer_left,outer_right] } }
  mappings:
    java:    { id: minecraft:oak_stairs, base: "1.13" }
    bedrock: { id: minecraft:oak_stairs, state_map: { half=top: {upside_down_bit: true} }, dropped_states: [shape] }
  sensitivity:
    - { edition: bedrock, kind: missing_state, state: shape, reason: "no inner/outer stair shape" }
```

## 10.4 Fail-loud + minimum-version inference
Unknown IDs, out-of-domain states, and parity gaps are **hard errors**. Silent substitution and
implicit dropping are **forbidden**. An error returns the **closed set of candidates valid in the
target** + the minimum version + a suggested DSL fix, sending the model back to registry-derived
candidates rather than its memory. This feeds the self-correction loop ([evaluation.md](evaluation.md)).

```text
E_UNKNOWN_ID line 12: "minecraft:pale_oak_planks" not in 1.21.4 registry.
  Similar valid: minecraft:oak_planks, minecraft:dark_oak_planks, minecraft:cherry_planks

E_VERSION_CAP line 7: minecraft:cherry_planks introduced in 1.20 (target 1.19.4).
  Fix: --target >=1.20, or  slot decor -> @oak_planks

E_STATE_DOMAIN line 18: wall north=true invalid for 1.21.4. Valid: none, low, tall (changed from boolean in 1.16).
  Suggested DSL: wall_segment id=yard_wall connect_north=low

E_PARITY_UNSUPPORTED line 8: text_display is Java-only (since 1.19.4); Bedrock has no display entity.
  Suggested: sign side=front text="Inn", or slot+theme fallback, or @edition java guard
```

`def` / `theme` may declare `requires version>=X`; the minimum version of a composite is the max of its
parts.

## 10.5 "Which version is it for?" = three axes
There is no single "for-version". `cairn info` returns three axes:

1. **registry-compatible range [Vmin, Vmax]**: the compiler derives the intersection of `since/until`
   over the used tokens/states.
2. **semantic-sensitive members (most important)**: **semantic drift** where the ID stays valid but
   meaning/behavior/appearance changes. `since/until` only sees "is the ID valid". Behavior changes far
   more often than IDs disappear, so deciding Vmax from the registry alone is dangerous. The constraint
   catalog carries a `semantic_sensitivity` (boundary version + reason) separate from `since/until`,
   and emits a warning when a compile crosses it (e.g. cauldron split @1.17, wall connection
   bool→none/low/tall @1.16, item format @1.20.5).
3. **verified lock target** (10.6).

```text
$ cairn info build.crn --editions java,bedrock
registry compatibility:  Java: 1.20.0 .. latest   Bedrock: 1.21.30 .. latest
edition portability:     portable: 42  degraded: 3  unsupported: 1
semantic-sensitive:      yard_water(cauldron split@1.17), fence(wall conn@1.16)
recommended test targets: Java min 1.20.0 / latest 1.21.4
```

## 10.6 Provenance and lock (reproducibility)
- The `.crn` carries only `@intended_targets` (wish/hint). **`verified:true` + DataVersion + the hashes
  exist only in the lock, written by the compiler on a successful build** (users/LLMs do not hand-write
  them).
- Minimal sufficient set for the lock: `source_hash` / `cairn_version` (the Cairn release's CalVer; see
  [README](README.md)) / `target(mc_version + data_version)` / `registry_pack_hash` /
  `constraint_catalog_hash` / **`resolved_ir_hash`** (the core of reproducibility: fixes the IR after
  macro expansion, default filling, and auto-address assignment).

```yaml
# build.cairn.lock (compiler-generated)
source_hash: sha256:...
cairn_version: 2026.06        # the Cairn release's date version (CalVer)
target: { edition: java, mc_version: 1.20.4, data_version: 3700 }
inputs: { registry_pack_hash: sha256:..., constraint_catalog_hash: sha256:... }
resolved_ir_hash: sha256:...
verified: true
member_version_sensitivity: [ { id: yard_water, reason: "cauldron split at 1.17" } ]
```

Recompiling for a different target shows the difference from the verified one as a loud warning:

```text
$ cairn compile build.crn --target 1.21.4 --lock build.cairn.lock
W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/3955.
W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence
```

## 10.7 Java / Bedrock portability
- Derivation rules are edition-specific: **intent_state is neutral, resolved_state is per-edition**.
  The contract is "from the same intent, resolve the nearest legal representation per edition", not
  "guarantee the same result".

```yaml
intent_state: { primitive: stairs, corner: inner_left, facing: east }   # edition-neutral
resolved_state:
  java:    { facing: east, half: bottom, shape: inner_left }
  bedrock: { weirdo_direction: 1, upside_down_bit: false }              # no shape → corners don't join
```

When a resolved difference becomes a visual/functional difference, lint notifies:

```text
W_INTENT_DEGRADED line 12 id=roof_corner:
  shape=inner_left cannot be resolved in Bedrock (stairs have no shape state).
  Bedrock stairs render straight; visual gaps at corners.
```

- The canonical vocabulary can absorb only ID/state/serialization differences. **Concept absence and
  game-behavior differences are not absorbed.** Representative cases that cannot be absorbed: display
  entities (absent on Bedrock), stairs shape (no state on Bedrock), armor_stand pose, redstone
  propagation, item components↔Bedrock item NBT, light block internal behavior.

- **`@edition` conditionals in the semantic layer are forbidden.** When an alternative is needed, use
  this hierarchy:
  1. Closed semantic primitive (neutral) → if not representable, fail-loud (`E_PARITY_UNSUPPORTED`).
  2. **Fallback via slot + per-edition theme** (resolve a `floating_text` slot to `text_display` on
     Java, a glowing sign on Bedrock).
  3. `@edition` guards only at the escape-hatch layer (raw IDs/nbt are inherently edition-specific).

```
hologram id=shop_sign text="Weapon" mat_slot=floating_text   # the semantic layer is always neutral
theme shop_java:    slot floating_text -> text_display scale=2.0
theme shop_bedrock: slot floating_text -> sign glowing=true   # Bedrock fallback

@edition java    { raw_block mat=minecraft:light[level=15] at=4,3,2 }
@edition bedrock { raw_block mat=minecraft:light_block["block_light_level"=15] at=4,3,2 }
```

- Cross-version application is asymmetric: **downgrade (new-version NBT → old-version world) = hard
  error** (unknown components cause crashes/corruption). **Upgrade (old-version NBT → new-version world)
  = loud warning + DataVersion stamp + DFU dependence** (only with explicit `--allow-cross-version`).
  Not every build needs to be edition-portable; the compiler states what breaks portability.
