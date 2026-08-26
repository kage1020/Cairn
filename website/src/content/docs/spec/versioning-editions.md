---
title: "10. Versioning and Edition Strategy"
---

## 10.1 The target is a compile-time parameter

The target is the pair `(edition, version)`, and neither is written in the source. Only the backend
knows them ([Compilation Model](compilation)).

**Version strings are opaque labels.** A Minecraft version may be the legacy semver-ish `1.21.4` or,
from the latest release onward, date-based. Cairn does not compare version strings; it orders by
**DataVersion**, the monotonically increasing integer Mojang assigns. That keeps `since`/`until`,
Vmin/Vmax, `@requires`, and `semantic_sensitivity` boundaries working across the semver → date-based
transition.

The backend holds a "version string ↔ DataVersion" table, so `--target` accepts either spelling of
the same version. Bedrock resolves its version strings to an internal monotonic key the same way.

## 10.2 Language contract: recompile, don't transcode

> The spec does **not** guarantee NBT portability across version or edition. The only guarantee is
> "the result of compiling the same source to a given target".

The source is the blueprint; the `.nbt` is a target-pinned build output, the equivalent of a binary.
To use a build on a new version or another edition, recompile the source.

DataFixerUpper is forward-only, lossy, and incomplete, and loss is common in items, signs,
paintings, and block entities. It is a rescue tool, kept out of the language semantics.

Some residue is unsolvable and is stated rather than hidden:

- Meaning changes across versions (the cauldron split, item `tag` → `components`).
- Game behaviour not in data tables (fluids, gravity, attachment, redstone).
- Visual consistency (colour-temperature drift).
- Physics rule changes (1.21 wind charges breaking old traps).

Geometrically correct NBT is emitted; the gameplay experience is not guaranteed.

## 10.3 Backend = data tables

Two sources feed the backend, and they are kept apart.

**Machine-extracted** from the game's `--reports` / registry dumps. This is the truth about syntax
and domains: block and entity IDs, blockstate properties and their domains, item and component
schemas, DataVersion, tags. Taking the game itself as the source of truth, rather than anyone's
memory, closes the knowledge gap about new versions at its root.

**Hand-written, version-tagged constraint catalog** for what the data does not carry: attachment (a
frame cannot go on glass), gravity and support (gravel, hanging lanterns), fluid behaviour, entity
AABB, redstone. Defined once per new version, and every user benefits.

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

The canonical token is the primary key, and each token carries a per-edition mapping (id +
state_map). Versions fold with `inherits + diffs`: Java is the base, Bedrock the overriding diffs.
The hand-written semantic catalog records only the points that differ.

```yaml
"@oak_stairs":
  base: { states: { half: [bottom,top], shape: [straight,inner_left,inner_right,outer_left,outer_right] } }
  mappings:
    java:    { id: minecraft:oak_stairs, base: "1.13" }
    bedrock: { id: minecraft:oak_stairs, state_map: { half=top: {upside_down_bit: true} }, dropped_states: [shape] }
  sensitivity:
    - { edition: bedrock, kind: missing_state, state: shape, reason: "no inner/outer stair shape" }
```

## 10.4 Fail-loud and minimum-version inference

Unknown IDs, out-of-domain states, and parity gaps are hard errors. Silent substitution and implicit
dropping are forbidden. An error returns the closed set of candidates valid in the target, the
minimum version, and a suggested fix. That sends the model back to registry-derived candidates
rather than to its memory.

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

### Which registry an unknown ID is unknown to

An ID is checked against the block table for the one `(edition, version)` the compile pinned, not
against the edition as a whole. Bedrock 1.21.0 spells stone bricks `stonebrick` and 1.21.40 spells
it `stone_bricks`, so an edition-wide answer would accept both everywhere and catch neither mistake.
The tables ship in the registry pack's `blocks` component, folded with the `inherits + diffs` rule
of [§10.3](#103-backend--data-tables).

The check therefore runs on `cairn compile --target` and nowhere else. `cairn info` and `cairn
lower` do lower, but pin no version, since `info` reports across the whole range by design. They
skip the comparison rather than pick a version on the author's behalf. `cairn check` does not run
block-array lowering at all, so no lowering-stage code reaches it, `E_UNKNOWN_ABSTRACT_TOKEN`
included.

The suggested fix is a typo finder over the same table: `oak_plank` is answered with `oak_planks`.
A **rename** is not a typo. Bedrock calls Java's `light` `light_block`, six edits away, so the
message says it has no candidate rather than offering the nearest unrelated block.
Closing that gap needs a per-edition alias table the pack does not carry yet.

The same per-version scoping applies to a pack's own material mappings. An entry may carry
`overrides` naming the versions that spell it differently, which is what lets one
`@floor.stone.smooth` resolve across a range containing a rename. The `since` half is still
deferred: the tables record which IDs a version *has*, not which version first introduced one, so
the `E_VERSION_CAP` example above is an `@requires` floor rather than registry-inferred.

`def` and `theme` may declare `requires version>=X`, and the minimum version of a composite is the
max of its parts. Module-level `@requires` is implemented; the member-level form is not yet parsed.

### The declared floor is enforced

A module's `@requires` floor is the strictest of its `@requires` lines. `cairn compile --target`
below it is `E_VERSION_CAP`, reported before any artifact is prepared, so a refused build leaves no
structure file and no lock. That ordering matters: a lock records what was verified, and it must
never say `verified: true` for a target the source itself rules out.

`E_REQUIRES_CONFLICT` is **reserved**. It is defined as a declared floor contradicting the
registry-*inferred* range, and no inferred range is derived yet, because the pack carries no `since`
/ `until`. It is not a conflict between two `@requires` lines: floors compose by taking the
strictest, so their intersection is never empty. A constraint needing an upper bound, such as
`version<1.20`, is not a shape the language accepts; that is `E_INVALID_REQUIRES`.

### Ordering, and where it stops

[§10.1](#101-the-target-is-a-compile-time-parameter) makes `DataVersion` the canonical ordering key.
Version comparison today is **component-wise over dotted decimals** instead. That is a known
shortfall, not a second convention. The pack does ship a `DataVersion` table; the obstacle is its
coverage, since it names only the versions the pack was built for while a floor may name any
version.

Until a lookup can answer for an arbitrary label, a version Cairn cannot order is refused at the
directive rather than sorted wrongly later: pre-release, snapshot, and date-based labels
(`1.21.4-rc1`, `24w14a`) are not accepted in `@requires`.

Two things the convention still gets wrong within what it does accept:

- **A date-based label against a semver one.** This is the transition
  [§10.1](#101-the-target-is-a-compile-time-parameter) exists to survive, and dotted-decimal
  comparison does not survive it.
- **The two editions' numbering, compared as if it were one.** Java releases run `1.20.4 / 1.21 /
  1.21.4`, Bedrock `1.21.0 / 1.21.40 / 1.21.60`. A floor carries no edition, so
  `@requires version>=1.21.4` reads as satisfied by Bedrock `1.21.40` (`40 > 4`), certifying a build
  against a version that is below the floor in Java's numbering. Whether `@requires` is
  edition-neutral at all is an open question.

## 10.5 "Which version is it for?" has three answers

There is no single "for-version". `cairn info` reports three axes:

1. **Registry-compatible range `[Vmin, Vmax]`**: the intersection of `since`/`until` over the used
   tokens and states.
2. **Semantic-sensitive members**: cases where the ID stays valid but meaning, behaviour, or
   appearance changes. This matters more than the range: behaviour changes far more often than IDs
   disappear, so deciding Vmax from the registry alone is dangerous. The constraint catalog carries
   a `semantic_sensitivity` (boundary version + reason) separate from `since`/`until`, and a compile
   crossing one warns. Examples: the cauldron split at 1.17, wall connections going bool →
   `none/low/tall` at 1.16, the item format at 1.20.5.
3. **The verified lock target** ([§10.6](#106-provenance-and-lock)).

```text
$ cairn info build.crn --editions java,bedrock
registry compatibility:  1.21.40 .. latest
edition portability:     Java: portable: 42  degraded: 0  unsupported: 0   Bedrock: portable: 38  degraded: 3  unsupported: 1
buildable targets:       Java: none (1.20.4, 1.21, 1.21.4 all refuse)   Bedrock: 1.21.40, 1.21.60 (1.21.0 refuses)
semantic-sensitive:      yard_water(cauldron split@1.17), fence(wall conn@1.16)
```

Every version named is one the built-in packs declare. The file behind this output carries
`@requires version>=1.21.40`, which is what puts every Java target below the floor, and Bedrock
1.21.0 with them.

The four lines go to stdout; what each figure is made of goes to stderr as `note:` lines. A pipeline
reading the rows sees the same four lines every time `cairn info` runs to completion. A run that
A run that does not complete is a different case. A finding refuses the command before any row is
computed, so stdout is empty rather than short a line.

### The `edition portability` row

The row counts palette entries. An entry is `unsupported` for one of four reasons:

| Reason | The repair |
|---|---|
| The edition has no such block at all. | Change the material, or the pack's mapping for it. |
| It has the block, but Cairn has no mapping for the states the intent carries ([§10.7](#107-java--bedrock-portability)). | None yet. The mapping is Cairn's to add. |
| A state value outside the Java domain reached the state translator. | None. A pack is expected to reject it, though no pack schema can state a value domain today. |
| A state key the translator does not read reached it. | Remove the key from the source blockstate. |

The first is a question about IDs and the rest about states. Only the second can produce `degraded`:
a block that does not exist has nothing to lose detail from, and a state the translator refuses
outright is not a partial loss. The third and fourth are not portability facts at all: something
upstream let a blockstate through.

Because four different repairs hide behind one figure, each counted entry is named on stderr with
its reason. The ID case also gets a `did you mean` read the way `E_UNKNOWN_ID` reads one.
`--format json` carries them as `edition_portability[].unsupported_entries`, one element per unit of
the count, in palette order.

Both questions are asked of the *edition* rather than of a version, because this row reports across
a whole compatible range. An ID valid for only part of that range is therefore not `unsupported`, as
when Bedrock renamed `stonebrick` to `stone_bricks` at 1.21.40. Whether the version being built has
it is what `cairn compile --target` answers, as `E_UNKNOWN_ID` ([§10.4](#104-fail-loud-and-minimum-version-inference)).

### The `buildable targets` row

Counters cannot say everything: two entries can be declared by *disjoint* sets of versions and each
answer "the edition has it", leaving the row clean while no single version declares both.

`buildable targets` is the per-version answer. Per requested edition, it lists the supported
versions whose pinned lowering raises no error, with the refusing ones named beside them. It is a
set rather than a `[Vmin, Vmax]` range, because two IDs whose version sets interleave leave a gap a
range would claim.

It is derived by lowering once per supported version, the same check `cairn compile --target` runs.
It is **not** derived by intersecting the range-wide palette's ID sets, which is unsound: with no
target pinned every material takes its *default* mapping, so a token the target respells is compared
as the wrong ID.

Like the counters, this row reports and does not refuse. `cairn info` exits 0 even for a source no
supported version can build, because the build is the command that refuses it. Each refusing
version's own findings are printed under that version, so an `E_UNKNOWN_ID` never stands without the
target that raised it.

A fifth line, `recommended test targets`, belongs to this axis and answers a different question
again: which versions are worth testing against. No code path emits it yet.

## 10.6 Provenance and lock

The `.crn` carries only `@intended_targets`, a hint. `verified: true`, the DataVersion, and the
hashes exist only in the lock, written by the compiler on a successful build. They are never
hand-written.

```yaml
# build.cairn.lock (compiler-generated)
lock_schema_version: 1        # revision of this document's own schema
source_hash: sha256:...
cairn_version: 2026.06        # the Cairn release's date version (CalVer)
target: { edition: java, mc_version: 1.20.4, data_version: 3700 }
inputs: { registry_pack_hash: sha256:..., constraint_catalog_hash: sha256:... }
resolved_ir_hash: sha256:...
verified: true
member_version_sensitivity: [ { id: yard_water, reason: "cauldron split at 1.17" } ]
```

`resolved_ir_hash` is the core of reproducibility: it fixes the IR after macro expansion, default
filling, and auto-address assignment.

`lock_schema_version` leads the document so a reader can decide whether it understands the rest
before parsing it. Version `1` is the shape above, and a document omitting the key is version `1`.
A document declaring a higher version is refused rather than read as if the field names still meant
the same thing. Keys the schema does not declare are refused wherever they appear.

Recompiling for a different target shows the difference from the verified one loudly:

```text
$ cairn compile build.crn --target 1.21.4 --lock build.cairn.lock
W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189.
W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence
```

## 10.7 Java / Bedrock portability

Derivation rules are edition-specific: **`intent_state` is neutral, `resolved_state` is
per-edition**. The contract is "from the same intent, resolve the nearest legal representation per
edition", not "guarantee the same result".

```yaml
intent_state: { primitive: stairs, corner: inner_left, facing: east }   # edition-neutral
resolved_state:
  java:    { facing: east, half: bottom, shape: inner_left }
  bedrock: { weirdo_direction: 1, upside_down_bit: false }              # no shape → corners don't join
```

When a resolved difference becomes a visual or functional one, lint says so:

```text
W_INTENT_DEGRADED line 12 id=roof_corner:
  shape=inner_left cannot be resolved in Bedrock (stairs have no shape state).
  Bedrock stairs render straight; visual gaps at corners.
```

The canonical vocabulary absorbs only ID, state, and serialization differences. **Concept absence
and game-behaviour differences are not absorbed.** The cases that cannot be:

- display entities (absent on Bedrock)
- stairs `shape` (no such state on Bedrock)
- armor_stand pose
- redstone propagation
- item components ↔ Bedrock item NBT
- light block internal behaviour

### Writing an alternative

`@edition` conditionals in the semantic layer are forbidden. When an alternative is needed, work
down this hierarchy:

1. Use a closed semantic primitive (neutral). If it is not representable, fail loud with
   `E_PARITY_UNSUPPORTED`.
2. Fall back via **slot + per-edition theme**, resolving a `floating_text` slot to `text_display`
   on Java and a glowing sign on Bedrock.
3. Only at the escape-hatch layer, guard with `@edition`. Raw IDs and NBT are edition-specific by
   nature.

```
hologram id=shop_sign text="Weapon" mat_slot=floating_text   # the semantic layer is always neutral
theme shop_java:    slot floating_text -> text_display scale=2.0
theme shop_bedrock: slot floating_text -> sign glowing=true   # Bedrock fallback

@edition java    { raw_block mat=minecraft:light[level=15] at=4,3,2 }
@edition bedrock { raw_block mat=minecraft:light_block["block_light_level"=15] at=4,3,2 }
```

### The build picks the variant, not the source

`theme NAME_java` and `theme NAME_bedrock` declare two variants of the logical theme `NAME`. A
`--edition` pin binds that edition's variant, falls back to an unsuffixed `NAME`, and stops there.
Binding the *other* edition's variant would route its slot values into this edition's output, which
is the silent substitution [§10.4](#104-fail-loud-and-minimum-version-inference) forbids. When
neither exists the compile stops with `E_THEME_VARIANT_MISSING` rather than building the requested
extent out of air.

`place ... theme=NAME` names the **logical** theme and follows exactly that rule, so one site places
the same def under whichever variant the build needs. Naming a variant there
(`theme=shop_bedrock`) still resolves: the pin binds the variant it selects, and
`W_THEME_VARIANT_REBOUND` says which was bound instead. The neutral spelling is still what the
semantic layer is meant to carry.

With no `--edition` pin, nothing re-picks a variant the author named. A declared name binds
verbatim. A name written *without* a suffix resolves through the same unpinned order the
module-level pick uses. A name written *with* a suffix the module does not declare is
`E_UNRESOLVED_THEME_REF`.

### Cross-version application

Asymmetric by design:

- **Downgrade** (new-version NBT → old-version world) is a hard error. Unknown components cause
  crashes and corruption.
- **Upgrade** (old-version NBT → new-version world) is a loud warning plus a DataVersion stamp, and
  depends on DFU. It requires an explicit `--allow-cross-version`.

Not every build needs to be edition-portable. The compiler's job is to state what breaks
portability.

## 10.8 Compatibility tier of Cairn's own surfaces

The `(edition, version)` axis above covers what Cairn *emits*. The orthogonal axis is what Cairn
promises about its own evolution: `.crn` syntax, the lockfile, the CLI flags, the Rust API. CalVer
has no "major" axis to read those promises off, so they are spelled out in
[Compatibility Tiers](compatibility). A `Stable` surface gives one release of `W_DEPRECATED` lead
time; an `Evolving` surface can change in any monthly minor; `Internal` makes no promise.
