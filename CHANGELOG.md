# Changelog

## [Unreleased]

### Added

- *(core,tree-sitter)* A truth table that is partial on purpose had no way to say so. A table may
  leave combinations to the circuit deliberately, and `W_TRUTH_TABLE_PARTIAL` had no sentence the
  author could write to mean it:

  ```
  assert truth(sig.a, sig.b, sig.enable -> sig.out) { 001->0; 011->1; 101->1; 111->1 }
  ```

  ```
  s.crn:2:3: warning[W_TRUTH_TABLE_PARTIAL]: this `assert truth` assigns 4 of the 8 combinations its 3 inputs can take
    note: Fix: add a row for `000`, `010`, `100`, and `110`
  ```

  Every combination the warning names has `enable = 0` and is unconstrained on purpose. The only
  ways out were to write those four rows — asserting what the author does not mean — or to leave
  the warning standing, which spends the signal the coverage finding exists to carry.

  `-` is now read in both halves of a row, meaning a different thing in each. In the **input**
  pattern it is a don't-care: the row stands for every value of that input, so `-11->1` says what
  `011->1` and `111->1` say together. In the **output** it declines to constrain: `--0 -> -` says
  the table has nothing to say about any combination whose third input is low. Together they
  answer the table above, and it lints clean:

  ```
  assert truth(sig.a, sig.b, sig.enable -> sig.out) { 001->0; -11->1; 101->1; --0 -> - }
  ```

  An input `-` is shorthand for the rows it stands for rather than a construct of its own, so the
  rules around it are the table's existing rules read through the don't-cares. Coverage counts
  combinations rather than rows: `0--` assigns four of a three-input table's eight, and a `-`
  output counts the same as any other, which is what lets one row answer four. Two rows may not
  both assign one combination, which is what `E_TRUTH_TABLE_CONFLICT` already said of two rows
  sharing a pattern and now says of `0-` and `-1`, which share `01`:

  ```
  s.crn:2:52: error[E_TRUTH_TABLE_CONFLICT]: this row assigns `01` the output `1`, and an earlier row assigns it `0`
  s.crn:2:43:   note: first row assigning `01` here
  ```

  When two rows agree it stays `W_TRUTH_TABLE_DUPLICATE_ROW`, and the repair depends on the shape.
  A row inside an earlier one — `01` under `0-` — is deleted. Two that merely cross are narrowed,
  because deleting either would lose the combinations only it assigns. That case also withholds the
  coverage finding rather than answering it with a count taken without the later row, which would
  name a combination the table does assign.

  A `-` output beside a concrete one is neither: there is nothing for the concrete output to
  contradict, and nothing for it to agree with, so the rows overlap without the usual repair being
  available — whichever is deleted, the table reads differently afterwards:

  ```
  s.crn:2:52: warning[W_TRUTH_TABLE_DUPLICATE_ROW]: this row assigns `01` the output `1`, and the earlier row `0-` leaves it unconstrained
  s.crn:2:43:   note: first row assigning `01` here
    note: Fix: delete whichever of the two you did not mean — a `-` output says the combination is deliberately unconstrained, and the other row constrains it, so the table reads differently depending on which one stays
  ```

  A table whose rows all decline is the empty table written at length, so it earns the empty
  table's error rather than a coverage warning it would pass:

  ```
  s.crn:2:3: error[E_TRUTH_TABLE_EMPTY]: every row of this `assert truth` has a `-` output, so it verifies nothing
    note: Fix: give at least one row a `0` or `1` output, or delete the assertion — a `-` output says a combination is deliberately unconstrained, which is only worth writing beside combinations that are constrained (this table has 4 to choose from)
  ```

  `-` and `->` share a character and the lexer takes the arrow whenever it can, so a row whose last
  input is a don't-care is written `11--> 0` or `11- -> 0`; both are the same row. One character
  short of the input list is the only width a swallowed `-` can produce, so that refusal says so:

  ```
  s.crn:2:50: error[E_PARSE]: truth-table input pattern `11` is 2 bits wide, but the table has 3 inputs. A `-` written immediately before `->` is read as part of the arrow, so a row whose last input is a don't-care is written `11--> 0` or `11- -> 0`
  ```

  The output side has no such wrinkle: the arrow is already read by then, so `-> -` and `->-` are
  the same row.

  `bit_pattern` becomes an external token in the tree-sitter grammar for the same reason. A token
  regex takes the longest run of `[01-]` it can, which reads `11--` out of `11--> 0` and leaves a
  bare `>` the row's arrow cannot use — a grammar that accepts the spaced spelling, refuses the
  run-together one, and builds a different row from the source both parsers accept.

- *(core)* An argument in a keyword's vocabulary could still be read by nothing, depending on how
  a sibling argument on the same line was written. `E_UNKNOWN_ARGUMENT` closed the case where a
  `key=` is outside the vocabulary; one level down, a key that *is* in it routed past its reader in
  silence:

  ```
  struct s size=9x9
    roof kind=gable mat_slot=roof slope_to=front
  ```

  `slope_to=` is a `roof` argument and lints clean. `fill_roof` dispatches on `kind=` and only the
  `shed` arm consults the direction, so on a gable the value was carried into the IR and dropped —
  exactly the way a misspelled key used to be. The author got a roof that ignored the way they
  pointed it. `place gap=` was the same defect on the other side of the language: an `at=origin`
  row is anchored absolutely and `resolve_place_origin` returns before it reads a distance, so
  `at=origin gap=5` built and said nothing.

  The vocabulary now has a second axis, closed per keyword *and* per the way the argument that
  selects the lowering rule is written (`MemberRole::conditional_arguments`), and the `arguments`
  check pass reports a key the selected rule does not read as `W_IGNORED_ARGUMENT`:

  ```
  s.crn:2:42: warning[W_IGNORED_ARGUMENT]: `slope_to=` is an argument `roof` reads only with `kind=shed`, and this one is `kind=gable`; the value was ignored
    note: either argument may be the repair — write `kind=shed` to have the `slope_to=` read, or drop `slope_to=` and keep the `gable` roof the line already asks for
  ```

  An arm of that axis can be the selector's *absence*, where the absence is itself a rule: a
  `place` with no `at=` is the relative placement that reads `gap=`, so the finding on
  `at=origin gap=5` offers dropping the `at=` as one of its two repairs.

  A warning rather than the refusal `E_UNKNOWN_ARGUMENT` is, because a key outside the vocabulary
  has one repair site and this has two: the author meant the rule that reads the key, or meant this
  rule and the key is left over. The message names both and picks neither — `spec/lint.md` §11.3
  records the same reasoning.

  Where the *selector* names no rule — a `kind=` the dispatch does not know, or, on an axis with no
  absent arm, none at all — nothing is reported here: the member does not lower, its
  `W_DEFERRED_MEMBER` carries the whole repair, and a second finding would bill one repair twice.
  That deferral comes from block-array lowering, so a `cairn check` with no `--edition` /
  `--target` reports neither; the case that is always reported is the one where the member builds.
  The `stair` row of the table is the other half of the shape and reports nothing today:
  `kind=stairs` is the only kind `fill_stair` accepts, so every key it reads is read under that
  one value.

  "Nothing reads it" is not a fact a table can check about itself, so the table is held to the
  dispatch from both sides. Each pair is built twice, at two values a reader would tell apart: an
  arm listed as reading the key has to build different blocks, an arm listed as not reading it the
  same ones, and each build has to paint more than the same body without the member — two empty
  builds compare equal. From the other side, `RoofKind::ALL` and the table's arms are required to
  name the same kinds, so a kind added to the dispatch alone fails rather than going quiet. A key
  made inert by a *count* rather than by a rule — `window step=` at `repeat=1`, which the stamp
  loop consults only from the second instance on — is a different shape and is still unreported.

  `roof kind=` written as something other than an identifier now names the shape it got rather than
  repeating the closed set: `kind="shed"` spells a kind that is in the set, and being told the set
  again answers a question the author did not ask. That message is the whole repair for such a
  line, since the check pass defers to it.

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

- *(tree-sitter)* A size separator with a space after it was accepted by `cairn-lang-core` and
  refused by this grammar ([#270](https://github.com/kage1020/Cairn/issues/270)):

  | source | core | grammar |
  | --- | --- | --- |
  | `"struct s size=3x3\n  floor size=2x 2\n"` | Accept | **Reject** |

  The reference lexer reads `2x 2` as three values — `2`, `x`, `2`, the reading `size=2 x 2`
  gets — because `scan_number` only takes an `x` as a separator when a digit follows it. The
  grammar's separator is an external token, and its scanner branch took the `x` on sight, so the
  row entered `size_literal` and then failed it for want of a height. The branch now looks at the
  character behind the `x` and declines where it is not a digit, so tree-sitter's own lexer reads
  the `x` as an identifier and the row parses the way the reference parser parses it. The source
  is still refused, one layer later and the same way on both sides: `cairn check` reports
  `E_TYPE_MISMATCH_SIZE` on the bare `2` handed to `size=` and `E_UNEXPECTED_POSITIONAL` on the
  two values behind it. `9 x 7` and `9 x7` stay refused, and `2x2y` and `2x2x9` stay listed as
  divergences in the direction they were. The entry leaves `KNOWN_DIVERGENCES` for the fixture
  table.

- *(tree-sitter)* An over-indented line took the line in front of it down with it in an editor's
  error recovery ([#272](https://github.com/kage1020/Cairn/issues/272)):

  ```
  "theme t:\n    walls b=2\n"

  before: (source_file (ERROR (identifier) (ERROR) (identifier) (identifier) (integer) (ERROR)))
  after:  (source_file (theme_decl name: (identifier)) (ERROR (identifier) (identifier) (integer) (ERROR)))
  ```

  The scanner read one line ahead of every line break and withheld the break when the line behind
  it had an odd indent or jumped more than one level. `_line_start` already refuses both where
  they stand, so the lookahead decided no verdict — across 4000 random layouts none moved — only
  where the error went, and on the break it swallowed the header meant to be protected. It is
  removed, and the break branch consumes the break and nothing else. On the same 4000 layouts
  219 recovery trees change: 24 keep more declarations, directives and rows outside an `ERROR`,
  2 keep fewer — both runs of illegal lines where what was kept was a header the file had
  indented under another — and the rest keep the same amount in a different shape. Which files
  are refused does not change.

- *(redstone)* A `circuit region=` inside a very wide struct was routed before anything measured
  it. The reservation takes the floor's extent, so `region=` is as wide as `size=` and the actuator
  pad lands at `x = width - 1`; stage 2 laid a wire out to it one coord at a time, stage 3 rebuilt
  the same trees to measure them, and stage 3 was what refused the result against the attenuation
  cap. The answer was right and a four-million-wide floor took over a minute of a debug build to
  give:

  ```
  struct big size=4000000x4000000
    ...
    circuit region=floor void=3
  ```

  The cap is on the routed segment, and the straight line between two coords is a floor on every
  route between them — it is the router's own search heuristic, admissible for exactly that reason.
  A sink further from its driver than the cap therefore has no route any stage would accept, and
  the routine all three passes call to lay their nets now says so before laying one. Same code,
  same sink named the same way, 4 ms instead of 72 s:

  ```
  s.crn:15:3: error[E_ATTENUATION_LIMIT]: placed netlist for struct `big` puts output pad #0 3999999 blocks from its driver in a straight line — exceeds the v1 attenuation limit of 256 blocks, and no route between two coords is shorter than the straight line between them
    note: Fix: split the logic across several `circuit` blocks, or reserve a `region=` whose pad column sits within the cap of the cells it serves — a larger reservation cannot help, because the straight line between these two is already over the cap
  ```

  A floor and not the measure, so it refuses strictly less than the delay pass does and replaces
  nothing. A route is as long as the straight line only where nothing stands in the way: a region
  256 wide puts its pad 255 blocks from the driver and routes 257 to get there, which is over the
  cap and not over this, and the delay pass is still what catches it. What moved is the shape whose
  distance alone is already hopeless — those are refused at `--stage route` now rather than at
  `--stage delay`.

  The fix line differs from the delay pass's for the same reason. "Enlarge `region=`" is the repair
  when a route is long because it had to go round something, and room to go straight shortens it.
  Nothing shortens a straight line, and for the shape that raises this most often — a reservation
  as wide as the `size=` it came from — enlarging is the direction that makes it worse.

  **`CircuitRegionReservation::reserved_area` saturates** rather than overflowing. It is
  `width * depth * void` in a `u64`, which holds the product of two `u32`s and not of three:
  `u32::MAX` squared is within a rounding error of `u64::MAX`, `2^33 - 2` short of it, so any
  `void` above 1 can carry it over. The workspace sets no `overflow-checks`, so what that cost
  depended on the build, and the shipped profile had the worse of the two outcomes. A release
  build wrapped silently: `size=4294967295x4294967295 void=3` wrapped to ~1.8e19, which no
  caller distinguishes from a real reservation, and the run exited 0 printing a netlist. Where
  the product is an exact multiple of `2^64` it wrapped to **zero** instead —
  `size=2147483648x2147483648 void=4` is the smallest such shape — and dividing by that zero in
  the congestion test took the process down. One stage before any of the above, and a `cairn
  synth` away; the reason it went unnoticed is that every reservation in the corpus has a width
  whose square fits comfortably under.

  `cli_hostile_dimensions` covers the place-and-route passes now. Its existing rows run `parse`,
  `check`, `lower`, `info` and `compile`, none of which routes, so a reservation as wide as a
  hostile `size=` was measured by nothing; the new row puts the same widths to
  `synth --stage route` under the suite's 30-second deadline, and it was the largest of them
  that found the overflow above.

- *(redstone)* The three place-and-route passes each asked one routine what coord a `NetRef`
  names, and the routing pass answered two shapes differently from the other two. A `NetRef::Cell(j)`
  past the end of the cell list was a `debug_assert!` there and a `panic!` in delay and crossing,
  so a release build rooted that net at the **last** cell's coord instead — and what that costs
  grew when routing began laying a path rather than a length, so a net rooted at the wrong coord
  lays its dust down the wrong corridor, and the occupancy set, the crossing detection and every
  buffer coord inherit it. It then panicked one stage later anyway, which is what the leniency
  bought: a wrong layout and the same crash.

  A scope carrying cells but no `circuit region=` reservation was the same asymmetry one screen
  up. Routing asserted in debug and in release handed the scope on unrouted, with no diagnostic;
  the delay pass met it next and refused it there, naming its own stage. What each pass writes is
  promised by the producer↔variant table on `PlacementPhase` after its own stage, so the pass that
  cannot write it is the pass that says why. All three now do, with the wording they already
  shared:

  ```
  error[E_NO_CIRCUIT_REGION]: placed netlist for struct `s` reached routing carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope
    note: Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error
  ```

  **`NetRef::Input(i)` is now bounded too**, in all three. It never was: `input_pad(i, region)`
  answers for any index, saturating `z` at `depth - 1`, so an index past the netlist's input list
  got a coord that stands in no block the pass emitted and the router routed to it. That
  saturation is deliberate where the index is real — more sensors than rows is a collapsed pad
  row, which every stage refuses as `E_ROUTE_CONGESTION` — and it is the absence of a bound above
  it that let an index the netlist never had borrow the same answer.

  ```
  NetRef::Input(1) out of range (inputs.len()=1) — netlist invariant broken by caller-side hand-built IR
  ```

  Both are asserts rather than diagnostics because no edit to a `.crn` produces them and none
  repairs them: a `cairn` run reaches these passes only through the netlist pass, which builds
  both lists. What reaches them otherwise is a caller assembling the Placement IR itself — an
  in-crate test, or a library consumer calling `compile_routing`, `compile_delay` or
  `compile_crossing`, all of which the crate root re-exports.

- *(redstone)* A reservation too shallow to hold its pad row was refused by the routing pass and
  by neither of the two stages after it. The pad column's `z` saturates at `depth - 1`, so a
  region one row deep lands the actuator pad on a cell body or on another pad. Routing caught
  that in its own occupancy seed; the delay and crossing passes rebuilt the same block list, built
  a router over it and laid nets against it without ever asking.

  A `cairn` run cannot see the asymmetry: routing runs first and elides the scope, so the later
  stages never meet one. What reaches them is a caller that assembles the Placement IR itself —
  an in-crate test, or a library consumer calling `compile_delay` or `compile_crossing`, both of
  which the crate root re-exports. What came out then was not a wrong diagnostic but a plausible
  circuit: two blocks on one voxel is a net whose source and sink are the same coord, so the wire
  length is small, the tick count is under every cap, and the dump reads as a layout that works.

  The refusal now sits beside the stranded-sink refusal it is the sibling of, in the routine all
  three stages already call to lay their nets. Both findings are `E_ROUTE_CONGESTION` and only one
  comes out, so which one is now stated rather than incidental: the pad row first, because the
  stranded sink is a symptom of the same shortage and the shared code leaves an author nothing
  else to tell them apart by. Its fix line also stops disagreeing with the placement pass's, which
  refuses the same shape one stage earlier: `depth >= max(inputs, outputs)`, one row per sensor or
  actuator, not that plus one.

  **Two cells on one coord now panic** where they used to pass all three stages in silence. That
  is the same failure — one voxel, a zero-length net, a plausible dump — but no `size=` repairs
  it, because a cell's coord comes from its topological index rather than from the reservation.
  It is an IR the caller built wrong, so it asserts, as this crate already does for a broken
  topological invariant. Nothing the pipeline produces can reach it; a caller hand-building a
  scope with two cells on one coord gets a panic naming the coord instead of a circuit that
  looks fine.

- *(formats,docs)* `cairn-lang-formats`' README named 13 of the 36 items the crate root
  re-exports. The `## Public API` table is what crates.io shows as the answer to "what can I
  call", and it had been answering with about a third of it — every `registry` type, both
  `load_builtin_*` entry points, `supported_list`, `UnsupportedTarget`, `write_compound_gzip`,
  `OutputExt`, `Compound`, `ParityNote`, `StateTranslation`, `BedrockStateError`,
  `InvalidPalette`, `PortabilityCounts` and `PortabilityReport` were absent. The table now names
  all 36.

  Thirteen rather than the seventeen a name-only count gives, because four of the spans were not
  paths at all. Four rows named a second item with the module dropped
  (`data_version::JavaTarget` / `resolve_java_target`), and one of those four —
  `portability::portability_for_java` / `_for_bedrock` — named it with a suffix that is not a
  symbol. Every span is now a full `module::item` path, which also split the portability pair
  into two rows: they do not return the same type, and one sentence for both had been saying they
  did.

  **What the heading means is now written down.** The crate root's `pub use` re-exports, and
  nothing else. An item that is only `pub` inside a module — `registry::AliasCatalog`, anything
  under `registry::blocks` — is deliberately off the table. Both READMEs say so, and
  `CONTRIBUTING.md` and `CONTRIBUTING.ja.md` say it beside the rule that now counts all four
  crate-README inventories as guarded rather than two.

  A test holds both tables to it in either direction, so a `pub use` added without a row fails
  `cargo test --workspace` and names the item, as `cairn-lang-core`'s module table and
  `cairn-lang-cli`'s subcommand table already did. `cairn-lang-nbt`'s table was exact before this
  change and still is; it is guarded now because staying correct by memory is what the other one
  was also doing until it stopped.

- *(core)* A `def` member's lowering diagnostics were reported once per placement. Lowering
  voxelises a def body once for every `place` that instantiates it, and a finding about the def —
  or about the theme `slot` line the def reads — came back on each walk, byte for byte the same:

  ```
  dup.crn:3:16: error[E_INCOMPATIBLE_MATERIAL]: `gable` roof derives `facing` / `half` / `shape` from the geometry and `minecraft:spruce_planks` is not a stair, so those states have nowhere to go
    note: bind the slot to a `*_stairs` material — the registry pack's `roof.dark_wood`, ... all resolve to one
    note: reached through `mat_slot=roof`, so every member reading that slot has it too
  dup.crn:3:16: error[E_INCOMPATIBLE_MATERIAL]: ...the same three lines again...
  ```

  Three placements made three copies on one line, and the second note is the sentence that makes
  the repetition pointless — the finding is anchored on the theme's slot value precisely because
  every member reading that slot has the same problem for the same reason. `E_UNKNOWN_ID` under a
  pinned `--target` multiplied identically, being about a resolved id and the target it was weighed
  against, neither of which the placement changes.

  A lowering diagnostic's identity is now the diagnostic: code, span, message, notes and data. Two
  findings that agree on all of them are one finding reported twice, and the repeat is an artifact
  of how the pass walks rather than anything the author can act on. The rule is stated once, where
  the pass already decides what it reports, rather than per code — the `(member, slot, theme)`
  triple `resolve::resolver` keys its own ledger on is a claim about one finding, and lowering has
  two codes that would each need their own.

  **This is not only about placements.** `E_INCOMPATIBLE_MATERIAL` is anchored on the theme's slot
  value rather than on the member, so two `struct`s — or two `def`s — reading one bad slot were
  two findings on one line and are now one. That is the anchor being taken at its word: the fix is
  the slot line, and the finding's own note already says every member reading that slot has it too.
  A source with no `place` in it at all can see this change.

  What the rule keeps apart is what says it is the right one. Two themes binding the roof slot to a
  non-stair are two findings, even when they bind the *same* non-stair, because they are two lines
  to edit. A `gable` roof and an eave `stair` reading one slot are two findings on that one line,
  because they carry their states from different places and the messages say so. Two `place` rows
  that lost their origin earn the same warning twice — identical text, two rows to fix — kept apart
  by their spans alone. And one `walls` row overwriting two earlier ones earns two
  `W_PHASE_CONFLICT` warnings that agree on everything except which member each note points at.

  One message had to change for the rule to hold. A `mat_slot=` that resolves to nothing now names
  the theme it was read against:

  ```
  variants.crn:17:3: warning[W_DEFERRED_MEMBER]: `gable` roof's `mat_slot=` did not resolve to a block id under theme `alpha_java`; it falls back to `minecraft:spruce_stairs`
  ```

  Without the theme, two placements under two sibling-variant themes wrote the same sentence on the
  `def`'s shared member line, and repairing one of them changed nothing the author could see. This
  is the arm where the resolver stays deliberately silent — a slot only the `_bedrock` sibling
  declares is softened until a pin picks a variant — so on an unpinned `cairn lower` this pass is
  the only reporter.

  The resolver keeps its ledgers. There the answer is not byte equality: sibling-variant softening
  lets two resolutions of one body judge a slot differently, so a resolution that stayed silent has
  to leave the next one free to speak.

- *(website)* 217 links on the documentation site were 404. Every cross-chapter link in the spec was
  written bare-relative (`[Lint](lint)`, 255 of them), Astro emits an extensionless href verbatim,
  and the site canonicalises to a trailing slash — so a link written in `spec/syntax.md` was served
  from `/spec/syntax/` and resolved to `/spec/syntax/lint`. The 38 that worked were the ones on an
  `index` page, where the file's own directory and the page's URL happen to coincide.

  Every internal link is now root-absolute with a trailing slash (`/spec/lint/`, `/ja/spec/lint/`),
  which is the only form that survives: Astro rewrites a link's href in neither direction, with or
  without an `.md` suffix, so what is written is what ships.

  Two checks now hold it, and neither existed. `starlight-links-validator` runs as a build hook,
  after the pages exist, and resolves every link and every `#anchor` fragment — across pages as well
  as within one — and refuses a relative link outright, because on this site a relative link is a
  broken link. `astro check` type-checks `astro.config.mjs` and everything under `src`: the project
  had no `check` script and no `@astrojs/check`, although the config opens with `// @ts-check` and
  carries a `@type` annotation for the sidebar. On its first run it reported that the annotation
  names a type the installed Starlight does not export (`@astrojs/starlight/schema` has no
  `StarlightUserConfig`; it is `@astrojs/starlight/types`), so the sidebar had been unchecked either
  way. Fixed in the same change.

  CI runs `pnpm check` before `pnpm build`. Neither subsumes the other: the build type-checks
  nothing, and the checker renders no page, so it cannot see a link.

- *(docs)* `spec/versioning-editions` described `cairn info`'s `registry compatibility` row as a
  derivation — "the intersection of `since`/`until` over the used tokens and states" — which
  nothing computes. The row reads back the unscoped floors the file and its instantiated parts
  declare, and reports `latest` as the upper edge; the shipped packs carry no `since` / `until` for
  an intersection to be taken over.

  The gap is visible in one run. This source declares a floor every supported version clears, and
  uses a block Java gained in 1.21.4:

  ```text
  $ cairn info hut.crn --editions java           # `@requires version>=1.20.4`, `@pale_moss_block`
  registry compatibility:  1.20.4 .. latest
  edition portability:     Java: portable: 2  degraded: 0  unsupported: 0
  buildable targets:       Java: 1.21.4 (1.20.4, 1.21 refuse)
  intended targets:        (none declared)
  semantic-sensitive:      (none)
  ```

  `1.20.4 .. latest` reads as an answer about which versions build, and the row two below disproves
  it — with `1.20.4` itself among the refusals. The declared floor is not wrong; it is answering a
  different question, and the spec said it was answering this one.

  The spec now says what it does. Axis 1's entry in the section on the three answers to "which
  version is it for?" names the declared range, and a new "The `registry compatibility` row"
  section sits beside the two the other rows already have: what feeds `Vmin` (the `@requires`
  headers *and* the `requires` line of every `def` and `theme` the build instantiates), where
  "strictest" is only a comparison of labels, the two causes of `0.0`, why `Vmax` is a literal, the
  run above, and why the declared floor stays a row of its own rather than being folded away — it
  is an *input* the author wrote, bounding what `cairn compile --target` accepts, where `buildable
  targets` is an *output* about the packs that ship. `E_REQUIRES_CONFLICT` stays reserved for the
  day the two can be compared. The ja mirror follows it.

  That per-version answer is also the shape an intersection could not take: it is per edition, and
  it need not be contiguous, so `[Vmin, Vmax]` would claim a gap it cannot see. So the derivation
  the spec promised is already in the report, two rows down.

  No behaviour changes. `info_8b_the_declared_range_is_not_an_answer_about_which_versions_build`
  holds the claim against the shipped packs — including that the row's own lower edge is among the
  versions the build refuses — and the stale description is cleared from the places it survived
  outside the spec: `version_axes.rs`'s module doc, which still called axis 1 the intersection, the
  `cairn info` renderer, `--help`, and the CLI crate's README.

- *(core)* A `-> value` tail on a member that cannot emit a signal was silent through `check` and
  `compile`:

  ```
  struct s size=5x5
    walls height=3 mat_slot=wall -> sig.a
  ```

  The wall was built, the tail went nowhere, and `sig.a` was left emitted by nothing. The rule that
  refuses this lived in `cairn-lang-redstone`, which only
  `cairn synth --experimental-logic-synth` reaches, so the one pass that knew better was the one
  pass an author was least likely to run.

  `SENSOR_HOSTS` moves into `cairn-lang-core` beside the per-role argument tables, and a new
  `binding` check pass reports the tail as `E_MISPLACED_BINDING`:

  ```
  s.crn:2:35: error[E_MISPLACED_BINDING]: `walls` cannot emit a signal; only a sensor carries a `-> sig.<name>` tail
    note: move the tail onto a sensor — `pressure_plate` is the sensor keyword the surface accepts today; `spec/redstone` "Signal binding" also lists `lever`, `button`, `daylight`, and `observer`
  ```

  Only the host is asked here, because only the host can be asked without a Logic IR. Whether the
  tail's value names a signal, and whether that signal is driven twice or by nobody, stay in the
  redstone pipeline — so a tail on a `pressure_plate` passes this pass whatever it names. A keyword
  the role table does not know is still left to `E_UNKNOWN_KEYWORD`, which is why `lever -> sig.a`
  is not told its host is wrong: `lever` is a sensor the specification lists and the surface has
  not reached.

  `synth`'s `diag_misplaced_sensor` is gone rather than kept beside it. `cairn synth` gates on
  `check`, so keeping both would print the same sentence twice on one line; what that pass still
  does with such a tail is take the driver out of the scope, so a `logic` line reading the signal
  is not separately told it is undefined. `cairn synth`'s emitted code for a misplaced tail
  therefore changes from `E_LOGIC_MISPLACED_BINDING` to `E_MISPLACED_BINDING`, which is allowed:
  the redstone pipeline is Internal tier per `spec/compatibility`. A misplaced *actuator key*
  remains `E_LOGIC_MISPLACED_BINDING`.

- *(core)* A typo in a member's own `[key=value]` was silent through `check` and `compile`, and the
  value was lost:

  ```
  struct s size=9x7
    walls id=shell mat_slot=wall height=5
    window[clas=outer] side=front offset=2 y=2 size=2x2 mat_slot=glass
  ```

  Both commands exited 0 with nothing to say, and the `class` never reached the member. Selector
  attribute keys are read in exactly two places — the `door` actuator-patch recogniser in
  `block_array::lower` and redstone's binding-key walk in `synth` — and on any other role nothing
  consumed them and nothing judged them.

  The `arguments` pass now judges a member's own selector against the same vocabulary its arguments
  answer to: the role's keys, the universal ones, and whatever the module's `theme` selectors coin
  for that keyword. `window[clas=outer]` is `E_UNKNOWN_ARGUMENT` with ``did you mean `class`?``,
  the same finding `window clas=outer` has always earned, because it is the same defect written in
  brackets. `door[id=front]` is unaffected — `id` is a universal key — and so is a word the module
  genuinely coins.

  What a member's selector *means* is a separate question and stays open: it is carried through
  verbatim, and later passes decide whether one binds a fresh id or references an existing member.
  This check does not need that answer. It asks whether the word is one something in this module
  reads, and that has the same answer whichever way the meaning is settled.

  One thing the fix made visible and left alone: a key in a member's own bracket does not make the
  member *carry* the attribute, so a `theme` row selecting on it matches nothing and is
  `E_THEME_SELECTOR_UNMATCHED`. That is the existing rule, now written down in `spec/lint`
  "Diagnostic codes" beside the rest.

  The `-> value` tail, the other half of #263, is not closed here — see that issue for the decision
  it still holds.

- *(tree-sitter)* A declaration with nothing but layout behind it to the end of the file was
  accepted by `cairn-lang-core` and refused by this grammar. It was the last position in a class
  where every other one already parsed:

  | source | core | grammar |
  | --- | --- | --- |
  | `"theme a:\n\n"` | Accept | **Reject** |
  | `"theme a:\n# c\n"` | Accept | **Reject** |
  | `"struct s size=3x3\n  # note\n"` | Accept | **Reject** |

  "Bodyless" was not the whole of it: a header whose body holds nothing but comment lines has no
  row to absorb the layout either.

  Blank and comment lines after a declaration are crossed by the scanner on the way to the
  construct behind them. At the end of a file there is no such construct, nothing asks, and the
  layout was left with no token that could consume it. A declaration whose body holds a row absorbs
  its own through that body's `repeat1($._newline)`, and a directive through its own, which is why
  this was the one position where it survived.

  A new external token, `_file_end`, closes it: `source_file` ends with an optional one, and the
  scanner emits it where it has crossed layout and found the end of the file. A grammar-level
  `repeat($._newline)` cannot do this. At the end of `source_file` it is ambiguous against the one
  at the start — for a file holding nothing but line breaks the two own the same tokens, and
  `tree-sitter generate` refuses it outright. Making the run reachable only after a declaration
  generates, and then moves the failure: `_newline` becomes valid between a declaration's header
  and the body it opens, which is exactly where the scanner has to cross layout rather than
  tokenise it, and five fixtures with a blank or comment line in front of a body start failing. The
  question is one only the scanner can answer — whether what is left is layout all the way to the
  end of the file — so that is where it is answered.

  The three entries leave `KNOWN_DIVERGENCES` and join the fixture table, with eight more shapes
  beside them: blank and comment lines together, a comment line after a body, the lone-`\r` and
  CRLF spellings, a member row and a nested body in front of the trailing layout, a file that ends
  without a final break, and a trailing line of spaces. The committed differential sweep
  (`SWEPT_LAYOUTS` in `parser_parity.rs`) finds no disagreement in the direction it can assert; it
  found 12 before the fix. That direction is the grammar accepting what the reference parser
  refuses, so a regression back to *refusing* trailing layout is held by the fixtures rather than
  by the sweep — which is why they now cover the depths and the line endings rather than one shape
  of each.

  What the fix spends is the comment. The scanner crosses a trailing comment line as whitespace, so
  it reaches no `comment` extra and becomes no node, and `queries/highlights.scm` does not colour
  it. Declining instead would refuse all four shapes the change exists to accept — after a bodyless
  header there is no `_newline` for the file to end on — so the trees are pinned in
  `test/corpus/comments.txt` and the trade is written down at the arm that makes it.

- *(spec)* `spec/lint` "Diagnostic codes" is titled as the catalog a consumer looks a code up in,
  and listed 32 of the 57 codes `DiagnosticCode::as_str` can render. Twenty-five had no row, and
  seven of those appeared on no spec page in either language — the compiler printed them and
  nothing said what they meant.

  A code is public contract: it is the `code` field of the `--format json` payload, it is what a
  consumer branches on instead of matching the prose, and "Error vs warning" sorts codes into error
  and warning by rules that assume the reader can find the code the rule is about. A code with no
  row is a string nothing can read back.

  The section is now exhaustive, in both languages, and says so. Four tables are new — sites and
  placements, connections and walkways, lowering, and the theme / abstract-token rows folded into
  materials and targets — and six codes that had been named only in the prose of a neighbouring
  row or in the payload table further down (`E_UNKNOWN_SLOT_TARGET`,
  `E_THEME_SELECTOR_UNMATCHED`, `W_IGNORED_ARGUMENT`, `W_DEFERRED_MEMBER`, `W_UNUSED_DEF`,
  `W_WALKWAY_BLOCKED`) have rows of their own, since a mention in someone else's paragraph is not
  what a reader with a code in hand finds. `E_PARTIAL_BUILD` is a seventh of the same kind, raised
  outside `DiagnosticCode` by `cairn compile` and by a pinned `cairn check`, and it gets a row
  too. Where a code's rule belongs to another
  chapter the row says what the code means and links there, rather than restating the rule in two
  places.

  `E_THEME_SELECTOR_UNMATCHED` gets a sentence of its own: it is a warning despite the `E_` prefix,
  the prefix is part of a Stable string and stays as written, and severity is read from the
  `severity` field rather than from the first letter.

  What the claim covers is what a stable command prints. The redstone pipeline's `E_LOGIC_*` /
  `W_LOGIC_*` codes are reachable only through `cairn synth --experimental-logic-synth`, whose
  whole surface is Internal tier, so nothing about those strings is promised and a catalog row
  would state a contract that does not exist. The section says that rather than leaving it to be
  discovered.

  A unit test beside `every_code_renders_its_documented_string` now reads the catalog back and
  fails on a code with no row, in either language. It looks for a table row rather than a mention,
  which is the gap the six above were in, and it finds the section by its *title* — carried per
  language, since the title is translated — so renumbering the spec still touches no Rust, which is
  what `CONTRIBUTING.md` promises.

- *(core)* `W_FUTURE_CAIRN_VERSION` could not fire on the file it explains best. It says a source
  declares a language newer than the build reading it, and it is raised by a check pass — so it is
  absent in exactly the case where the version gap is the whole explanation, because parsing
  precedes every check pass and a source that does not parse reaches none of them.

  A later language that adds a keyword or an argument lands inside the shapes this build already
  parses, and the header's warning sits beside the resulting `E_UNKNOWN_KEYWORD` telling the author
  to weigh it differently. A later language that adds a whole *syntactic form* does not: an
  unrecognised `@directive` and an unrecognised top-level item are both `E_PARSE`, and the author
  got

  ```
  bad.crn:3:1: error[E_PARSE]: unknown directive `@materials`
  ```

  with nothing about the header that would explain it.

  `diagnose_parse_failure` now reads the `@cairn` line out of the source text and attaches a note
  when it names a later version:

  ```
  bad.crn:3:1: error[E_PARSE]: unknown directive `@materials`
  bad.crn:1:1:   note: this file declares Cairn `9999.12`, which is newer than this build (`2026.9.2`); the line this error names may be a form a later Cairn adds
  ```

  Out of the text rather than the AST because there is no AST, and rather than the token stream
  because a source that fails to *lex* has no tokens either — `floor a=%` is the shape the note most
  needs to reach. Only the header block is read, the run of lines at the top of the file before the
  first that is neither blank, a comment, nor a directive at column zero, so a `@cairn` written
  anywhere else is not mistaken for one. A note rather than a second finding, because `spec/lint`
  "Error vs warning" makes `E_PARSE` the one finding a build that does not parse reports, and a note
  keeps that true. In `core` rather than in either front end, so the CLI and the language server
  both carry it.

- *(cli)* `cairn parse --format json` and `cairn lower --format json` wrote nothing to stdout when
  the source failed. The reason reached stderr as prose, so a human was told; a consumer reading
  stdout saw an empty stream and had to guess from the exit code:

  ```
  $ cairn parse bad.crn --format json ; echo "exit=$?"
  exit=1
  ```

  `spec/lint` "Machine-readable payload" says every command taking the flag writes exactly one JSON
  document per input. The two fixed here are the two whose product is a dump rather than a report —
  the AST and the block-array IR — and a failure is not either of those with a hole in it. Both now write the document `info` writes where it has no report,
  `{"diagnostics": [ ... ]}`, told apart from the dump by its keys and by the exit code. `lower`
  writes it for a source that does not parse *and* for one that fails a later pass, since the second
  is the other way its stdout came out empty: it refuses to dump an IR built from a source `check`
  rejects, and that refusal now says so on the stream the flag promised.

  Under every other `--format` the findings still read as prose on stderr. `parse` defaults to
  `--format json`, so a bare `cairn parse bad.crn` now reports on stdout as JSON; `--format debug`
  is the prose form.

  Two holes of the same kind are next door and are *not* closed here. `cairn check --format json`
  writes a bare array rather than this document, and its `E_PARTIAL_BUILD` goes to stderr as prose,
  so a pinned run can exit 1 with nothing of error severity in the payload. `cairn info --format
  json` writes nothing at all to stdout on an edition-specific refusal, which "Machine-readable
  payload" has covered since before this change. Both predate it; naming them is what stops this
  entry reading as though every command now holds to the rule.

- *(core)* A `door` asked the wall column *whether* it held any row, where a `window` asked it
  *where*. The two questions differ on one shape, and on that shape the door carved nothing and
  said nothing:

  ```
  struct hut size=5x5
    floor mat_slot=floor
    level id=upper y=6
      walls id=w mat_slot=wall height=4
    door  id=e side=front at=center
  ```

  The walls paint rows 7…10; the door opens at row 1 and painted `AIR` over two rows that were
  already air. The `window` written beside it on the same body was deferred with "the walls occupy
  y=7..=10" quoted back — so the compiler both knew where the masonry was and agreed with a door
  written into a building whose walls start a storey up.

  `carve_door` now asks the column for the course holding the row it opens at, `y_offset + 1`. A
  row inside no course is `W_DEFERRED_MEMBER` naming the row and the rows the walls do occupy, in
  the shape the window's finding already had; a struct with no walls at all keeps its own sentence,
  because "nowhere" and "not here" are different findings. That course is also the cap, replacing
  the check against the struct's highest wall row: a doorway takes two rows where its own course
  has them and one where it does not, so the row above a short course is no longer carved on the
  strength of a taller course elsewhere in the struct. The walkway port asks the same question, as
  `spec/components-editing-sites.md` §9.3.5 requires, so a strip is refused to exactly the doorways
  the openings pass refuses to cut.

  That last part is the one place output changes on disk rather than only in the report. Where a
  `connect` row anchored on such a door, the strip was laid to a doorway that was never carved: its
  `<site>_walkway_<from>__<to>.nbt` is no longer written, the lockfile loses the `walkways:` entry
  for it, and `resolved_ir_hash` moves with the IR. Every `.nbt` for a struct or placement is
  byte-identical — the carve painted air over air — so a rebuild changes nothing an author placed
  in a world except the strip that led nowhere.

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

- *(cli)* `cairn info --format json` wrote zero bytes to stdout when the finding that refused the
  run came from the strict per-edition dry-run:

  ```console
  $ cairn info split.crn --editions java,bedrock --format json > out.json
  $ echo $?
  1
  $ wc -c < out.json
  0
  ```

  `run_info` refuses a source it has read in four ways. Two route through
  `report_failure_document`, which consults `--format`; the other two returned a bare exit code
  from `edition_rows`, so the format was never asked. Those two — a finding only the per-edition
  pass sees, and a palette the pack refuses — were therefore the refusals a JSON consumer could
  not read, on the one command whose job is showing where the editions diverge. The
  edition-neutral gate unions slot names across a theme's per-edition variants, so the divergence
  is exactly what only the strict pass finds.

  `edition_rows` now gives back the findings rather than an exit code, and the caller writes the
  same `{"diagnostics": [ ... ]}` document the other two paths write. Under `--format json` the
  error-severity findings are held for that document, which is written after every requested
  edition has been walked — so a second edition's finding is still not hidden behind the first.
  `--format text` prints byte-for-byte what it printed before, each finding under the note naming
  the edition that raised it.

  Which pass refused still decides what the document carries, and `spec/lint` "Machine-readable
  payload" now says so rather than leaving it to be read off the code. A neutral refusal puts its
  warnings in the document beside the errors; a per-edition refusal carries the errors alone,
  because the neutral warnings have already been reported as text and a per-edition warning
  belongs to a row the run is about to discard.

  A refused palette names no span in the source and no repair its author could make, so it stays
  prose on stderr in both formats, the way that section already reports a run-level refusal. What
  changes is that the document is written at all: a run refused by the palette alone writes
  `{"diagnostics": []}` rather than an empty stdout. The promise now holds on every path where
  `info` refuses a source it has read. It does not reach the mistakes that exit before there is an
  input to write about — an `--editions` value that is empty or names no edition, and a file that
  cannot be read, all still exit 2 with the reason on stderr.

- *(cli,core)* `cairn info --format json` said an edition could build nothing and never said why:

  ```console
  $ cairn info s.crn --editions java --format json | jq '.buildable_targets[0]'
  {
    "edition": "java",
    "buildable": [],
    "considered": ["1.20.4", "1.21", "1.21.4"]
  }
  ```

  Four different things collapse into that empty list: a floor the edition's version table cannot
  place, a floor every release sits below, a scope that produced no voxels, and a lowering the
  pinned version refused. Which ones held was on stderr as prose — and `--format json` exists for
  the consumer reading stdout, so the row it reads carried the figure and none of the answer. That
  is the problem `unsupported: N` had before `unsupported_entries`, and it is answered the same
  way: the row now carries a `reason` beside the list.

  ```console
  $ cairn info s.crn --editions java --format json | jq -c '.buildable_targets[0].reason'
  {"versions":[
    {"version":"1.20.4","refusal":"below_floor",
     "floors":[{"declared":"java version>=1.21.5","line":2,"col":1}]},
    {"version":"1.21","refusal":"below_floor","floors":[ ... ]},
    {"version":"1.21.4","refusal":"below_floor","floors":[ ... ]}]}
  ```

  `reason` reports every cause that holds, not the first one found. Two of the four are facts about
  the *edition* — identical under every release and settled before any release is weighed — so
  `unplaceable_floors` and `dropped_scopes` are fields of their own; the other two differ between
  releases and sit in `versions` beside them, each entry carrying a `refusal` tag of `below_floor`
  or `lowering_refused`. A shape that had to pick one would send the author back for the next cause
  after each repair, which is the loop this row exists to close: a file can declare a floor this
  edition cannot place *and* use an id one of its releases has never had, and one run has to say
  both.

  Saying both meant weighing the versions even where a floor is unplaceable. That walk used to be
  skipped outright, so such a file reported the floor and nothing else — on stdout *or* stderr —
  and the ids surfaced only after the `@requires` line was fixed and the command run again. It now
  runs and `buildable` is emptied afterwards: what an unplaceable floor costs is the certification,
  not the reader's view of the ids.

  `versions` is a subsequence of `considered` rather than a parallel array — a release with nothing
  against it but an edition-wide answer contributes no entry — so a consumer joins on `version`.
  Each tag carries the pieces of its answer rather than a rendered sentence, the way
  `unsupported_entries` does: a floor arrives as `{declared, line, col, declared_by?}` and a
  refused lowering as the findings themselves, in the shape `spec/lint` "Machine-readable payload"
  already gives them. A `below_floor` entry lists only the floors that refuse *that* version, since
  a module declaring several is a file where the repair is one line and not all of them.

  The same per-floor precision fixes a `note:` that was saying something false. The stderr line
  listed every version some floor refused under *each* refusing floor, so a file with a header
  floor `>=1.21.40` and a `def` floor `>=1.21.60` printed "bedrock 1.21.0, 1.21.40 are below the
  `bedrock version>=1.21.40` this file declares" — and `1.21.40` is not below `>=1.21.40`. Each
  note now names the versions its own floor refuses.

  The `reason` key is absent when a version builds, so an ordinary report is byte-for-byte what it
  was. The exit code stays 0, because an edition that builds nothing is something `info` reports
  rather than refuses, and the text rows are unchanged —
  `buildable targets:       Bedrock: none (1.21.0, 1.21.40, 1.21.60 all refuse)` reads the same for
  all four causes. `spec/versioning-editions` "The `buildable targets` row" documents the payload.
  `spec/lint` "Machine-readable payload" is borrowed for the shape of a finding and otherwise
  untouched: this is a row of a report `info` writes on a run it does not refuse, not a diagnostic.

### Breaking changes

- *(cli,core,formats)* `cairn info`'s `degraded` figure counted palette entries it could name and
  did not:

  ```console
  $ cairn info examples/roof-hip.crn --editions bedrock
  edition portability:     Bedrock: portable: 7  degraded: 4  unsupported: 0
  ```

  Which four, and what did each of them lose? The answer was in hand where the counter was raised
  — `translate_states` returns the dropped intent and the fold read only whether the list was
  empty — and it was dropped there. This is the shape `unsupported: N` had before
  `unsupported_entries`, on the one figure of that row still carrying it. A build does name them,
  as `W_INTENT_DEGRADED`, but `info` exists to be read *before* the build.

  ```console
  $ cairn info examples/roof-hip.crn --editions bedrock 2>&1 >/dev/null | head -2
  note: what `degraded: 4` counts on bedrock:
    note: `minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]` — shape=outer_left has no Bedrock state; Bedrock stairs render straight, so corners show visual gaps
  ```

  Two lists rather than one under a category tag: `degraded` and `unsupported` answer different
  questions — what was lost, against why there is no form at all — and keeping them apart is what
  lets each be read against its own figure. `--format json` mirrors the note as
  `edition_portability[].degraded_entries`, one element per unit of the count, in palette order,
  the way `unsupported_entries` already is. `PortabilityReport` gained a second push site so the
  new figure rises only beside its own entry, which is what makes the length *be* the count rather
  than a second tally that has to agree.

  An entry is `{id, states, dropped}`, and the `states` are not decoration. Degradation is a fact
  about the state combination rather than the block: one id reaches the list once per combination
  that loses something, and all four of `roof-hip`'s entries are `minecraft:spruce_stairs`. Keyed
  by id alone the report would have printed the same line four times.

  `dropped` carries `{key, value}` rather than the sentence about them, so a consumer reads which
  property was lost without parsing English out of the field beside it — the rule `UnsupportedReason`
  already states for its own variants. `StateTranslation.degraded` changes from `Vec<String>` to
  carry those pieces, and the wording moves into `bedrock_state::degradation_detail`, which is now
  the only place it is written: `W_INTENT_DEGRADED` composes its form from the same function, so
  the build and the report cannot describe one loss two ways. A value the edition can express
  never reaches the list at all — Bedrock's stairs are `straight`, so `shape=straight` drops
  without an entry.

  The figures are unchanged, the rows on stdout are unchanged, and the exit code is unchanged: the
  names go to stderr beside every other `note:` the command prints. `spec/versioning-editions`
  "The `edition portability` row" documents the payload, with the ja mirror.

  Three Rust API changes come with it, all of them `spec/compatibility` "Internal", where "no
  promise. Any release may change anything." — but breaking for anyone building on the crates.
  `PortabilityReport::into_unsupported` is replaced by `into_entries() -> PortabilityEntries`,
  since a caller now needs both lists and each `into_` would consume the report.
  `StateTranslation.degraded` changes from `Vec<String>` to `Vec<DroppedIntent>`, which is a
  closed set (`DroppedIntent::Shape { value }`) rather than a `{key, value}` pair so that the
  renderer must match on it: the sentence written for a dropped `shape` talks about stairs, and
  the day a second block family drops an intent it must bring its own wording rather than
  inherit one about corners. On the wire that variant is the same `{"key": "shape", "value": …}`
  object. And `EditionPortability` / `EditionReport` gain a `degraded_entries` field; neither is
  `#[non_exhaustive]`, so struct-literal construction of either needs the new field.

- *(redstone,cli)* The per-cell figure delay insertion writes is renamed `delay_ticks` →
  `local_delay_ticks`: on the `PlacementPhase::Delayed` / `Legalized` variants, on
  `PlacementPhase::delay_ticks()` and the `PlacedCellNode` / `PlacedOutputNode` accessors of the
  same name, and as the JSON key in the `cairn synth --stage delay` and `--stage crossing` dumps.

  The old name claimed something the number is not. It is the cell's base delay plus the implicit
  buffer repeaters standing on every net that drives it, **summed over those nets** — what the wires
  feeding one cell cost. A combinational cell's output is ready when the *last* of its inputs
  settles, not after the sum of their arrivals, so a cell whose `a` port comes in over a segment
  carrying one buffer and whose `b` port over one carrying two is recorded at `base + 3` and settles
  at `base + 2`. Read as a latency the figure is off in both directions: it counts the buffers on
  every incoming net where only the slowest matters, and it leaves out everything upstream of the
  cell.

  The sum is kept and the name corrected rather than the other way round, because the sum is what
  stage 4 is held to: `local_delay_ticks - base_delay_ticks` is `BUFFER_REPEATER_TICKS` per block in
  that cell's deduplicated `buffer_coords`, and that identity is the only check the two passes have
  on each other. A `max` would leave the buffers on every shorter segment in no tick figure at all,
  and those blocks are real. Path latency — `assert latency(sig.in -> sig.out)`, spec §14.7 — maxes
  over the incoming nets and walks back through the upstream cells; it belongs to the pass that
  evaluates the assertion against the headless per-tick simulator, and neither is built. The
  assertion form is not parsed today either. The redstone stages reach no compiled artifact; what moves is the name, the accessors, and
  one key in the stage dumps.

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
