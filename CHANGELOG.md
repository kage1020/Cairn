# Changelog

## [Unreleased]

### Added

- *(cli)* `cairn check` takes `--target <version>`, and with it reports `E_UNKNOWN_ID`. The command
  took `--edition` and no version, so it pinned no target — and a block id either exists in a
  version or it does not, so with nothing pinned the check was skipped rather than run against a
  version nobody chose:

  ```
  $ cairn check s.crn ; echo "exit=$?"        # slot floor -> @totally_not_a_block
  exit=0
  $ cairn check s.crn --edition java --target 1.21.4 ; echo "exit=$?"
  s.crn:2:17: error[E_UNKNOWN_ID]: `minecraft:totally_not_a_block` is not a block in `java 1.21.4`
  exit=1
  ```

  A CI job gating on `cairn check` therefore went green on a source `cairn compile` refuses, which
  is the shape of gap `E_PARTIAL_BUILD` and the `check`-inside-`compile` call were added to close.
  Here it was intrinsic: the information the check needed was not on the command line. The flag puts
  it there. It requires `--edition` for the reason spec §4.2 forbids `--target` alone — "1.21"
  names different releases on Java and Bedrock — and resolves against the same data table `compile`
  does, `latest` included; a version the edition does not ship is the same refusal `compile` gives,
  printed after the file's own findings so a command-line typo does not bury a syntax error, and
  the file is lowered against the unpinned view first so every finding that needs no id table is
  reported rather than waiting for a second run with the flag spelled right.

  Pinning a version is what turns block-array lowering on, so the rest of that stage's findings —
  `E_UNKNOWN_ABSTRACT_TOKEN`, `E_INCOMPATIBLE_MATERIAL` — come with it rather than being filtered
  back out: a report that ran the pass, saw them, and said nothing would be the same silence the
  flag exists to end. A scope the lowering loses earns the same `E_PARTIAL_BUILD` refusal
  `compile` gives it, for the same reason and in different words: the compile must not certify a
  partial build, and the check certifies nothing but must not pass a source the compile at that
  pin refuses. What does not come with it is anything `compile` writes. No artifact, no lockfile,
  and no `@requires` floor enforcement (`E_VERSION_CAP`): a lockfile certifies a build, and holding
  `--target` to the floors belongs to the command that produces one.

  Neither run-level refusal — the unshipped target, the lost scope — is an element of the
  `--format json` array. Neither is a finding at a span, and giving one a line number would put a
  position on a fact that has none; both are stderr and an exit code, which is the shape `compile`
  gives them too.

  Checking against every version the edition ships would have needed no flag and answered a
  different question: `stone_bricks` is a block on Bedrock 1.21.40 and not on Bedrock 1.21.0, so an
  edition-wide pass accepts it everywhere and tells a 1.21.0 build nothing. That case is the
  interesting half, and it is the one a pin gets right — the refusal names `minecraft:stonebrick`,
  from the pack's alias table.

  The flag is opt-in, so the no-target behaviour is unchanged and no source that passes today starts
  failing. It also makes `DiagnosticData::UnknownId` (`spec/lint.md` §11.2) observable from a CLI
  for the first time: the payload was documented and reachable only by library consumers, because
  `compile` prints text and `check --format json` never produced the code.

- *(core)* The `@cairn` header's value is read as a language version, under two new codes.
  `W_INVALID_CAIRN_VERSION` when the value is not `YYYY.M[.PATCH]`, and `W_FUTURE_CAIRN_VERSION`
  when it names a version later than the compiler reading it. Every one of `@cairn banana`,
  `@cairn 2026.06.1.2`, `@cairn 2026.13` and `@cairn 9999.1` was accepted in silence, because
  nothing in the pipeline had ever looked at the string: it was wrapped verbatim at parse time and
  read once afterwards, by the duplicate-header pass, which only wanted a name.

  The header exists so a later compiler can parse and warn correctly, and provenance that no
  compiler can read cannot do that job. The future-version code is the other half of it, and the
  one thing an older build can usefully say about a file written against a newer language: a
  keyword or argument added after this build is reported as `E_UNKNOWN_KEYWORD` or
  `E_UNKNOWN_ARGUMENT`, and only the header knows those findings may be about the gap rather than
  about the lines they name. A whole new syntactic form is `E_PARSE` instead, and no check pass
  runs then, so the gap goes unsaid in the case it explains best.

  Both are **warnings** rather than errors, which is the opposite call from `@requires`. `spec/lint`
  §11.3 makes a finding an error when leaving it alone yields something other than what the source
  asked for; `@cairn banana` builds byte-for-byte what `@cairn 2026.06` builds. `@requires` is an
  error on the same test because its floor is an input — it sets `cairn info`'s compatible range
  and the bound `cairn compile --target` is held to, so a floor that evaporates accepts a target it
  should not. No source that compiles today stops compiling.

  The accepted shape is a four-digit year, a month `1 … 12`, and an optional patch, plus nothing
  after it — the header value is a whole line, so `@cairn 2026.6 draft` names a version and then
  something else. A leading zero on the month is accepted and does not change it, so `2026.06` —
  what every shipped `.crn` and the spec itself write, calver.org's `YYYY.0M` — and `2026.6` are
  one version. That is stricter than the label `@requires` reads, deliberately: that one orders a
  label out of Mojang's namespace, where a component need only begin with a digit and a
  pre-release tag is a label too, while this one names Cairn's own version, where every component
  is digits, `2026.13` is a month that does not exist and `1.2` is a semver rather than a year.

### Fixed

- *(docs)* `spec/versioning-editions.md` §10.7 illustrated the `@edition` escape hatch with
  `minecraft:light_block["block_light_level"=15]`, an id that exists on Bedrock 1.21.0 and on
  neither of the other two targets the registry pack ships: 1.21.40 promoted the level into the id,
  so 1.21.40 and 1.21.60 spell that block `light_block_0` … `light_block_15` and carry no
  `light_block` at all. The section is the one place the spec names an id to show that Bedrock
  spells things differently, so the wrong spelling there is the spelling a reader carries away —
  and a source copied from it is refused with `E_UNKNOWN_ID` on two of the three targets.

  The branch is now written for the flattened ids, beside the edition-scoped floor that says which
  of the two shapes it is for, and a new subsection says why the floor belongs there: `@edition`
  picks an edition and not a version, while the id inside it is checked against the one
  `(edition, version)` the compile pinned. Where an edition respells a block inside its own
  supported range, the guard alone does not pin a spelling — and the `aliases` row holding those
  spellings together answers the diagnostic rather than the source, since an answer is the closed
  set and never a pick from it.

### Breaking changes

- *(core)* The palette is a **set with a canonical rendering**, not an insertion log. Slot `0` is
  still air; every other entry is now placed by `(id, properties)` — the id, then the state
  properties compared by name — and the voxel grid is renumbered onto the result.

  `spec/compilation.md` §4.1 opens by promising that source may be written "flat and order-free".
  The phase buckets close that across phases and the palette prune closes it for a member whose last
  cell another one took, but neither reaches two members in one phase that share no voxel at all:
  `Palette::intern` appends on first use, first use is a paint, and inside a phase the paints run in
  the order the lines are written. So two windows on opposite walls interned in line order —

  ```
  struct t size=7x5
    walls  mat_slot=wall height=3
    window side=front y=1 offset=1 size=1x1 mat_slot=glass
    window side=back  y=1 offset=1 size=1x1 mat_slot=deck
  ```

  — and swapping those two lines moved `oak_planks` past `glass_pane` in a palette the `.nbt` emits
  verbatim, in the portability rows `cairn info` prints one per entry, and inside the
  `resolved_ir_hash` that covers the whole array. Every voxel was identical and the artifact was
  not, so a build cache keyed on that hash missed on an edit that changed nothing.

  §4.1's grant of last-wins to "local overrides within the same phase" does not cover it, because
  that grant is about *which block wins a cell* and these two members do not share a cell. Nothing
  about the result depended on their order; only the numbering did. The two ways out were to sort,
  or to say the palette order is part of what the source describes and that "order-free" is a claim
  about voxels rather than about bytes. §4.8 now takes the first: the palette is derived from the
  finished grid alone, so any permutation of the source that leaves the grid alone leaves the whole
  `BlockArray` alone — which is the stronger reading §4.1 already reads as.

  The sort consults the resolved state and nothing else — no spans, member ids, phases, or paint
  order — so it is not a claim that two targets agree on the palette, only that one target's palette
  does not depend on how the source was arranged. Property bags are compared by name rather than in
  iteration order, because `IndexMap` compares as a map: two states carrying the same pairs in
  different orders are already equal and fold onto one slot, and reading them in place would have
  put the source-order dependence back in through the tie-break.

  **Breaking**: the palette order of every structure Cairn emits changes, and with it the `.nbt`
  bytes, the order of `cairn info`'s portability rows, `resolved_ir_hash`, and the letters
  `cairn lower`'s ASCII preview draws — its glyphs come off the palette index, so `cottage.crn` now
  reads `#` cobblestone, `a` glass_pane, `b` oak_planks where it read `#` oak_planks,
  `a` cobblestone. `cottage.crn`'s palette went from
  `[air, oak_planks, cobblestone, spruce_stairs…, glass_pane]` to `[air, cobblestone, glass_pane,
  oak_planks, spruce_stairs…]`. Every `*.crn.lock` therefore reports a mismatch against
  a build made with an older compiler and has to be regenerated; no example lockfile is committed to
  this repository, so there was nothing here to refresh. The blocks in the file and where they sit
  are unchanged — this moves numbering, not geometry, and a structure loaded into the game is the
  same structure. `Palette::canonicalize` and `BlockArray::canonicalize_palette` are new and public
  for anyone assembling an array outside the lowering pass; `BlockState::canonical_key` exposes the
  order itself.

- *(formats,core,cli)* A blockstate the registry pack was expected to refuse no longer counts as
  ordinary portability. `cairn info`'s Bedrock fold sent every `translate_states` failure into
  `unsupported` through a wildcard, and the three failures do not mean the same thing.
  `UnmappableBlock` is a portability fact: the edition has the block and this compiler has no
  Bedrock mapping for the states on it, which is what a build would hit as well. The other two are
  not, and their own docs say so — a value outside the Java domain ("the registry pack should
  reject these one layer up") and a key the translator does not read. Both mean a blockstate no
  validated pack can produce reached the translator anyway.

  Counted, such a leak printed as `unsupported: 1`: indistinguishable from a stair whose corner
  shape Bedrock simply has no state for, and pointing the reader at a material to change rather
  than at the pack bug it is. `portability_for_bedrock` now answers
  `Result<PortabilityReport, InvalidPalette>`. The palette is walked to the end, so one run names
  every leak rather than the first, and then the whole report is refused rather than published over
  a palette that should not exist:

  ```
  error: the bedrock palette carries blockstates a registry pack is expected to refuse, so this edition gets no portability figure:
    error: stair `minecraft:oak_stairs` has `facing=up`, which is not a valid Java `facing`. Valid `facing`: east, west, south, north. Fix: correct the source blockstate, or compile with `--edition java`
    note: none of that is the source's to repair — a validated pack cannot produce these, so the leak is the pack's or this compiler's. The figure is withheld rather than counting a validation gap as ordinary portability
  exit=1
  ```

  Each leak keeps the translator's own words, so it reads the same from the report as from the
  build that would refuse the same entry, and the closing note is there because every one of those
  sentences ends on a `Fix:` addressed to the author of a blockstate no source can carry.
  `cairn info` exits 1 with no rows at all, which is the shape it already had for a finding that
  refuses the command before a row is computed; the remaining editions are still walked first, so
  one refusal does not hide another's findings.

  The wildcard also removed the exhaustiveness check. The three variants are matched one by one, so
  a fourth has to be classified at the fold rather than joining whichever bucket a `_` arm pointed
  at.

  Two surfaces move. `portability_for_bedrock` returns a `Result` and `UnsupportedReason` loses
  `StateValueUnexpected` and `StateKeyUnread` — the Rust API, tier Internal. Nothing produces
  those two now, and an enum of reasons an entry is unsupported should not carry two that are not
  reasons an entry is unsupported. Dropping them also takes `"reason": "state_value_unexpected"`
  and `"state_key_unread"` out of `edition_portability[].unsupported_entries`, and that is the
  command output, tier Evolving. Neither tag has ever appeared in a real run, so the wire shape a
  consumer has actually seen is unchanged.

  No `.crn` reaches any of this today, so nothing that reports figures now stops reporting them:
  stair properties are built from `Cardinal` and `StairShape` and are in domain by construction,
  the lexer refuses an authored `@id[k=v]`, and a pack's `PackView::lookup` answers with
  `BlockState::bare`. The shipped corpus is held to that — `example_portability.rs` fails the run
  if any example on either edition ever leaks one.

- *(core,cli)* `@intended_targets` is weighed against the version floors the same file declares.
  The two headers used to be two inert statements, so `@requires version>=1.21` beside
  `@intended_targets ["1.20.4"]` passed `cairn check` at exit 0 — while `cairn compile --target
  1.20.4` refused it with `E_VERSION_CAP`. One of the two declarations decides a build and the
  other decided nothing, and the ignored one was the header that reads like an instruction.

  Each version the header names is now placed in the target edition's `DataVersion` table. One no
  `--target` of the edition builds is `W_INTENDED_TARGET_UNSUPPORTED` — a release the pack ships no
  block data for, or a label the table cannot place at all, which is what the other edition's
  numbering looks like — and is not also weighed against a floor, because "this target does not
  exist here" is what the author acts on. The rest, the versions the edition *can* build, are
  counted among themselves: every one of them below a floor is `E_INTENDED_TARGET_CAP`, since the
  file can then be built for nothing it says it is for, and some of them is
  `W_INTENDED_TARGET_CAP`, because the header is a hint (`spec/syntax.md` §5.3) and the versions
  above the floor still build. The floors are the composite fold, so a `def` in a library can
  refuse the intent of the file that places it, and the finding names the part.

  Each command weighs the header in the tables of the editions it is about — the one `--edition`
  names, the ones `cairn info --editions` lists, or both where the command names none — so a report
  scoped to one edition is not refused by the other's answer. One span carries one cap finding, and
  two editions disagreeing about how far it reaches report the error.
  `W_INTENDED_TARGET_UNSUPPORTED` waits until exactly one edition is in scope.

  `cairn info` grew an `intended targets` row, beside the `buildable targets` it can contradict.
  The report used to leave the header out entirely, so the one declaration that names versions was
  the one the output never mentioned.

  **Breaking**: a source declaring both headers in a way that cannot hold now exits 1 from
  `cairn check`, `cairn info` and `cairn compile` where it exited 0. `VersionAxes` carries a new
  `intended_targets` field, and `cairn info --format json` a matching key, which is additive on the
  wire and a new field for anyone constructing the struct — it is `#[non_exhaustive]`, so the
  addition compiles. `cairn info --format text` prints five rows where it printed four.

- *(core,formats,cli)* `E_UNKNOWN_ID` answers a **rename**, not only a typo. The registry pack grew
  an `aliases` component — groups of the spellings one block has worn — and a refused id is now
  reported with the name the pinned target actually uses:

  ```
  $ cairn compile s.crn --edition bedrock --target 1.21.60   # slot floor -> @light
  s.crn:2:17: error[E_UNKNOWN_ID]: `minecraft:light` is not a block in `bedrock 1.21.60`
    note: `bedrock 1.21.60` spells this block `minecraft:light_block_0`,
          `minecraft:light_block_1`, `minecraft:light_block_2`, `minecraft:light_block_3`
          and 12 more, per the registry pack's alias table
  ```

  The suggestion was a Damerau-Levenshtein search over the target's own block table, capped at three
  edits for a path of seven characters or more. That catches `oak_plank` → `oak_planks` and nothing
  else, and every rename is well past the cap: `oak_sign` → `standing_sign` is seven edits,
  `light` → `light_block_0` is eight. So the message an author was most likely to hit was the one
  that said it had no candidate — honest, and no help at all against a table of a thousand ids.
  `spec/versioning-editions.md` §10.4 asks an error to return "the closed set of candidates valid in
  the target", and a distance search structurally cannot produce that set for two spellings that are
  not textually related.

  The real work was deciding what a row is keyed on. Java ids as the base, with Bedrock as
  overriding diffs, is the obvious first cut and answers only half of it — it says nothing about
  Bedrock 1.21.0 → 1.21.40, which is the same class of problem inside one edition. So a row is keyed
  on nothing: it is a **group of spellings**, and which of them belongs to which
  `(edition, version)` is a question the `blocks` tables already answer per version. A lookup keeps
  the members the pinned target declares, and the same rows therefore answer Java → Bedrock and
  Bedrock 1.21.0 → 1.21.40 alike. What the key cannot express is a spelling both editions declare
  meaning different blocks — Bedrock's `snow` is Java's `snow_block` while Java's `snow` is
  Bedrock's `snow_layer` — and such a pair gets no row rather than a wrong one.

  An answer is the closed set and never a pick from it: `@light` on Bedrock 1.21.60 is reported with
  all sixteen light levels, because choosing one would be the silent substitution §10.4 forbids. The
  note prints the first four and counts the rest; the whole set is in the diagnostic's `data`
  payload, under a new `aliases` key beside `suggestion`. The two stay apart because they are
  different claims about the same span — an alias is the pack stating that two names are one block,
  a suggestion is a guess from a string distance — so a quick-fix may apply the first unasked and
  should not apply the second. `cairn info`'s `edition portability` row reads the same table, and an
  entry the edition has under another name now names it instead of reporting a dead end.

  The typo search is unchanged and still runs wherever the alias table says nothing, which includes
  every pack that ships no `aliases` component: the message such a pack produces today is the one it
  produced before. The built-in packs cover the Java/Bedrock spelling splits an author most often
  walks into and the ids Bedrock's own 1.21.40 flattening wave retired. Breaking, and the first of
  these reaches someone who never touches the API: both built-in packs gained a component, so
  `inputs.registry_pack_hash` moves and a lockfile written by an older build records a different
  pack than the one this build reads. For API consumers, `portability_for_java` /
  `portability_for_bedrock` take the pack's `AliasIndex` as a third argument, and
  `UnsupportedReason::AbsentFromEdition` and `DiagnosticData::UnknownId` each carry one more
  field.

- *(core,cli)* A `def` and a `theme` may declare `requires version>=X` on a line of their own, and
  the minimum version of a composite is the max of its parts. `spec/versioning-editions.md` §10.4
  has always said so; neither spelling parsed, so the sentence described a feature that existed at
  no layer. A `def` is a template instantiated by `place`, so a floor on the `def` is a floor on
  every site that uses it — which is how a library of templates carries its own requirements
  instead of every consumer restating them. Module-level `@requires` cannot say that: it applies to
  the whole file, not to the template.

  The expression is the one `@requires` takes, edition scope and all, read by the same
  `parse_requirement` and taken verbatim to end of line — so `24w14a` and `1.21.4-rc1` parse here as
  they do there. `declared_version_floors` now walks the composite: the module's headers plus every
  part the build instantiates, which is a `def` a `place use=` names and a `theme` a scope binds.
  `cairn info`'s `registry compatibility` row runs the same fold, and `E_VERSION_CAP` names the part
  that imposed the floor rather than only the number, because a target refused by a floor written
  inside a template is not actionable as a bare version.

  A `struct` and a `site` take no such line: each *is* the build rather than a part of one, so a
  floor written inside one says what `@requires` already says. Nor does a member's own indented
  child, whose repair is a dedent. Neither refusal fires on the word alone — `requires` stays an
  ordinary keyword wherever it declares nothing.

  **Breaking**: `ast::Item`'s `Def` and `Theme` variants each carry a new `requires` field, and
  `resolve::VersionFloor` a new `origin`, so an external pattern naming every field of one of them
  no longer compiles; adding `..` is the migration. Those two variants are now
  `#[non_exhaustive]` themselves, so the next field costs no second break — the attribute on the
  enum only ever covered adding a variant. The Rust API is Internal tier (compatibility C.2), so no
  deprecation window is owed; the entry is here because C.3 requires one regardless of tier. The
  serialized shape is unchanged: `requires` is skipped when empty and `VersionFloor` is not
  `Serialize`, so `cairn parse`'s AST JSON and `cairn info`'s wire shape are byte-identical for any
  source that does not use the new line.

  One `.crn` source that parsed before does not: `requires` alone on a line in a `def` body was a
  member with no arguments and is now `E_PARSE`, since a body that reads floors owns the failure of
  a floor that states nothing. It was `E_UNKNOWN_KEYWORD` from `cairn check` either way; what
  changes is that `cairn parse` now refuses it too.

- *(core)* `TokenKind::Int` carries its `lexeme` only; the `value: i64` field is gone. The lexer
  cannot tell an integer literal from a truth-table row's bit pattern — both are digits, and only
  the grammar around them says which — so it no longer parses either into a number. Whoever needs
  a value parses it into the type that position actually takes: `i64` in `parse_value`, `u32` for
  a `within` bound. This is what lifts the ceiling that refused a truth table of twenty inputs
  whose row read `11111111111111111111`, with a message about integer range for a table holding
  no integers.

  **Breaking**: `TokenKind` is re-exported from the crate root, and `#[non_exhaustive]` sits on the
  enum rather than on the variant, so an external `TokenKind::Int { value, .. }` pattern no longer
  compiles. The Rust API is Internal tier (compatibility C.2), so no deprecation window is owed —
  the entry is here because C.3 requires one regardless of tier.

  A digit run past `i64` in value position is still `E_PARSE` with the same `IntContext`, lexeme,
  overflow kind and position; it is raised a layer later. Outside value position it now gets the
  message for the position it is in — a `within` bound says "invalid `within` bound", a truth row
  says the pattern holds only `0` and `1`, and `struct 99999999999999999999` says an identifier was
  expected — instead of one message about integer range for all of them.

- *(core)* A `connect` row anchors its ports to the masonry the placement actually painted, so the
  strip and the opening it runs to are decided by one answer instead of two. `walkway` used to
  rebuild its own wall column from the `def`'s top-level `walls` members and their `height=`, and
  the two readings disagreed in both directions: over `walls` whose `mat_slot=` did not resolve the
  openings pass deferred the cut and the strip was laid anyway — a path to a window that was never
  cut, and to a doorway that was never carved, because the door branch consulted no column at all
  — while `walls` declared inside a `level` were painted, cut into, and then refused as an anchor,
  which told an author to move a window that was already in masonry.

  **Breaking**: a source whose walls paint nothing loses walkways it used to get, alongside the
  `W_DEFERRED_MEMBER` the member already earned; one whose walls sit in a `level` gains walkways it
  used to be refused. `port_world_position` is also no longer exported from `cairn_lang_core`: it
  can only be asked correctly with the column the body was lowered against, which nothing outside
  the pass holds, and exporting it is what invited the second derivation.

  The column now travels beside the placement, from the phase that painted it to the pass that
  reads it. Nothing derives it twice, so the rule has nowhere left to drift: a level-scoped wall
  and an unresolvable material change the port's verdict because they changed the wall, not because
  a second copy of the rule was taught about them. The row's `W_DEFERRED_MEMBER` gains a fourth
  note for the masonry contract, and it is the only one of the four whose repair is on another
  line — so it says that the member that could not be built says so on its own line.

- *(core, cli)* `@requires` floors are ordered by `DataVersion` in the target edition's version
  table, the key `spec/versioning-editions.md` §10.1 makes canonical, instead of by comparing
  version labels component-wise as dotted decimals. That comparison had no way to tell two
  editions' numbering apart: Java ships `1.20.4 / 1.21 / 1.21.4` and Bedrock
  `1.21.0 / 1.21.40 / 1.21.60`, so `@requires version>=1.21.4` read as satisfied by Bedrock
  `1.21.40` on `40 > 4` and wrote a lock saying `verified: true` for a target the source rules out
  — the same defect enforcing the floor exists to remove, one edition to the left.

  A floor may now name the edition it is written in — `@requires java version>=1.21.4` — which
  constrains that edition's build and is inert in the other's. An unscoped floor is a floor on
  whichever edition is built and is resolved in that edition's table, so `@requires version>=1.21`
  still means Java's `1.21` and Bedrock's `1.21.0`.

  A floor the table cannot place — inside its span, naming no row, which is what a floor in the
  other edition's numbering is — is the new `E_REQUIRES_UNORDERABLE` rather than a guess. A floor
  below every row is met by every target and one above every row by none, since those two answers
  hold whatever key each row carries.

  **Breaking**: `resolve::declared_version_floor` is replaced by `declared_version_floors(module,
  edition)`, which returns every floor that edition's build is held to instead of the strictest of
  them — picking the strictest needs an ordering, and the ordering is now per edition.
  `parse_requirement` / `parse_min_version` return a `Requirement` carrying the edition scope
  rather than a bare `&str`. A Bedrock build of a source declaring a Java-shaped floor now fails
  where it used to succeed, which is the point.

- *(core)* `@requires` accepts every version-label shape `spec/versioning-editions.md` §10.1 says
  will exist: the pre-release `1.21.4-rc1`, a snapshot `24w14a`, and whatever a date-based scheme
  spells. A `-` is now a token rather than a lexer error, so the suffix reaches the directive at
  all — a header's value is the raw source between its tokens, so a character the lexer refuses
  never reaches the pass that would accept it. A pre-release sorts below the release it names.
  Accepting a label is not a claim that it can be ordered: that is the target edition's table's
  answer, given at `cairn compile --target`. `RequirementError` gains `UnknownEditionScope` and
  `PreRelease`, and `E_INVALID_REQUIRES`'s `reason` payload gains `unknown_edition_scope` and
  `prerelease_not_a_tag`.

- *(formats)!* The registry pack's version table names every **release** of its edition, not only
  the versions the pack can build for. A row says which it is with a new `targetable` column, and
  `data_versions.schema_version` moves to `2` for it — a `schema_version: 1` table still loads and
  still means what it meant, since every row of one was a buildable target and the column defaults
  to `true`.

  Ordering an `@requires` floor and building for a version are different questions, and answering
  the first with the three-row sample the second needs is what made a floor naming a real release
  the pack does not ship — `version>=1.21.1` on Java — indistinguishable from one naming another
  edition's release. It refused a build that used to work, with a message that said `1.21.1` is not
  a Java release. It is; the pack simply had no `DataVersion` for it. Both are fixed: the floor is
  ordered, and the same shape of label that genuinely names no release of the edition being built
  is still `E_REQUIRES_UNORDERABLE`.

  The Java rows are the game's own version metadata (releases only, 1.14 onward, via
  `misode/mcmeta`'s machine-generated summary); the Bedrock rows are Mojang's own
  `bedrock-samples` release list, with the palette integer computed as
  `(major << 24) | (minor << 16) | (patch << 8) | revision` of that build — a formula the three
  rows already shipped confirm exactly. `data_versions.json` records where its rows came from in a
  new `source` field. The loader now also refuses a table whose keys are not unique and ascending,
  or whose labels do not sort the same way by text as by key — the property placing a floor
  *outside* the table's span actually rests on, which was previously stated in a doc comment as
  "whatever key each row carries" and is not that.

  `--target`, `cairn info`'s `buildable targets`, and the supported-target lists are unchanged:
  they read the targetable rows.

## 2026.9.0 — 2026-09-01

### Added
- *(core,cli,lsp)* give a file that does not parse a diagnostic like every other ([#275](https://github.com/kage1020/Cairn/pull/275))

### Fixed
- *(core)* report a def member's slot finding once per theme, not once per placement ([#277](https://github.com/kage1020/Cairn/pull/277))
- *(core)* [**breaking**] refuse a `key=` no member of that role reads ([#262](https://github.com/kage1020/Cairn/pull/262))
- *(formats,core,cli)* [**breaking**] name the palette entries the unsupported figure counts ([#254](https://github.com/kage1020/Cairn/pull/254))
- *(cli)* [**breaking**] say which supported targets can build the source ([#252](https://github.com/kage1020/Cairn/pull/252))
- *(core)* [**breaking**] report a truth table that verifies nothing ([#249](https://github.com/kage1020/Cairn/pull/249))
- *(redstone)* [**breaking**] refuse a binding that names no signal ([#248](https://github.com/kage1020/Cairn/pull/248))
- *(core)* size the volume for the members that will paint ([#242](https://github.com/kage1020/Cairn/pull/242))
- *(core)* compare a value by what it says, not by where it was written ([#236](https://github.com/kage1020/Cairn/pull/236))
- *(core,cli,formats,nbt)* [**breaking**] read the lockfile back, and make the rest of the reproducibility story true ([#234](https://github.com/kage1020/Cairn/pull/234))
- *(core,tree-sitter,ci)* one version number for every manifest, and a gate on the file editors resolve `.crn` through ([#231](https://github.com/kage1020/Cairn/pull/231))
- *(core)* cut a window into the wall it names, and seat a one-layer roof on the wall ([#227](https://github.com/kage1020/Cairn/pull/227))
- *(redstone)* give every signal one driver, and every binding a component to sit on ([#224](https://github.com/kage1020/Cairn/pull/224))
- *(core,redstone)* evaluate a member in the phase the spec puts it in, and collect in source order ([#220](https://github.com/kage1020/Cairn/pull/220))
- *(core)* size the volume from the members the pass will actually paint ([#216](https://github.com/kage1020/Cairn/pull/216))
- *(core)* stop stapling stair states onto a block that has none ([#214](https://github.com/kage1020/Cairn/pull/214))
- *(core)* make a --edition pin mean the same thing wherever a theme is picked ([#212](https://github.com/kage1020/Cairn/pull/212))
- *(formats)* count a block the edition does not have as unsupported ([#207](https://github.com/kage1020/Cairn/pull/207))
- *(formats)* check every block id against the target that will load it ([#206](https://github.com/kage1020/Cairn/pull/206))
- *(core)* enforce the version floor the source declares ([#200](https://github.com/kage1020/Cairn/pull/200))
- *(core)* report the line the error is on, whichever way the file ends its lines ([#197](https://github.com/kage1020/Cairn/pull/197))
- *(core)* refuse the text the lexer cannot represent ([#193](https://github.com/kage1020/Cairn/pull/193))
- *(core)* report two theme selector rows that select the same members ([#183](https://github.com/kage1020/Cairn/pull/183))
- *(core)* report a `place` row that cannot become a placement ([#182](https://github.com/kage1020/Cairn/pull/182))
- *(core)* [**breaking**] report members and values the enclosing body never reads ([#181](https://github.com/kage1020/Cairn/pull/181))
- *(core)* report an indented body that nothing reads ([#179](https://github.com/kage1020/Cairn/pull/179))
- *(core)* report duplicate top-level names and header directives ([#177](https://github.com/kage1020/Cairn/pull/177))
- *(core)* reject connect endpoints that are not `<place>.<port>` ([#175](https://github.com/kage1020/Cairn/pull/175))
- *(cli)* refuse a --lock that lands on an artifact, and name what blocked
- *(cli)* commit a build as one set instead of deleting on failure
- *(core)* bound the walkway by area, which is what gets allocated
- *(core)* bound derived extents and stop trusting source text as an id
- *(core)* guard block nesting and bound the expression tree
- *(core)* bound value and expression nesting in the parser

[Read More](https://github.com/kage1020/Cairn/releases#release-v2026.9.0)

## 2026.8.2 — 2026-08-01

### Fixed
- *(tree-sitter)* regenerate the parser when the version moves

## 2026.8.0 — 2026-08-01

### Added
- *(redstone,cli,core)* place edition-tagged cells inside circuit region reservations (M6-PR4) ([#93](https://github.com/kage1020/Cairn/pull/93))
- *(core,formats,cli)* populate parity table and per-edition theme fallback (M4-PR3) ([#86](https://github.com/kage1020/Cairn/pull/86))
- *(core)* route walkways around structures so village compiles clean ([#83](https://github.com/kage1020/Cairn/pull/83))
- *(core)* recognize door actuator patches so redstone-door compiles clean ([#82](https://github.com/kage1020/Cairn/pull/82))
- *(core)* recognize circuit region markers so redstone-door drops the circuit deferred ([#81](https://github.com/kage1020/Cairn/pull/81))
- *(core)* lower pressure_plate fixtures so redstone-door drops the plate deferreds ([#80](https://github.com/kage1020/Cairn/pull/80))
- *(core)* lower level blocks and eave stairs so themed-tower compiles clean ([#77](https://github.com/kage1020/Cairn/pull/77))

[Read More](https://github.com/kage1020/Cairn/releases#release-v2026.8.0)

## 2026.7.0 — 2026-07-01

### Added
- *(core,examples,docs)* extend door `at=` to accept `left|right` for site walkways ([#51](https://github.com/kage1020/Cairn/pull/51))
- *(core,examples,docs)* expose walkway ports on window members ([#50](https://github.com/kage1020/Cairn/pull/50))
- *(core,docs)* add E_CONNECT_ARITY check pass for connect row arity ([#49](https://github.com/kage1020/Cairn/pull/49))
- *(core,formats,cli,docs)* lock walkway lowering follow-ups (M3-PR5) ([#37](https://github.com/kage1020/Cairn/pull/37))
- *(core,cli,formats,docs)* land port model and walkway voxelisation (M3-PR4) ([#32](https://github.com/kage1020/Cairn/pull/32))
- *(core,cli,formats,docs)* lower site placements end-to-end (M3-PR3) ([#31](https://github.com/kage1020/Cairn/pull/31))
- *(core,cli,formats)* lift abstract material tokens via registry pack (M3-PR2) ([#30](https://github.com/kage1020/Cairn/pull/30))
- *(core,docs)* add shed/hip/flat roof voxelisers (M3-PR1) ([#28](https://github.com/kage1020/Cairn/pull/28))
- *(core,formats)* add fail-loud nearest-valid suggestions (2026.12-PR2) ([#27](https://github.com/kage1020/Cairn/pull/27))
- *(core,cli,formats)* voxelize cottage.crn end-to-end (M2-PR6) ([#25](https://github.com/kage1020/Cairn/pull/25))
- *(core,cli,formats,nbt)* add java backend + lockfile + cairn compile (M2-PR5) ([#24](https://github.com/kage1020/Cairn/pull/24))
- *(core,cli)* add block-array IR + cairn lower (M2-PR4) ([#23](https://github.com/kage1020/Cairn/pull/23))
- *(core,cli)* add semantic resolver + cairn info (M2-PR3) ([#22](https://github.com/kage1020/Cairn/pull/22))
- *(core,cli)* add cairn check + span-bearing AST/IR (M2-PR2) ([#21](https://github.com/kage1020/Cairn/pull/21))
- *(core)* introduce Intent IR and AST->IR lowering ([#20](https://github.com/kage1020/Cairn/pull/20))
- *(core)* [**breaking**] structured ParseError::InvalidInt with IntContext ([#19](https://github.com/kage1020/Cairn/pull/19))
- *(core)* land M1 lexer, parser, and `cairn parse` on canary ([#12](https://github.com/kage1020/Cairn/pull/12))

### Changed
- *(core,cli,formats)* [**breaking**] replace site/walkway String primitives with newtypes (closes #34) ([#48](https://github.com/kage1020/Cairn/pull/48))
- *(core,cli,docs)* expose structured Diagnostic data payload ([#47](https://github.com/kage1020/Cairn/pull/47))
- *(core)* mark resolver silent arms as INVARIANT and add W_DEFERRED_CONNECT cascade ([#46](https://github.com/kage1020/Cairn/pull/46))
- *(core)* [**breaking**] lift 1-based / non-zero / boolean invariants into the AST types ([#17](https://github.com/kage1020/Cairn/pull/17))
- *(core)* [**breaking**] introduce DottedRef and Statement; remove Command/Extra ([#16](https://github.com/kage1020/Cairn/pull/16))
- *(core)* make indent-stack invariant explicit + surface ParseIntError kind ([#15](https://github.com/kage1020/Cairn/pull/15))

### Fixed
- *(ci,docs)* switch CalVer from YYYY.0M to YYYY.M so cargo accepts the version ([#52](https://github.com/kage1020/Cairn/pull/52))
- rename crates to cairn-lang-* and gate publish until first release ([#4](https://github.com/kage1020/Cairn/pull/4))

[Read More](https://github.com/kage1020/Cairn/releases#release-v2026.7.1)
