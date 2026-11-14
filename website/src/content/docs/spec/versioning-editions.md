---
title: "10. Versioning and Edition Strategy"
---

## 10.1 The target is a compile-time parameter

The target is the pair `(edition, version)`, and neither is written in the source. Only the backend
knows them ([Compilation Model](/spec/compilation/)).

**Version strings are opaque labels.** A Minecraft version may be the legacy semver-ish `1.21.4` or,
from the latest release onward, date-based. Cairn does not compare version strings; it orders by
**DataVersion**, the monotonically increasing integer Mojang assigns. That keeps `since`/`until`,
Vmin/Vmax, `@requires`, and `semantic_sensitivity` boundaries working across the semver → date-based
transition.

The backend holds a "version string ↔ DataVersion" table, so `--target` accepts either spelling of
the same version. Bedrock has no DataVersion, so its table keys each release by the release's own
numbers, packed into one monotonic integer. The block-state version a Bedrock structure carries is
not that key, since releases can share one, as `1.21.60` and `1.26.60` do.
Two labels are the same version when they differ only in ways that carry no information: a missing
trailing component is zero, and a leading zero in a numeric component is padding rather than a
digit. So `1.21`, `1.21.0` and `1.021` are one version (`1.21.0` is Java's `1.21`, and `1.21` is
Bedrock's `1.21.0`). `@requires` and `@intended_targets` read a label by the same rule. The build
and its lockfile name the version as the table spells it.

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

Geometrically correct NBT is emitted; the gameplay experience is not guaranteed. Where the
constraint catalog knows the game will drop a block, let it fall or let water run out, a pinned
build warns ([Lint](/spec/lint/#constraint-checks)), and where it knows a member's meaning changes at
a version, `cairn info` and the lock name the member ([§10.5](#the-semantic-sensitive-row)). Neither
is a guarantee: the catalog lists what it knows.

## 10.3 Backend = data tables

Two sources feed the backend, and they are kept apart.

**Machine-extracted** from the game's `--reports` / registry dumps. This is the truth about syntax
and domains: block and entity IDs, blockstate properties and their domains, item and component
schemas, DataVersion, tags. Taking the game itself as the source of truth, rather than anyone's
memory, closes the knowledge gap about new versions at its root. Of that list the built-in packs
carry each version's block IDs, each block's state properties with their domains, and the version
table. The block tables are extracted: Java's from the server jar's `--reports`, Bedrock's from the
release's block palette, every block state the game accepts. Each `pack.json` names the file every
table was extracted from and that file's hash, and rerunning the extraction against them is a check.

**Hand-written constraint catalog** for what the data does not carry: attachment (a lever cannot
hang on a glass pane), gravity and support (gravel, hanging lanterns), fluid behaviour, entity AABB,
redstone, and the versions at which a block's meaning changes. Defined once, and every user
benefits. The built-in catalog ships beside the packs, one file for both editions in canonical IDs.
Its rules hold at every version that declares their blocks, and read each version's faces from the
pack; a meaning that changes at a version is tagged with that version. Its tables, and the checks
that read them, are [Lint §11.4](/spec/lint/#114-constraint-catalog)'s. A lock records the sha256 of
its text ([§10.6](#106-provenance-and-lock)).

```json
"support": [
  {"ids": ["lantern"], "when": {"hanging": "false"}, "defaults": {"hanging": "false"},
   "on": "below", "needs": "center"},
  {"ids": ["lantern"], "when": {"hanging": "true"}, "defaults": {"hanging": "false"},
   "on": "above", "needs": "center"}
]
```

### Folding the `(edition, version)` matrix

The canonical token is the primary key. An abstract token maps to a block ID through the
edition's `materials`, a canonical token names its own, and the canonical states, which are Java's,
map to the edition's own through a state map. Java is the base, so its states need no map. The
hand-written semantic catalog records only the points that differ.

Each edition has a registry pack of its own, whose `pack.json` names its `edition`, and versions
fold with `inherits + diffs` inside one pack only. The two packs do not fold onto each other,
because they answer different questions: Java's tables are the canonical vocabulary itself, and
Bedrock's state maps translate *from* it. A Bedrock version's map reads the states of the Java
release its source maps from, which need not be any version of the Java pack, so a Bedrock table
cannot be written as a diff over a Java one. Inside one pack the `blocks` component holds one
version's blocks, each with the values every state property takes. Each later version inherits the
one listed before it and names it, with the blocks it adds, the blocks whose states it changes, and
the IDs it removes (Java `1.20.4` → `1.21` → `1.21.4` → `26.3`, Bedrock `1.21.0` → `1.21.40` →
`1.21.60` → `1.26.60`):

```json
{ "mc_version": "26.3", "inherits": "1.21.4",
  "added": { "iron_chain": { "axis": ["x", "y", "z"], "waterlogged": ["true", "false"] } },
  "removed": ["chain"] }
```

An ID's `since` and `until`, the first and last carried version that declares it, follow from that
chain rather than being written beside it: `pale_oak_planks` is Java's from 1.21.4 on, and `chain`
Java's until 1.21.4. A version between the two may still lack the ID, when one diff removes it and
a later one adds it back. Java values are strings, as the game's reports give them; Bedrock's are
booleans, integers and strings, as its block palette types them. A `materials` entry maps a token to
one block ID, and its `overrides` name the versions that spell it differently, as Bedrock's does for
1.21.0:

```json
{ "token": "floor.stone.smooth", "block": "stone_bricks",
  "overrides": [{ "mc_version": "1.21.0", "block": "stonebrick" }] }
```

An entry whose block the edition spells for several Java blocks also names, as `canonical`, the
Java block the token stands for. Bedrock's one `bed` is every dyed bed, and its writer saves the
colour of the bed `canonical` names in the bed's block entity ([Entities
§8.1](/spec/entities/#81-two-tiers)):

```json
{ "token": "bed.blue", "block": "bed", "canonical": "blue_bed" }
```

The Bedrock pack carries its state maps as two more components. `states` is generated from
GeyserMC's mappings, one pinned per version, and checked against that version's block palette when
the pack is extracted and again when it is loaded. The Java states of one block that become one
Bedrock block form a **family**, and each Bedrock state a family writes is a constant or a lookup
over the fewest Java properties that decide it. `ids` names the pairs a row maps, as the Bedrock ID,
then `=` and the Java ID when the two are spelled apart (`oak_double_slab=oak_slab`):

```json
{ "mc_versions": ["1.26.60"], "ids": ["acacia_stairs", "andesite_stairs", "..."],
  "java": { "facing": ["north", "south", "west", "east"], "half": ["top", "bottom"],
            "shape": ["straight", "inner_left", "inner_right", "outer_left", "outer_right"],
            "waterlogged": ["true", "false"] },
  "defaults": { "facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false" },
  "states": {
    "minecraft:corner": { "reads": ["shape"],
                          "map": { "inner_left": "inner_left", "...": "...", "straight": "none" } },
    "upside_down_bit": { "reads": ["half"], "map": { "bottom": false, "top": true } },
    "weirdo_direction": { "reads": ["facing"],
                          "map": { "east": 0, "north": 3, "south": 2, "west": 1 } } },
  "dropped": [] }
```

A palette entry is read by its ID. An ID a row pairs with itself names its own Java block, and the
entry reads as that block's family. Any other ID is a Bedrock spelling, and the entry reads as the
family, among the Java blocks spelled that way, that takes its states. An ID that is both reads as
its own Java block unless the entry carries a state that block does not take: Bedrock spells Java's
`redstone_wall_torch` as `redstone_torch` too, so there `redstone_torch[facing=north,lit=true]` is
the wall torch and `redstone_torch[lit=true]` the standing one. The states such an ID takes are
those of every block it reads as.

The same stairs at 1.21.60 have no `minecraft:corner`, so that row lists `shape` under `dropped`: a
property the family holds at more than one value that no Bedrock state reads. `waterlogged` never
drops, because the `.mcstructure` carries it as the water layer. A family writes every state its
Bedrock block's palette declares, and a Bedrock state that is no function of the Java states fails
the extraction. A version's `refused` list names the pairs whose GeyserMC states its palette does
not declare, so the pack carries no form for them there, not even the bare block. `in_water` names
the Bedrock blocks GeyserMC always places in water (`kelp`, `seagrass`, `bubble_column` and their
kin), whose Java blocks have no `waterlogged`.

What dropping a property costs is not in GeyserMC's mappings, so `sensitivity` is written by hand,
one row per property and the blocks it covers, or every other block that drops it. A `missing_state`
loses a value other than the Java default (a stair's `shape` up to 1.21.60), and a `derived` one
loses nothing the lowering derived, because Bedrock works it out itself (a fence's connections), but
loses at any value one a member's material or line wrote ([Blockstate
§6.1](/spec/blockstate/#connections)). A `block_entity` one loses nothing, because Bedrock keeps it
in the block's block entity, which the writer fills from it: a chest's `type`, as the pairing of its
two halves, and a note block's `note` ([Entities §8.1](/spec/entities/#81-two-tiers)). A
`block_entity` row the writer has no block entity for is a defect in the pack, and the writer
refuses each entry whose value it would have to keep there. A row's `reason` is the sentence
`W_INTENT_DEGRADED` prints where the property is lost ([§10.7](#107-java--bedrock-portability)).
The loader refuses a dropped property no row classifies, and the extractor a row nothing drops.

Both packs carry a `connections` component: what each block is to the rules that join panes, fences,
walls, chests and redstone dust to their neighbours ([Blockstate
§6.1](/spec/blockstate/#connections)). It is read off the Java release each version's states follow,
the version itself on Java and the release its state map follows on Bedrock, so `ids` names
canonical blocks in both. The release's block tags say which blocks are walls, fences and fence
gates, which fences are wooden, and which raise a wall's post, the tags of the feature packs its jar
ships included. A pane is a block with a pane's states that is no fence, a chest is a block whose
`type` is `single`, `left` or `right`, dust is the one block the game names `redstone_wire`, and the
blocks never joined by a face are the ones the game names in code. Which faces dust beside a block
joins is the game's code too: a repeater's two along its facing, an observer's back when that is a
side, and every side of a block that gives off a signal from any side, the `buttons` and
`pressure_plates` tags' and the rest by name. Which faces of each state are sturdy, and which parts
of a wall below its bottom covers, are read off the block models the release's client jar ships,
each a lookup over the fewest properties that decide it where it varies:

```json
{ "mc_versions": ["1.20.4", "1.21", "1.21.4", "26.3"], "ids": ["acacia_slab", "..."],
  "defaults": { "type": "bottom" },
  "sturdy": { "reads": ["type"],
              "map": { "bottom": ["down"], "double": ["down", "up", "north", "south", "west", "east"],
                       "top": ["up"] } },
  "covers": { "reads": ["type"],
              "map": { "bottom": ["post", "north", "east", "south", "west"],
                       "double": ["post", "north", "east", "south", "west"], "top": [] } } }
```

A row's `kind` is `pane`, `wall`, `fence`, `wooden_fence`, `fence_gate`, `chest` or `redstone_dust`,
`never_joined_by_face` marks a block a pane, fence or wall never joins by a face, `raises_post` one
in the `wall_post_override` tag, and `defaults` gives the default of each state the rules read,
which a state that leaves it out takes, a fence gate's `facing` and `open` and a chest's `facing`
among them. `covers` names `post` and the sides' directions, and a row with a `kind` has none, since
the rules read a connecting block by its connection states. `dust` names the sideways faces dust
beside the block joins, as a list or a lookup like `sturdy`'s, a repeater's reading its `facing`; a
row without it is joined by no dust. A block no row names is nothing to the rules. Every target
postdates 1.16, where wall connections turned from booleans into `none` / `low` / `tall`
([§10.5](#105-which-version-is-it-for-has-three-answers)), so a wall's sides are derived as those
three values at every target.

The loader refuses a component with no rows, a row that names no block or no version, a lookup that
reads no property or maps no value, a default no rule reads, a fence gate whose default `facing` is
no sideways face or whose `open` is no boolean, a chest whose default `facing` is no sideways face,
and a `dust` face that is no sideways face, since dust lies on the block under it. On Java it also
refuses a row naming a block, or a value of a state, that the version's block table does not give.

## 10.4 Fail-loud and minimum-version inference

Unknown IDs, out-of-domain states, and parity gaps are hard errors. Silent substitution and implicit
dropping are forbidden. An error returns the closed set of candidates valid in the target, the
minimum version, and a suggested fix. That sends the model back to registry-derived candidates
rather than to its memory.

```text
E_UNKNOWN_ID line 4: "minecraft:pale_oak_planks" not in java 1.21 registry.
  First target that declares it: 1.21.4. Fix: --target 1.21.4, or @requires java version>=1.21.4

E_VERSION_CAP line 2: this file requires version>=1.21.4 (target 1.21).
  Valid java targets: 1.21.4. Fix: --target 1.21.4, or lower the @requires floor

E_STATE_DOMAIN line 18: minecraft:creaking_heart has no state `active` at java 26.3. Valid: axis, creaking_heart_state, natural.
  `active=true` is valid at 1.21.4. Fix: --target 1.21.4, or correct the state literal

E_PARITY_UNSUPPORTED line 2: minecraft:border_block is a block only Bedrock has, so java 26.3 cannot build it.
  Bedrock uses it to wall players in; Java has no such block.
  Fix: bind the slot from theme NAME_bedrock and give NAME_java a Java block, or raw_block under @edition bedrock
```

### A state is checked against its block's states at the target

Every property and value of a block state a member resolves to is checked against the states its
block takes at the pinned version, and one outside them is `E_STATE_DOMAIN` ([Lint](/spec/lint/)).
On Java those are the `blocks` table's. On Bedrock they are the canonical vocabulary the version's
state map reads ([Folding the matrix](#folding-the-edition-version-matrix)), since a palette entry
carries Java's states until the writer maps them; an ID no Java block maps onto there, or one whose
states the version's palette refuses, has none, and a state on it is `E_STATE_UNMAPPED` instead. The
check runs where the ID check runs and nowhere else
([below](#which-registry-an-unknown-id-is-unknown-to)): a lowering with no version pinned leaves
both to a build that pins one.

A state literal ([Syntax](/spec/syntax/)) is checked at its token, and the member or walkway that
binds it is not built. A state a member's own line writes, such as a `note_block`'s `instrument=`
([Blockstate Model §6.1](/spec/blockstate/#61-derive-by-default-promote-on-override)), is checked
the same way at its value. The finding names the pair, the values the property takes, or the
properties the block has when it has no such property, and the other versions whose tables take the
pair: a domain that changed between releases is the likeliest cause of a literal that is valid
somewhere, and the repair is then a target rather than an edit. Lowering checks the states it
derives as well (a stair's `facing`, `half` and `shape`), at the member, so a derivation the
target's block cannot hold is refused rather than written; that is a defect in Cairn or the pack's
table, and the finding says so.

A literal on a block the pinned target's pack records no states for is written as given, with
`W_STATE_LITERAL_UNCHECKED` ([Lint](/spec/lint/)) so that it is said rather than silent. Both
built-in packs record every block's states, so only an external pack can leave one unrecorded.

### Which registry an unknown ID is unknown to

An ID is checked against the block table for the one `(edition, version)` the compile pinned, not
against the edition as a whole. Bedrock 1.21.0 spells stone bricks `stonebrick` and 1.21.40 spells
it `stone_bricks`, so an edition-wide answer would accept both everywhere and catch neither mistake.
The tables ship in the registry pack's `blocks` component, folded with the `inherits + diffs` rule
of [§10.3](#103-backend--data-tables).

The check therefore runs where a version is pinned and nowhere else: on `cairn compile --target`,
on `cairn check --edition E --target V`, which pins the same pair to run the same pass without
writing anything, and in the `buildable targets` row of `cairn info`, which pins each supported
version in turn. A `compile` whose target the file's floors refuse pins none: it checks no id and
is refused on the floor instead ([§10.4](#the-declared-floor-is-enforced)), while `check --target`
is not held to the floors and pins the target regardless. The `buildable targets` row reports each
version's `E_UNKNOWN_ID` under that version and leaves the exit code alone
([§10.5](#the-buildable-targets-row)). The other rows of `info` report across the whole range by
design, and `cairn lower` lowers with no target, so neither pins a version: they skip the
comparison rather than pick one on the author's behalf. A `cairn check` with no `--target` lowers
with no target at all, which pins no version and leaves each abstract token to the pack of the
target that builds it, so neither this check, `E_UNKNOWN_ABSTRACT_TOKEN` nor
`W_ABSTRACT_TOKEN_DEFERRED` reaches it ([Lint §11.3](/spec/lint/#113-error-vs-warning)).

Checking against *every* version the edition ships and refusing only the ids valid in none of them
would need no flag, and would answer a different question: `stone_bricks` is valid somewhere on
Bedrock, so a build pinned to 1.21.0 would still be told nothing. The pin is what makes the answer
true of the build being made.

The suggested fix depends on which of four ways a wrong ID arrived. A **typo** is answered by a
distance search over the same table: `oak_plank` is answered with `oak_planks`. A **rename** is not
a typo. Bedrock spells Java's `light` as `light_block_0` … `light_block_15`, eight edits away, and
no threshold that keeps the typo finder honest will ever connect the two. Renames are answered from
the registry pack's `aliases` component instead, and only where the pack has a row; where it does
not, and no other version of the edition declares the ID, the message says it has no candidate
rather than offering the nearest unrelated block.

An `aliases` row is a **group of spellings**: the names one block has worn, across editions and
across one edition's own range, including the several IDs one old spelling split into.

```json
{ "spellings": ["light", "light_block", "light_block_0", "light_block_1"] }
```

Nothing in the row says which spelling belongs to which `(edition, version)`. The `blocks` component
already knows that, per version, so a lookup is "take the group, keep the members the pinned target
declares". That is what lets one set of rows answer Java → Bedrock (`oak_sign` → `standing_sign`)
and Bedrock 1.21.0 → 1.21.40 (`stonebrick` → `stone_bricks`) alike. An answer is the closed set
§10.4 asks for, never a pick from it: `@light` on Bedrock 1.21.60 is answered with all sixteen light
levels, because choosing one would be the silent substitution this section forbids.

The third kind is a real block at the wrong version: one a later release adds, or one a later
release drops. The block tables answer it, so the error names the targets of the edition nearest
the pinned one on either side that declare the ID: the first after it and the last before it. They
are the versions the pack carries a table for, not the releases that changed the block: the
built-in Java tables carry 1.20.4, 1.21, 1.21.4 and 26.3, so `@leaf_litter`, which 1.21.5 added,
is first declared at 26.3 as far as they say. The fix offered is a `--target` among them. For an ID
the author named that no earlier target declares, it also offers an `@requires <edition>
version>=` floor at the release that added it, with the first target as that release's upper
bound, so a target without the block is refused at the floor instead; an ID the pack chose is the
pack's to fix and gets no floor. `cairn check --target` is not held to the floors, so it keeps
reporting the ID at an older target once the floor is declared, where `cairn compile` refuses the
target at the floor. Such an ID is no typo, so no distance search runs for it: `@pale_oak_planks`
on Java 1.21 is answered with 1.21.4, not with `dark_oak_planks`. An alias answer still stands
beside it, and comes first, since both are true: `@stone_bricks` on Bedrock 1.21.0 is answered with
both `stonebrick`, the spelling 1.21.0 declares, and 1.21.40, the first target that declares
`stone_bricks`.

The fourth is a block only the other edition has, which the pinned edition's pack lists as
`absent` ([§10.7](#a-block-only-the-other-edition-has)). It is not a typo, a rename or a version,
so it gets none of these answers: its note gives the reason the pack writes for it, and the fix is
a block this edition has, or a build for the other edition.

What the key cannot express is a spelling both editions declare meaning different blocks: Bedrock's
`snow` is Java's `snow_block` while Java's `snow` is Bedrock's `snow_layer`. Such a pair gets no
row, and the typo search still runs behind it.

The same per-version scoping applies to a pack's own material mappings. An entry may carry
`overrides` naming the versions that spell it differently, which is what lets one
`@floor.stone.smooth` resolve across a range containing a rename. The first target that declares
an ID is named by an error and nowhere else: no floor is inferred from it, so the `E_VERSION_CAP`
example above is an `@requires` floor the file declares rather than one the registry inferred.

### A part may declare its own floor

`def` and `theme` may declare `requires version>=X` on a line of their own, and the minimum version
of a composite is the max of its parts:

```
def cottage size=9x7:
  requires version>=1.21.4
  walls mat_slot=wall height=4
```

The expression is the one `@requires` takes, edition scope and all. The two spellings differ in what
they constrain, not in what they say. A module-level `@requires` is a floor on the *file*; this one
is a floor on the *part*, so a `place use=cottage` inherits it and a library of templates carries
its own requirements instead of every consumer restating them.

**Which parts a build inherits from.** A `def` a `place use=` names, and a `theme` a scope binds. A
part nothing instantiates contributes nothing: a `def` no `place` names builds no voxels (and is
already `W_UNUSED_DEF`), so refusing a target over it would be refusing over a template the author
left in the file.

**A theme's floor applies when the theme is bound**, whether or not a member reads a slot from it.
Binding a theme is the act of taking on what it declares. Charging the floor only once one of its
rules fires would make the floor depend on which selectors matched and which variant the pin picked,
so one source could require `1.21` on Java and nothing on Bedrock for a reason that is not about
editions. It would also err in the unsafe direction. An over-applied floor is reported against the
line that set it and is one edit away, while an under-applied one certifies a build the file itself
rules out.

Two things bind a theme, and both of them are a scope a build lowers. One is a `place ...
theme=NAME` reference, which also instantiates the `def` it places, since `theme=` is required on a
`place`. The other is the module-level auto-pick, read for a `struct`, the one scope a build lowers
without a placement. The auto-pick also binds the sole theme to every `def` scope, and a floor does
*not* follow it there: a `def` no `place` names builds nothing, so charging a theme's floor because
such a `def` exists would read one `def` as instantiated enough to take on a theme's floor and not
instantiated enough to be charged its own. A `def` that is placed reaches the theme through its
placement's own `theme=` instead.

`struct` and `site` take no such line. Neither is instantiated by anything, since each *is* the
build, so a floor written inside one constrains exactly the file it is in, which is what `@requires`
already says. The same goes for a member's own indented children: the floor belongs to the part, and
a `walls` line is not a part. That is three refusals with three messages for two repairs: one points
at `@requires`, the other at a dedent. A member's children inside a `def` get the dedent; inside a
`struct` or a `site` a dedent lands the line in a body that refuses it too, so they get `@requires`,
as the `struct` or `site` itself does. A floor is recognised by its expression before anything after
the line is read, so one with an indented line under it is refused as a floor rather than read as a
member. None of the three refuses on the word alone: `requires` is an ordinary keyword in a body
that reads no floors, so a member line spelled that way parses in a `struct`, a `site`, or under a
member like any other member line.

A `def` or `theme` body is the other half of that, and takes the word whatever follows it: the line
is a floor, and an expression that reads as none is `E_INVALID_REQUIRES` rather than a member. That
costs nothing: `requires` is not a member keyword, so the same line would otherwise be
`E_UNKNOWN_KEYWORD`.

`E_VERSION_CAP` names the part that imposed the floor, not only the number. A target refused by a
floor written inside a template is not actionable as a bare version, because the repair is at the
other end of the `place use=` that inherited it.

### The declared floor is enforced

A module's floors compose by intersection. `cairn compile --target` is held to every `@requires`
line that applies to the build, and a target below any of them is `E_VERSION_CAP`. The refusal comes
before any artifact is prepared, so a refused build leaves no structure file and no lock. That
ordering matters: a lock records what was verified, and it must never say `verified: true` for a
target the source itself rules out.

A target below a floor usually also lacks a block the file uses, since a block introduced after the
target is the common reason to declare one. So `cairn compile` pins no version for a target the
floors refuse: one below a floor, and every target when a floor names a version this edition's
table cannot place ([§10.4](#ordering-is-by-dataversion-per-edition)). Any id the source names goes
unchecked, including one no version declares, until the target clears every floor. The build is
refused on the floor instead, with `E_VERSION_CAP` or `E_REQUIRES_UNORDERABLE`, and no
`E_UNKNOWN_ID` tells the author to replace the block the floor was declared for. What needs no
version is still reported ahead of that refusal: the check pass's findings, such as
`E_INVALID_REQUIRES`, and lowering's, such as `E_UNKNOWN_ABSTRACT_TOKEN`. An error among them stops
the build before the refusal is reached. `cairn check --edition E --target V` is not held to the
floors, so at a target they refuse it reports the ids that target lacks, where `compile` reports
the floor ([Lint](/spec/lint/#materials-and-targets)).

### The hint is weighed against the floor

`@intended_targets` ([§5.3](/spec/syntax/#53-headers)) is a wish rather than a verification record,
and the floor above it is a constraint. A file may state both in a way that cannot hold, such as
`@requires version>=1.21` beside `@intended_targets ["1.20.4"]`. If only the floor were checked, one
of them would decide a build and the other would not. That is the worst arrangement, because the
header that reads like an instruction would be the ignored one.

So each version the header names is placed in the target edition's table and answered in one of
three ways:

| The version | The finding |
|---|---|
| No `--target` of the edition names it: a release the pack ships no block data for (`1.19` on Java), or a label the table cannot place at all (`1.21.40` on Java) | `W_INTENDED_TARGET_UNSUPPORTED` |
| A version the edition builds, below a floor the file declares | `E_INTENDED_TARGET_CAP` when *every buildable* one is, `W_INTENDED_TARGET_CAP` when only some are |
| A version the edition builds, at or above every floor | Nothing |

The order is deliberate. A version the compiler cannot build is reported as that whatever the floors
say, because `--target 1.19` not existing is what the author acts on. A cap beside it would ask them
to edit a floor that is not what stops the build. The split between the two cap codes is reach
rather than kind. "Nothing the file says it is for can be built" is the strongest reading a
contradiction between two declarations gets, and no author meant it. A list that reaches past the
floor at one end is a wish stated too widely, and what it names above the floor still builds.

**"Every" counts the versions the edition can build.** A name no `--target` of it carries answers
for none of the list, so `@intended_targets ["1.20.4", "1.19"]` under `version>=1.21` is the first
case and not the second: Java builds exactly one of the two, and the floor refuses it. Counting
`1.19` would let a version that was never buildable report a file nothing can build as half a
problem. It is reported separately as the unsupported case, which is where its own repair is.

Which floors count is the composite fold [§10.4](#a-part-may-declare-its-own-floor) already defines:
the file's `@requires` lines plus every part the build instantiates. So a `def` in a library can
refuse the intent of the file that places it, and the finding names the part. A floor scoped to the
other edition is inert here as everywhere, and a floor this edition's table cannot place refuses
nothing: that is `E_REQUIRES_UNORDERABLE`, whose repair is on the `requires` line rather than on the
intent.

Every command that gates on `cairn check` (`check`, `info`, `lower`, `compile`, `synth`) reports the
two cap codes, and each weighs the header in the tables of the editions it is about: the one
`--edition` names, the ones `cairn info --editions` lists, or both where the command names none. A
finding either edition reaches is reported, since the contradiction is between two lines of the file
however it is later built. One span carries one cap finding: two editions disagreeing about how far
it reaches report the error, because one of them finding part of the list still buildable does not
make the other's "none of it is" less true.

`W_INTENDED_TARGET_UNSUPPORTED` waits until exactly one edition is in scope, because a version Java
cannot build is routinely the Bedrock target the author means. `cairn info --editions bedrock` is
one edition in scope, and is weighed in Bedrock's table alone. A report scoped to one edition is not
refused by the other's answer.

`E_REQUIRES_CONFLICT` is **reserved**. It is defined as a declared floor contradicting the
registry-*inferred* range, and no inferred range is derived yet from the `since` / `until` the packs
carry. It is not a conflict between two `@requires` lines: floors compose by taking the
strictest, so their intersection is never empty. A constraint needing an upper bound, such as
`version<1.20`, is not a shape the language accepts; that is `E_INVALID_REQUIRES`.

### Ordering is by DataVersion, per edition

[§10.1](#101-the-target-is-a-compile-time-parameter) makes `DataVersion` the canonical ordering key,
and `@requires` uses it. A floor is placed in the **target edition's** version table
(`registry-data/{java,bedrock}/data_versions.json`) and weighed against the target's own key in
it: its `DataVersion` on Java, and on Bedrock the integer the table packs from the release's
numbers.

That table names the **release lines** of its edition that it carries, which is a different set
from the versions the pack can *build* for, the ones it ships block and material data for. A row says which it is (`targetable`). Java's rows are its releases from `1.14` on.
Bedrock's are its release lines, numbered in tens (`1.21.0`, `1.21.20`, `1.21.40`), without the
hotfixes between them. Keeping the two sets apart is what lets "inside the table's span, naming no
row" mean "no release this table can order": a floor of `1.21.1` is a Java release the pack cannot
build for and can order perfectly well, while `1.21.4` names no Bedrock row, and neither does the
hotfix `1.21.42`.

A floor may still name something no table carries, so placing one is not a bare lookup. Four
answers, and only the first is exact:

| The floor | Placed as | Because |
|---|---|---|
| Names a row (a missing trailing component is zero, and a leading zero in a numeric component is padding: `1.21` is Bedrock's `1.21.0`, `1.020` is Java's `1.20`) | That row's key | Exact. |
| Names a pre-release of a row (`1.21.4-rc1`) | That row's key | Nothing ships between a release candidate and its release, so no supported target lies between them either. |
| Sits below every row, or above every one | Met by every target, or by none | Reached by comparing the floor's label against the first and last rows' *labels*, while which rows those are is decided by their *keys*, so it holds exactly when the table's labels sort the same way by text as by key. The registry pack loader checks that at load time. The floor's own label must be a dotted decimal to be compared at all. |
| Anything else: inside the table's span, naming no row | Not placed at all | It has no key, and there is none to give it. `E_REQUIRES_UNORDERABLE`. |

The last row is a refusal, not a guess. `@requires version>=1.21.4` against Bedrock is exactly it:
Java's release names no Bedrock release and sits between `1.21.0` and `1.21.20`. Comparing the
labels would read it as satisfied on `40 > 4` and certify a Bedrock build against a version below
the floor: the same defect enforcing the floor exists to remove, one edition to the left.

The refusal can say more than "no". A label this edition cannot place but the *other* edition
names (a row of its table, or the pre-release of one) is a floor written in the other's numbering,
and `E_REQUIRES_UNORDERABLE` names it and offers the scope. A label the other edition does not name
gets no scope offered, because recommending one would be recommending a guess: scoped to an edition
that does not name it either, the floor goes inert there and the constraint disappears. That covers
a label neither edition can place, such as a snapshot, and also one the other edition places only
below or above every row. Those two placements are comparisons rather than releases: they say
nothing about which numbering the author meant, and scoped there the floor is met by every target
of that edition or by none.

### A floor may name its edition

Java releases run `1.20.4 / 1.21 / 1.21.4 / 26.3` and Bedrock `1.21.0 / 1.21.40 / 1.21.60 /
1.26.60`. The two are different scales, and a floor written in one of them means nothing in the
other. So a floor may say which it is written in:

```
@requires java    version>=1.21.4
@requires bedrock version>=1.21.40
```

A **scoped** floor constrains its own edition's build and is inert in the other's. Inert is not
violated, so the pair above builds on both. An **unscoped** floor is a floor on whatever is being
built, and is resolved in that edition's table like any other. That makes `@requires version>=1.21`
a floor both editions can honour (Java's `1.21`, Bedrock's `1.21.0`), and makes a floor that names
one edition's release and not the other's the error above rather than a silent pass.

The `registry compatibility` row of `cairn info` ([§10.5](#the-registry-compatibility-row))
reads only the unscoped floors. It is one row for a file that may be reported against both editions
at once, and a floor in Java's numbering says nothing about the file's Bedrock range; the
per-edition answer is the `buildable targets` row.

A floor a `theme` declares is held to the same test, and for the *part* it is inherited through
rather than for the words on the line. Per-edition theme variants
([§10.7](#107-java--bedrock-portability)) mean the two editions can bind different themes for one
`theme=` reference, so a theme feeds this row only when both bind the same one; otherwise the floor
is a per-edition fact wearing no edition scope. A `def` needs no such test: a `place use=NAME` names
one def and not a family of variants. Whatever the row leaves out is named on stderr with the
reason, so a `0.0` beside a `buildable targets` row that refuses versions is never left to be
inferred.

### Which labels a floor may use

Every label shape [§10.1](#101-the-target-is-a-compile-time-parameter) says will exist is accepted
by the directive: the semver-ish `1.21.4`, the pre-release `1.21.4-rc1`, a snapshot `24w14a`, and
whatever a date-based scheme spells. The shape rule is dot-separated components that each begin
with a digit and carry only letters and digits, with an optional `-` and a pre-release tag of the
same. `1.a` and `x` name no version in any scheme and are `E_INVALID_REQUIRES`.

Accepting a label is not claiming it can be ordered. Whether a given label has a `DataVersion` is
the table's answer, and it is asked per edition: `cairn compile --target` refuses the build, and
`cairn info --editions` reports the edition as having no buildable target and says why. `cairn
check` does not ask: it never weighs a floor against its target, even with `--edition` and
`--target` pinned ([§10.4](#the-declared-floor-is-enforced)).

## 10.5 "Which version is it for?" has three answers

There is no single "for-version". `cairn info` reports three axes:

1. **Declared registry range `[Vmin, Vmax]`**: the floors the file declares without naming an
   edition, composed, against an open upper edge. A reading of what the source and the parts it
   instantiates declare, not a fact derived from the blocks they use. See [the `registry
   compatibility` row](#the-registry-compatibility-row).
2. **Semantic-sensitive members**: cases where the ID stays valid but meaning, behaviour, or
   appearance changes. This matters more than the range: behaviour changes far more often than IDs
   disappear, so deciding Vmax from the registry alone is dangerous. The constraint catalog carries
   a `semantic_sensitivity` (boundary version + reason) separate from `since`/`until`, and a compile
   crossing one warns. Examples: the cauldron split at 1.17, wall connections going bool →
   `none/low/tall` at 1.16, the item format at 1.20.5, the three the built-in catalog ships. See
   [the `semantic-sensitive` row](#the-semantic-sensitive-row).
3. **The verified lock target** ([§10.6](#106-provenance-and-lock)). `cairn info` does not report
   it yet, because it reads no lockfile. `cairn compile` does compare a lock's target with the one
   it builds, and reports a change as `W_PREVIOUSLY_VERIFIED_TARGET`.

```text
$ cairn info build.crn --editions java,bedrock
registry compatibility:  1.21.40 .. latest
edition portability:     Java: portable: 42  degraded: 0  unsupported: 0   Bedrock: portable: 38  degraded: 3  unsupported: 1
buildable targets:       Java: none (1.20.4, 1.21, 1.21.4, 26.3 all refuse)   Bedrock: 1.21.40, 1.21.60, 1.26.60 (1.21.0 refuses)
intended targets:        1.21.40, 1.21.60
semantic-sensitive:      Java: yard_water(cauldron split @1.17), yard_wall(wall connections @1.16)   Bedrock: (none)
```

Every version named is one the built-in packs declare. The file behind this output carries
`@requires version>=1.21.40`, which is what puts every Java target below the floor, and Bedrock
1.21.0 with them. The `semantic-sensitive` row names Java members alone, since the built-in
catalog's boundaries are all Java's.

`intended targets` is the file's own `@intended_targets` line, verbatim, and the one row that is a
declaration rather than an answer. It sits beside `buildable targets` because that is the row it can
contradict. [§10.4](#the-hint-is-weighed-against-the-floor) automates the comparison for the half
that is decidable, and the reader makes it for the rest. A file declaring none gets `(none
declared)` rather than a missing row.

The five lines go to stdout; what each figure is made of goes to stderr as `note:` lines. A pipeline
reading the rows sees the same five lines every time `cairn info` runs to completion. A run that
does not complete is a different case: a finding refuses the command before any row is computed, so
stdout is empty rather than short a line.

### The `registry compatibility` row

The row reads declarations back; it never looks at the blocks the source uses. `Vmin` is the
strictest floor the file is bound by with no edition named, or `0.0` when there is none. The
candidates are the `@requires version>=X` headers, plus the `requires version>=X` line of every
`def` and `theme` the build instantiates that feeds this row
([§10.4](#a-part-may-declare-its-own-floor)); a theme feeds it only when both editions bind the same
one. Working out *which* parts a build instantiates is real work over the source; reading which
blocks they paint is not part of it.

"Strictest" is a comparison of the labels, so it is exact only while they are all dotted decimals. A
label the comparison cannot read as a number sorts above every one it can, which is fixed rather
than meaningful: a file declaring both `version>=1.21.4` and `version>=24w14a` reports the snapshot.
That order decides no build: every gate that does weighs each floor against the target edition's
table separately ([§10.4](#ordering-is-by-dataversion-per-edition)).

The row is edition-agnostic, which is why it reads only the unscoped floors, for the reasons and
with the stderr notes [§10.4](#a-floor-may-name-its-edition) gives. So `0.0` has two causes, a file
that declares no floor and one whose every floor names an edition, and the row cannot tell them
apart. The note on stderr can.

`Vmax` is the literal `latest`. An upper edge is the half of a *derived* range and this row carries
the declaration, so the range the packs' `since` / `until` imply would give its answer to
`buildable targets` rather than fill this in.

**The row is a declaration, not an answer about which versions build.** Those are easy to read as
one thing, and a source can make them look identical. This one declares a floor every supported
version clears, and uses a block Java gained in 1.21.4:

```text
$ cat hut.crn
@cairn 2026.06
@requires version>=1.20.4

theme pale:
  slot floor -> @pale_moss_block
  slot wall  -> @cobblestone

struct hut size=5x5
  floor mat_slot=floor
  walls mat_slot=wall height=3

$ cairn info hut.crn --editions java
registry compatibility:  1.20.4 .. latest
edition portability:     Java: portable: 2  degraded: 0  unsupported: 0
buildable targets:       Java: 1.21.4, 26.3 (1.20.4, 1.21 refuse)
intended targets:        (none declared)
semantic-sensitive:      Java: (none)
```

`1.20.4 .. latest` reads as an answer, and the row two below disproves it, with `1.20.4` itself in
the refusal list. The declared floor is not wrong; it is answering a different question.
`minecraft:pale_moss_block` arriving in 1.21.4 is a fact about the pack, and this row reads only
what the file declares.

**The derivation is the `buildable targets` row**, two rows down. It weighs the source against every
supported version and reports which ones a build would accept, per edition. That is the answer an
intersection of `since` / `until` was meant to approximate, reached by asking rather than by
inferring. It is also the shape the intersection could not take: the answer is per edition, and it
need not be contiguous, so a `[Vmin, Vmax]` pair would have to claim a gap it cannot see.

The declared floor stays a row of its own because it is a different kind of fact: an **input** the
author wrote, which bounds what `cairn compile --target` accepts and what the file promises a
reader, where `buildable targets` is an **output** about the packs that happen to ship.
`E_REQUIRES_CONFLICT` ([§10.4](#104-fail-loud-and-minimum-version-inference)) is reserved for the
day the two can be compared, when a declared floor could contradict a registry-*inferred* range. It
stays unreachable until a range is inferred from the packs' `since` / `until`.

### The `edition portability` row

The row counts palette entries. An entry is `unsupported` for one of two reasons:

| Reason | The repair |
|---|---|
| The edition has no such block at all. | Change the material, or the pack's mapping for it. |
| It has the block, but no version's state map has a form for the states the intent carries ([§10.7](#107-java--bedrock-portability)). | Bind the slot to another block, or build for the other edition. |

The first is a question about IDs and the second about states. Only the second can produce
`degraded`: a block that does not exist has nothing to lose detail from.

Because two different repairs hide behind one figure, each counted entry is named on stderr with its
reason. The ID case is answered by the two halves `E_UNKNOWN_ID` gives an ID no other version of the
edition declares: the `aliases` component where it has a row, so an entry this edition has under
another name is reported as that name rather than as a dead end (`standing_sign` on Java is
`oak_sign`), and a `did you mean` where it has none. They are alternatives, not a sequence: a row is
the pack's word about which block this is, and printing a distance guess beside it would ask the
reader to choose between them. An entry the edition's `absent` component lists is answered with that
row's reason instead, and neither half runs: the block is the other edition's, so the repair is a
block this edition has or a build for the other ([§10.7](#a-block-only-the-other-edition-has)). Its
JSON `reason` is `no_counterpart`, with the sentence as `detail`. The alias question is asked of the
edition here too: a spelling *some* supported version declares is an answer, where a pinned build
would keep only its own version's. `--format json` carries them as
`edition_portability[].unsupported_entries`, one element per unit of the count, in palette order.

Both questions are asked of the *edition* rather than of a version, because this row reports across
a whole compatible range. An ID valid for only part of that range is therefore not `unsupported`, as
when Bedrock renamed `stonebrick` to `stone_bricks` at 1.21.40. Whether the version being built has
it is what a pinned target answers, as `E_UNKNOWN_ID`, from `cairn compile --target` or `cairn check
--edition E --target V` for the same answer without a build
([§10.4](#104-fail-loud-and-minimum-version-inference)). States are asked the same way, of every
version that declares the block: `cherry_wood[axis=x]` is portable because three versions write it,
although 1.21.40's palette has no form for it, and a build pinned there answers `E_STATE_UNMAPPED`.

#### Which entries degraded, and what they lost

`degraded` is the other figure over entries the command can name, and it is named the same way. The
case is weaker than `unsupported`'s: an entry degrades only where a version drops one of its states,
and the repair is the same each time, a version that keeps it or a material that does not need it,
so a reader is not choosing between repairs. What is identical is "which of the N": `roof-hip`
reports `degraded: 4`, and the only other place that is answered is the build, as
`W_INTENT_DEGRADED`, which is the run this command exists to be read *before*.

Each counted entry is named on stderr and carried in `--format json` as
`edition_portability[].degraded_entries`, one element per unit of the count, in palette order. An
entry is `{id, states, dropped}`:

| Field | Carries |
|---|---|
| `id` | The palette entry's block ID, verbatim as the lowering interned it. |
| `states` | The entry's `key=value` pairs, comma-joined, the same spelling the `states_unmapped` reason uses. |
| `dropped` | One `{key, value, mc_versions}` per intent some version has no form for: the canonical property, the value asked for, and the versions that lose it, oldest first. `key` is a property the pack's `sensitivity` classifies as `missing_state`, or as `derived` where a member wrote it. |

Two lists rather than one under a category tag, and `states` rather than the ID alone. Degradation
is a fact about the *state combination*, not about the block: one ID reaches this list once per
combination that loses something, and `roof-hip`'s four entries are four spellings of
`minecraft:spruce_stairs`. A list keyed by the ID alone would print the same line four times. The
build's `W_INTENT_DEGRADED` names each entry the same way, once per line, as `id[states]` after
the structure's scope, so each warning is distinct and matches the note about the same entry.

`dropped` carries the property and the value rather than the sentence about them, for the reason the
`unsupported` reasons do: a consumer that reads this should not have to parse English to learn which
state was lost. A value the edition *can* express is not a loss and does not appear: a dropped
property at its Java default loses nothing, so `shape=straight` drops without an entry, and neither
does a `derived` one the lowering derived. One a member's material or line wrote is lost at any
value, since Bedrock works it out again from the blocks around it. The prose both the note and
`W_INTENT_DEGRADED` print is the `sensitivity` row's `reason`, written per `key` and the blocks it
covers rather than over the pair, because the sentence for a dropped `shape` talks about stairs and
the one for a shulker box's `facing` about its block entity; for a written `derived` one it follows
``the written `key=value` is not kept:``. The note adds the versions that lose it; the build names
its own target already.

#### A blockstate the pack should have refused is not a figure

Two further failures can reach the state translator: a property the Java block does not have, and a
value outside its domain (`facing=up` on a stair). Neither is an answer about the edition. No
validated registry pack produces either, so each comes from a state literal in the source, which
`E_STATE_DOMAIN` refuses at every pinned target but the range-wide lowering this row reads does not
check ([§10.4](#a-state-is-checked-against-its-blocks-states-at-the-target)), or from a defect in
the pack or in Cairn. A value one version's map holds is no such leak at a version whose map lacks
it, as `instrument=trumpet` is not before 1.26.60: it is canonical vocabulary the older map has no
form for, and counts there like any other state without one.

So they are not a third and fourth reason for `unsupported`. `cairn info` reports **no** portability
figures for an edition whose palette carries one. The counts would still be computable, and they
would read as ordinary portability. A `facing=up` counted as `unsupported: 1` is indistinguishable
from a state no Bedrock version has a form for, which is the one conclusion the reader must not
draw. The command names every such entry on stderr with the translator's own message, whose fix is
to the source blockstate, notes that each is either a state literal in the source or a leak in the
pack or Cairn, and exits non-zero without a row, the way any other finding that refuses the command
does.

The state translator is the only place this can be observed, so the rule is stated for the states
question alone. It is not a licence to answer `unsupported` for a validation gap elsewhere: a figure
computed over a palette a validated pack could not have produced is not a portability answer,
whatever produced it.

### The `buildable targets` row

Counters cannot say everything: two entries can be declared by *disjoint* sets of versions and each
answer "the edition has it", leaving the row clean while no single version declares both.

`buildable targets` is the per-version answer. Per requested edition, it lists the supported
versions whose pinned lowering raises no error, with the refusing ones named beside them. It is a
set rather than a `[Vmin, Vmax]` range, because two IDs whose version sets interleave leave a gap a
range would claim.

It is derived by lowering once per supported version, the same check `cairn compile --target` runs.
An entry the `edition portability` row counts `unsupported` for its states is one the edition's
writer cannot carry, so it refuses every version of that edition here as it refuses the compile. An
entry only some versions' maps carry refuses the others alone, through their pinned lowering's
`E_STATE_UNMAPPED`. It
is **not** derived by intersecting the range-wide palette's ID sets, which is unsound: with no
target pinned every material takes its *default* mapping, so a token the target respells is
compared as the wrong ID.

Like the counters, this row reports and does not refuse. `cairn info` exits 0 even for a source no
supported version can build, because the build is the command that refuses it. Each refusing
version's own findings are printed under that version, so an `E_UNKNOWN_ID` never stands without the
target that raised it.

#### Why the list is empty

An empty `buildable` has five causes, and they are not all repaired by the same edit: the
`@requires` line answers two of them, the member that produced no voxels a third, the material the
writer cannot carry a fourth, and whatever the pinned lowering named the fifth. The list alone
cannot tell them apart, so under `--format json` a `reason` accompanies it. The key is **absent**
whenever a version builds, so an ordinary report is unchanged.

`reason` is an object, and every cause that holds is reported. Three of the five are facts about
the **edition**, identical under every release and settled before any release is weighed, so they
are its own fields. The other two differ between releases and sit in a per-version list beside them.
Each field is omitted when it carries nothing.

| Field | Carries | What the author edits |
|---|---|---|
| `unplaceable_floors` | Floors | The `@requires` line. The floor names no release of this edition, so no version can be weighed against it and none is certified. |
| `dropped_scopes` | Scope keys, and `site::SITE::FROM ↔ TO` for a walkway | The member or `connect` row that produced no voxels. It refuses every version before its ID table is consulted, since a partial build is not certified. |
| `states_unmapped` | Palette entries, as `id[states]` | The theme slot bound to that material. The edition's writer cannot carry those states, so it refuses the build at every version. |
| `versions` | Refused targets | One entry per version that refused for a reason of its own. |

Beside rather than instead: a file can declare a floor this edition cannot place *and* use an ID
one of its releases has never had. A shape that reported the first alone would send the author back
for the second after the repair, one cause per run, which is the loop this row exists to close.

`versions` is a **subsequence** of `considered`, not a parallel array. A release with nothing
against it but an edition-wide answer above contributes no entry, so a consumer joins on `version`
rather than by position. Each entry carries its `version` and a `refusal` tag:

| `refusal` | Carries | What the author edits |
|---|---|---|
| `below_floor` | `floors` | The `@requires` line. Only the floors that refuse *this* version: a file declaring several is a file where the repair is one line rather than all of them. |
| `lowering_refused` | `findings` | Whatever the findings name, usually a material or an ID, and it differs per version. |

The two are exclusive, because a version below a floor is never lowered: a floor is a relation
between the source and the target and no ID table changes it.

A floor is `{declared, line, col, declared_by?}`: the floor as the author wrote it, scope and all,
and where it is written. `declared_by` is `{keyword, name}` with `keyword` one of `def` or `theme`,
for a floor a build inherited from a part; it is absent for a floor on the file itself, because the
position already points at the line and the line is the file's.

`findings` are rendered the way [Lint §11.2](/spec/lint/#112-machine-readable-payload) renders a
finding, and are the same findings the run prints under that version on stderr. That section is
unchanged by this row: these ride inside the report rather than in the `{"diagnostics": [ ... ]}`
document, and a report is not a refusal.

The text rows say none of this. `buildable targets: Bedrock: none (1.21.0, 1.21.40, 1.21.60,
1.26.60 all refuse)` is true for every one of the five causes. Each cause is reported on stderr as
well, though not in the same shape: the two floor causes print a `note:` carrying the position of
the line, a dropped scope prints a `note:` naming the scope and no position, an entry the writer
cannot carry prints a `note:` naming the entry, and a refused lowering prints the findings
themselves under the version, each with its own position. Only the JSON lacks them, and `reason`
fills that gap.

A fifth line, `recommended test targets`, belongs to this axis and answers a different question
again: which versions are worth testing against. No code path emits it yet.

### The `semantic-sensitive` row

The row answers the second axis per requested edition, as `Java: <members>   Bedrock: <members>`:
each member as `name(reason @boundary)`, or `(none)` where the edition has none. A member is listed
when a block it builds is one a `semantic_sensitivity` row of that edition names ([Lint
§11.4](/spec/lint/#114-constraint-catalog)), once for each such row, in the order of the members. Its
name is its `id=`, or its keyword where it carries none. The blocks are the ones the range-wide
lowering builds, which the `edition portability` row reads too, so a Bedrock block is read as every
canonical block a version's state map makes of it. `--format json` carries the row as
`semantic_sensitive`, one `{edition, member, reason, boundary_version}` per entry, edition by
edition in the order requested.

The row lists what may change, not where it does. A member is listed whichever side of its boundary
the versions it builds for lie: the row reads the source, and which versions that source is built
for is the other rows' answer. The boundaries are the edition's own versions, so a member is never
listed under an edition for another's boundary; the built-in catalog's are all Java's.

A compile records its target's own answer in the lock's `member_version_sensitivity`, reading the
blocks as that version does, and warns on a recompile for another version of the edition whose
boundary lies between the two ([§10.6](#106-provenance-and-lock)).

## 10.6 Provenance and lock

The `.crn` carries the language version it is written against (`@cairn`) and, about targets, only
`@intended_targets`, a hint. `verified: true`, the DataVersion, and the hashes exist only in the
lock, written by the compiler on a successful build. They are never hand-written.

```yaml
# build.crn.lock (compiler-generated)
lock_schema_version: 1        # revision of this document's own schema
source_hash: sha256:...
cairn_version: 2026.10.0      # the Cairn release that wrote it, as `cairn --version` prints it
target: { edition: java, mc_version: 1.20.4, data_version: 3700 }
inputs: { registry_pack_hash: sha256:..., constraint_catalog_hash: sha256:... }
resolved_ir_hash: sha256:...
verified: true
member_version_sensitivity:
  - { id: yard_water, reason: "cauldron split at 1.17" }
  - { id: pantry, reason: "item format at 1.20.5" }
```

A compile writes the lock next to its source as `<source>.lock` unless `--lock` names another
path. `constraint_catalog_hash` is the sha256 of the constraint catalog's text
([§10.3](#103-backend--data-tables)). `member_version_sensitivity` lists what [the
`semantic-sensitive` row](#the-semantic-sensitive-row) finds for the target's edition, reading the
blocks as the target does, and each `reason` is the row's reason and boundary as `<reason> at
<boundary>`.

`resolved_ir_hash` is the core of reproducibility: it fixes the IR after macro expansion, default
filling, and auto-address assignment. Auto-address assignment is not implemented yet: a member has
an address only where it carries `id=` ([§9.2](/spec/components-editing-sites/#92-editing-model)).

`lock_schema_version` leads the document so a reader can decide whether it understands the rest
before parsing it. Version `1` is the shape above plus two keys written only when they are
non-empty: `placements`, each `place` row's resolved origin and extent, and `walkways`, each
`connect` row's resolved strip and path material. A document omitting `lock_schema_version` is
version `1`. A document declaring a higher version is refused rather than read as if the field
names still meant the same thing. Keys the schema does not declare are refused wherever they
appear.

Recompiling for a different target shows the difference from the verified one loudly:

```text
$ cairn compile build.crn --edition java --target 1.21.4 --lock build.crn.lock
W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189.
W_SEMANTIC_SENSITIVITY: 1 member may resolve differently: pantry (item format at 1.20.5)
```

The second warning names the members of this build whose boundary lies between the version the lock
verified and the target, each with the boundaries it crosses. 1.20.4 and 1.21.4 lie either side of
1.20.5 alone, so `yard_water`'s 1.17 is not named. It reads this build rather than the list the lock
recorded: that list carries no boundary to weigh, and may name members the source no longer builds.
Across editions it names no member, since two editions' versions are not ordered against each
other. A lock whose version the edition's table cannot place has every sensitive member named, since
where it stood cannot be told.

## 10.7 Java / Bedrock portability

Derivation rules are edition-specific: **`intent_state` is neutral, and the blocks it resolves to
differ by edition**. The contract is "from the same intent, resolve the nearest legal representation
per edition", not "guarantee the same result".

```yaml
intent_state: { facing: east, shape: inner_left }                       # edition-neutral
written:
  java:            { facing: east, half: bottom, shape: inner_left }
  bedrock 1.21.60: { weirdo_direction: 0, upside_down_bit: false }      # no shape → corners don't join
  bedrock 1.26.60: { minecraft:corner: inner_left, weirdo_direction: 0, upside_down_bit: false }
```

Lowering records each member's `resolved_state` for the target it lowers at
([Blockstate Model §6.2](/spec/blockstate/#62-intent_state-and-resolved_state)): its block IDs are
the ones that target resolved, a theme's edition variant included, and its state names and values
are the canonical vocabulary's, which are Java's
([Materials and Themes §7.2](/spec/materials-themes/#72-canonical-vocabulary)). The Bedrock rows
are what the Bedrock writer maps those states to as it writes, through the pack's state map for the
target version ([Folding the matrix](#folding-the-edition-version-matrix)), so the record never
shows them. A property the entry leaves out takes its Java block's default. A waterlogged entry
holds water in the `.mcstructure`'s second layer, and so does a block the map lists `in_water`, such
as `kelp`, which Bedrock keeps only under water.

When a resolved difference becomes a visual or functional one, the build says so. `themed-tower`'s
eave stair is `shape=outer_left`, which Bedrock 1.21.60 has no state for:

```text
$ cairn compile examples/themed-tower.crn --edition bedrock --target 1.21.60
warning[W_INTENT_DEGRADED]: struct::keep: minecraft:dark_oak_stairs[facing=south,half=top,shape=outer_left]: this Bedrock version has no stair `shape` state; its stairs render straight, so corners show visual gaps
```

A state the target's map has no form for at all, rather than one it drops, refuses the build with
`E_STATE_UNMAPPED` at the member or walkway that built it ([Lint](/spec/lint/)): writing it bare
would silently become the block's default. A pair the version's `refused` list names is refused
this way even written bare, and the note names the versions whose maps carry the states. A value
only another version's map holds, as `noteblock[instrument=trumpet]` is before 1.26.60, whose maps
follow Java releases without it, is outside the block's states there, so it is `E_STATE_DOMAIN`
naming those versions instead ([§10.4](#a-state-is-checked-against-its-blocks-states-at-the-target)).

The canonical vocabulary absorbs only ID, state, and serialization differences. **Concept absence
and game-behaviour differences are not absorbed.** The cases that cannot be:

- blocks only one edition has ([below](#a-block-only-the-other-edition-has))
- display entities (absent on Bedrock)
- stairs `shape` on Bedrock before 1.26.60 (no such state there)
- armor_stand pose
- redstone propagation
- item components ↔ Bedrock item NBT
- light block internal behaviour

### A block only the other edition has

Some blocks of one edition have no counterpart in the other under any ID: Bedrock's world-building
blocks (`allow`, `deny`, `border_block`), its Education Edition blocks and chemistry, and its
item-frame blocks, which Java places only as entities, and Java's test-framework blocks
(`test_block`, `test_instance_block`). They are not renames, which `aliases` answers where it has a
row ([§10.4](#which-registry-an-unknown-id-is-unknown-to)), and the block tables cannot tell them
from one: Java's `snow_block` is Bedrock's `snow`, and Java's `comparator` is Bedrock's
`powered_comparator` and `unpowered_comparator`, so a diff of the two tables would call each of
them absent. Each pack therefore lists the other edition's blocks it lacks by hand, in an `absent`
component, each row with the sentence a diagnostic prints for it:

```json
{ "ids": ["border_block"],
  "reason": "the border block, which Bedrock uses to wall players in; Java has no such block" }
```

The loader refuses an `absent` ID that the pack's own `blocks` declare at some version, under that
spelling or another of its `aliases` group, since the edition then has the block.

A semantic member whose `mat_slot=` resolves to such a block, and a `connect … path=` that does, is
`E_PARITY_UNSUPPORTED` ([Lint](/spec/lint/)) at the token, at a pinned target of the edition that
lacks it. The block is no typo, so no nearest ID is offered. The notes give the pack's reason and
what to write instead. For a member, that is a step down the hierarchy
[below](#writing-an-alternative): a per-edition theme that binds the slot to a block each edition
has, or a `raw_block` under the other edition's `@edition` guard. Neither is open to a walkway,
since `path=` names its block with no slot to bind and nothing a guard holds builds in a `site`
body ([§10.7](#an-edition-guard-holds-the-escape-hatch)). So for a `path=`, the note asks for a
block both editions have, or an abstract token each edition's materials catalog maps.

A `raw_block` or `raw_fill` that names such a block is `E_UNKNOWN_ID`, since a raw ID is
edition-specific by nature, with a note giving the pack's reason and naming the guard that would
keep the line. An ID the pack's own mapping reached stays `E_UNKNOWN_ID` too, with the reason and
no guard: the mapping is the pack's to fix, and no line of the source is.

Like `E_UNKNOWN_ID`, the finding needs a pinned target
([§10.4](#which-registry-an-unknown-id-is-unknown-to)). `cairn info`'s `edition portability` row
answers an entry the edition's `absent` lists with the same reason
([§10.5](#the-edition-portability-row)).

### Writing an alternative

**Subject to change.** `hologram` and `slot` rules that take arguments are not built yet
([Compatibility Tiers](/spec/compatibility/#subject-to-change)).

`@edition` conditionals in the semantic layer are forbidden. When an alternative is needed, work
down this hierarchy:

1. Use a closed semantic primitive (neutral). If it is not representable, fail loud with
   `E_PARITY_UNSUPPORTED`.
2. Fall back via **slot + per-edition theme**, resolving a `floating_text` slot to `text_display`
   on Java and a glowing sign on Bedrock.
3. Only at the escape-hatch layer, guard with `@edition`. Raw IDs and NBT are edition-specific by
   nature.

```
@requires bedrock version>=1.21.40   # the Bedrock branch below is spelled for the flattened ids

theme shop_java:    slot floating_text -> text_display scale=2.0
theme shop_bedrock: slot floating_text -> sign glowing=true   # Bedrock fallback

struct shop size=9x7
  hologram id=shop_sign text="Weapon" mat_slot=floating_text   # the semantic layer is always neutral
  @edition java
    raw_block mat=minecraft:light[level=15] at=4,3,2
  @edition bedrock
    raw_block mat=minecraft:light_block_15 at=4,3,2
```

Of the snippet, the `@requires` line parses today, and so would all of the `struct`: its header, the
`hologram` line and the two guards with what they hold. The parse never reaches the `struct`,
though. A `theme` takes its rules on indented lines under its header, so the one-line form is
`E_PARSE` at the first `slot`, and the parse ends there. Even indented, a `slot` rule binds one
token and takes nothing after it, so `scale=2.0` and `glowing=true` are `E_PARSE`. Inside a body
`hologram` is `E_UNKNOWN_KEYWORD`. `E_PARITY_UNSUPPORTED` is raised where a member is bound to a
block only the other edition has ([above](#a-block-only-the-other-edition-has)). The `edition-fallback`
example builds the steps that do exist: a per-edition theme, and the light block under its guards.

### An `@edition` guard holds the escape hatch

A guard is a line of its own: `@edition`, then `java` or `bedrock`, and nothing after it. The lines
it guards are indented one level under it, and it stands in a `struct`, `def` or `site` body, or in
the body of a member there. It shares the sigil with the file directives but is not one. The items
of a file are edition-neutral, so a guard at top level is `E_PARSE`, and so is one in a `theme` body,
where the `NAME_java` and `NAME_bedrock` variants say what differs
([below](#the-build-picks-the-variant-not-the-source)). A guard that names no edition or another
word, takes anything after its edition, holds its lines in braces or has nothing indented under it
is `E_PARSE` too. A guard does not nest: the one around it already picks the edition, so a guard
directly under a guard is `E_PARSE`. Deeper down, a line is judged by the line it stands under, as
it would be outside the guard: a member under a raw member or a Tier 0 part, guarded again or not,
is `E_UNSUPPORTED_NESTING`, and anything under a refused line goes with that line.

A guard opens no scope. Its lines are read as if written where the guard stands, so one under a
`level` takes the level's `y=`, and a raw member or a Tier 0 part gets every check under a guard
that it would get outside one. What the guard adds is the edition whose build keeps the line.

What it may hold is the escape-hatch layer: `raw_block` and `raw_fill` ([Materials and Themes
§7.4](/spec/materials-themes/#74-raw-blocks)), and the Tier 0 parts of [Redstone
§14.1](/spec/redstone/#tier-0-parts): `repeater`, `comparator`, `redstone_torch` and
`redstone_dust`. A line that writes an `nbt={...}` compound belongs to that layer too, and stands
under a guard ([Entities §8.1](/spec/entities/#81-two-tiers)), though no keyword reads `nbt=` yet.
Any other member, and a `logic` or `assert` line, is `E_SEMANTIC_IN_EDITION_GUARD`
([Lint](/spec/lint/)), once at the outermost such line, taking
anything indented under it along. So is a `logic` or `assert` line under a guarded raw member or
Tier 0 part, which outside a guard would still reach redstone synthesis. No build and no other
check reads a refused line or what goes with it, so it draws that finding alone. A raw member or a
Tier 0 part in a `site` body is `E_MISPLACED_MEMBER`, with a guard around it or not, so a guard
there holds nothing that builds.

A pinned build keeps the lines under its own edition's guards and drops the other edition's.
`cairn compile --edition java` builds the snippet's `minecraft:light` and never builds
`light_block_15`. Lowering that pins no edition keeps neither: `cairn lower` without `--edition`
prints the edition-neutral pivot, so it leaves every guarded line out and a note says how many. It
still reports what each edition's branch finds on those lines, and exits 1 on an error there.

Two lines under guards for different editions never build together, so they may share an `id=`.
A guarded line and an unguarded one, or two under guards for one edition, are both built by that
edition and are `E_DUPLICATE_ID`.

A `cairn check` that pins no edition checks both branches. Each edition's resolution and lowering
keep that edition's guards, and a finding whose span, or one of whose notes' spans, is on a line
under that edition's guard is that edition's verdict, reported as it is for a line only one
edition's theme variant reads ([Lint §11.3](/spec/lint/#113-error-vs-warning)). Resolution pins no
variant on either branch, so it binds each theme as it would in a source with no guard.
`cairn info`'s portability and buildable-target rows ([§10.5](#105-which-version-is-it-for-has-three-answers))
are per edition already, so each counts its own edition's branch and none of the other's.

### `@edition` pins an edition, not a version

The guard settles which branch is built and nothing else. The id inside it is still checked against
the one `(edition, version)` the compile pinned
([§10.4](#104-fail-loud-and-minimum-version-inference)), so where an edition respells a block
inside its own supported range, `@edition bedrock` names the branch and the file's floor names the
spelling.

Bedrock's light block is that case, and it is why the snippet above carries a floor. 1.21.0 spells
the block `light_block` and carries the level beside it as a `block_light_level` state; 1.21.40
promoted the level into the id, so 1.21.40 and 1.21.60 spell the same block `light_block_0` …
`light_block_15` and have no id named `light_block` at all. The `aliases` row holding all of those
spellings together ([§10.4](#104-fail-loud-and-minimum-version-inference)) answers the diagnostic
and not the source. Its answer is the closed set the pinned target declares: `light` against Bedrock
1.21.60 comes back as all sixteen levels. A set of sixteen is not a spelling. Picking one of them is
the silent substitution that section forbids, so it stays the author's to do. Writing the branch for
one of the two shapes is doing it, and the floor is what says which shape that is. Being scoped, it
is inert on the Java build ([§10.4](#a-floor-may-name-its-edition)), which the other branch serves.

Leaving the floor off is loud rather than wrong: `light_block_15` against Bedrock 1.21.0 is
`E_UNKNOWN_ID`, since the check is per version. The floor adds two things. One is a different
refusal: with it, the same command is `E_VERSION_CAP`, which points at `--target` and the floor
rather than at the block, since a compile checks no id at a target its floors refuse
([§10.4](#the-declared-floor-is-enforced)). The other is a declaration: the version half of what the
branch is for, written where the rest of the file's constraints are, and the half the other headers
can be read against. A file floored at 1.21.40 whose `@intended_targets` names 1.21.0 is
`E_INTENDED_TARGET_CAP` at `cairn check` ([§10.4](#the-hint-is-weighed-against-the-floor)), before
any target is picked; the same file without the floor says nothing until one is. A build that must
serve both spellings is two builds: there is no version conditional to pair with `@edition`.

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

No command applies a structure file to a world yet, so neither rule is enforced and no command
takes `--allow-cross-version`.

Not every build needs to be edition-portable. The compiler's job is to state what breaks
portability.

## 10.8 Compatibility tier of Cairn's own surfaces

The `(edition, version)` axis above covers what Cairn *emits*. The orthogonal axis is what Cairn
promises about its own evolution: `.crn` syntax, the lockfile, the CLI flags, the Rust API. CalVer
has no "major" axis to read those promises off, so they are spelled out in
[Compatibility Tiers](/spec/compatibility/). A `Stable` surface gives one release of `W_DEPRECATED` lead
time, a code reserved until the first deprecation adds it; an `Evolving` surface can change in any
monthly minor; `Internal` makes no promise.
