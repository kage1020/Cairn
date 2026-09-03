# Changelog

> Language: **English** ([日本語](CHANGELOG.ja.md))

All notable changes to Cairn are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) so `release-plz` can append release
entries cleanly. Cairn uses date-based versioning (CalVer) `YYYY.M[.PATCH]`. This is the version
of the language + reference compiler + standard library + registry/constraint packs as a bundle,
and is a separate axis from the Minecraft target version.

## [Unreleased]

### Breaking changes

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

### Breaking changes

- *(core)* A `floor` or `walls` written without a `mat_slot=` is refused, under a new code
  `E_MISSING_MATERIAL`. Both roles reach the palette through the applied theme's slot map and have
  no default block, so one written without a slot painted nothing — and nothing anywhere said so.
  `cairn check` exited 0 and the structure lowered empty, which is exactly the "implicit dropping"
  `spec/lint` §11.3 forbids.

  **Breaking**: a source carrying such a member compiled before and does not now. The repair is one
  token — name a slot the applied theme declares. Every shipped example already does.

  The rule covers the roles that put nothing anywhere without one, and only those. A `window` or
  `door` with no `mat_slot=` is an **opening** — the rectangle is carved to air, which is how
  `examples/themed-tower.crn` punches arrow slits through a stone wall without choosing a species
  for them — and a `roof`, `stair` or `pressure_plate` paints a default block. None of those is
  reported. The report this fixes listed `window` alongside `floor` and `walls`; that part of it
  was wrong, and the corpus is what says so.

  It is a `check` pass reading the surface key rather than the resolved binding, which is what puts
  it in front of `cairn check` rather than in the dump, lets a module with no theme still be told
  which members name no material, and keeps it from colliding with `E_UNRESOLVED_SLOT` — that code
  needs the key present and unusable, this one needs it absent, and no member earns both.

- *(redstone)* Two nets are kept a step apart, not only off one coordinate. Dust reads the dust
  in the coordinate beside it, so two nets running down adjacent rows are one strand carrying two
  signals as surely as two nets on one coordinate are. The router kept them off one coordinate and
  nothing kept them apart, so every routed scope in the example corpus described a layout with a
  short in it, at exit 0 and with no diagnostic: six such pairs across three nets in
  `crossbar.crn`, two across one in `redstone-door.crn`, on both editions.

  The obstacle set a net routes around now grows from the dust already laid to that dust and the
  four coordinates beside it in its own plane. In its own plane and no further — whether two
  strands a layer apart read each other depends on what is standing between them, which the
  pseudo-2.5D model does not carry, so `spec/redstone` §14.5 now says in as many words that
  separating two strands within one step of each other *across* layers is the physical tile
  layer's obligation rather than the router's. Nine such pairs per edition remain in the example
  corpus after this change, one stacked and eight diagonal, and every one of them is an escape
  landing on or beside the strand it climbed to clear.

  The router cannot own the rule alone, and measuring is what said so. With the cell row on the
  `z = 0` edge, no order `crossbar.crn`'s four nets can be laid in wires the scope — none of the
  24, at every region size measured up to `12x8` with `void=3`, and neither a wider cell spacing
  nor a wider pad spacing changes it. A cell against the edge has one lane beside it, and one lane
  carries one net once dust reaches its neighbour, while a two-input gate has three nets touching
  it. So the placement pass lays the row one row in, at `z = 1`, and the I/O pads step along `z`
  from `0` — the row they were making room for has moved. That is one row for the whole netlist
  rather than one per cell, so unlike the column spacing it does not grow with the cell count.

  **Breaking**: every cell coordinate, `wire_length`, `delay_ticks` and `buffer_coords` in a
  `cairn synth` dump changes; input pad `i` is now `(0, 0, i)` and output pad `k` is
  `(width - 1, 0, k)`; a `circuit` region fewer than three rows deep — the cell row and a clear
  row either side — is refused by the placement pass; and the pad-row refusal counts rows rather
  than rows past the cell row, because a pad stands in a column no cell occupies.

- *(redstone)* Each net is routed around the dust already laid, so two nets no longer share a
  wire coordinate. `spec/redstone` §14.5 specifies the escape — "crossings escape to a `bridge`
  tile or a vertical layer" — and stage 4 performed it for buffer repeaters and never for wire,
  so any two signals meeting on a coordinate came out of the compiler as one strand of dust
  carrying both. It was not a crowded-circuit problem: every crossing in the example corpus came
  from the simplest shape there is, a cell with two inputs, and widening the region did not move
  one of them.

  Two measurements decided the fix. Changing the pad and cell-column conventions removed no
  crossing from the corpus and doubled `crossbar.crn`'s. And the reason is arithmetic rather than
  routing: a cell body is a block, so a net reaches it through a free neighbouring coordinate; a
  two-input gate has three distinct nets touching it and therefore needs three; and packed at
  `x = i` against the pad column, an interior cell of a chain has two — at every region size,
  and the cell at the end of the row has one. Lifting the wire onto a bridge layer does not
  help, because a lifted wire still has to arrive through a face. So the placement pass now
  lays the row at `x = 1 + 2i`, keeping a clear column past the last cell as well, and the router routes
  each net around the dust of the nets before it, climbing to a `bridge` layer where there is no
  way round and refusing the scope where there is no way at all. The escape happens at stage 2,
  which is what gets it measured: `wire_length` and `delay_ticks` are read off the routed tree.

  **Breaking**: cell coordinates move, so every `wire_length`, `delay_ticks` and `buffer_coords`
  in a `cairn synth` dump changes; a `circuit` region that cannot hold `2n + 1` columns for
  its `n` cells — one each, one beside each, and one past the end of the row — is refused by
  the placement pass, with a message naming the columns the row wants;
  `W_WIRE_CROSSING`, `E_CROSSING_CONGESTION` and `E_BUFFER_COORD_COLLISION` are removed — the
  first two because the compiler no longer makes the defect they reported, the third because a
  repeater now stands on its own net's path and has nothing to contest it for;
  `examples/crossbar.crn` grows one column, from `size=5x4` to `size=6x4`.

- *(core)* A `key=` no member of that role reads is refused. A member's arguments were validated
  nowhere: `check` had an allowlist for the statement keyword and nothing for the keys under it,
  so `walls ... hieght=3` exited 0 in silence, and the only thing the author eventually saw was a
  `W_DEFERRED_MEMBER` naming the argument that is now *absent* rather than the one that is
  misspelled. `MemberRole::arguments` is the vocabulary, matched with no wildcard so a new role
  has to be answered rather than inheriting an empty set, and it answers `Option` — an unknown
  keyword has no vocabulary, which is a different thing from an empty one and the reason
  `E_UNKNOWN_KEYWORD` keeps the whole line. `E_UNKNOWN_ARGUMENT` is an error for the same reason
  the keyword code is, one level down: the key names nothing, so no pass will read the value
  however the compiler grows. `compile` now prints the misspelling and then the deferral it
  causes, in that order — the repair before its consequence, where before there was only the
  consequence. A `theme` selector widens the vocabulary of the keyword it names, and of no other:
  `window[tags=...]` makes `tags=` a key the resolver's selector matcher reads on a window, so a
  module that selects on a key is a module where writing it is not a mistake. Writing the table
  down turned up a second, quieter hole — `window shape=` is in `spec/components-editing-sites`
  §9.2, no pass reads it, and the one example using it had been building without it. That is
  `W_IGNORED_ARGUMENT`, whose code already means "the member is in the build; one of its arguments
  is not", rather than a refusal: the key is the author's to write and the gap is the
  implementation's. **Breaking**: every source carrying a misspelled or otherwise
  unrecognised argument key compiles today and is refused now — a key the specification
  defines and no pass reads is *not* among them, and stays a warning; `MemberRole` gains `arguments`, `unread_arguments` and
  `accepted_arguments`; `examples/themed-tower.crn` drops the `shape=slit` its arrow-slit window
  was never built with.

- *(formats,core,cli)* `cairn info` names the palette entries its `unsupported` figure counts.
  The figure was one integer over four failures with four different repairs: the edition has no
  such block at all; it has the block and this compiler maps no states for it yet; a state value
  outside the Java domain reached the state translator; or a state key the translator does not
  read reached it, which is the only one of the four the author can act on and the only one whose
  error already said so. The id and the entry were in hand where the count was incremented and
  both were discarded, so a reader who saw `unsupported: 1` had to bisect the source by hand. The
  counts do not move — this is what they were already counting, named. Entries are listed on
  stderr under the figure, one per unit of it and in palette order, each with a `did you mean`
  read the way `E_UNKNOWN_ID` reads one (path compared inside a single namespace) but drawn from
  every id the edition declares rather than one pinned version's table, so the two can name
  different blocks and each is right about its own question. The four stdout rows do not change:
  they are the text twin of the JSON's top level, and a per-entry list is not the shape of a row.
  The Bedrock arm now matches `BedrockStateError` variant by variant instead of folding every
  failure through a wildcard, so a fourth variant has to be classified here rather than joining
  whichever bucket a `_` pointed at, and each reason carries the fields of its own answer rather
  than a rendered sentence — including the `valid` and `handled` lists, which thread out of the
  translator's own constants so a key or value added there reaches the report without a second
  edit. **Breaking**: `portability_for_java` / `portability_for_bedrock` return a
  `PortabilityReport` (private fields, `counts()` / `unsupported()` / `into_unsupported()`) in
  place of a bare `PortabilityCounts`, keeping one entry point per edition rather than adding a
  second pair that could answer the same question differently;
  `BedrockStateError::UnmappableBlock` and `UnknownStairKey` each gain a field naming the set
  their message lists; and `EditionPortability` / `EditionReport` each gain an
  `unsupported_entries` field. `--format json` gains
  `edition_portability[].unsupported_entries`, additive for consumers that ignore unknown keys.

- *(cli)* `cairn info` says which supported targets can build the source. The portability
  counters ask of the *edition* — a block one part of the range spells differently is not missing
  from it — and two palette entries can be declared by disjoint sets of versions while each
  answers yes. The row was then `unsupported: 0` for a source every supported target refuses. A
  new `buildable targets` row answers per version: the supported versions whose pinned lowering
  raises no error, with the refusing ones named beside them. It is a set rather than a range,
  because two ids whose version sets interleave leave a gap a range would claim, and it is
  derived by lowering once per supported version — the check `cairn compile --target` already
  runs — rather than by intersecting the range-wide palette's id sets. That intersection is
  unsound in the direction that matters: with no target pinned every material takes its default
  mapping, so a theme binding `@floor.stone.smooth` (respelled `stonebrick` at Bedrock 1.21.0)
  beside a literal `@stonebrick` intersects to nothing and builds on 1.21.0. The row reports and
  does not refuse — `cairn info` still exits 0 for a source no version can build, matching the
  rule the portability counters already follow, and the build is the command that refuses. Each
  refusing version's own findings are printed under it, because nothing else in the run would
  show them and a row that says `none` without saying why is not a report. A version counts as
  buildable when it passes the gates `cairn compile --target` applies to the source: the pinned
  lowering raises no error, the `@requires` floor is at or below it, and every scope the source
  declares lowered — a row naming a target `compile` refuses would be the same defect in a new
  place. **Breaking**: `compute_axes` now takes one `EditionReport` per requested edition in
  place of a portability list, so the two per-edition wire rows cannot disagree about length,
  order, or which edition they describe; `VersionAxes` gains a `buildable_targets` field and is
  now `#[non_exhaustive]`, so the next axis is not another break. `--format json` gains
  `buildable_targets`, which is additive for consumers that ignore unknown keys.

- *(core)* A truth table that verifies nothing is reported. `assert truth` exists to check a
  circuit, and three shapes of table checked nothing while reading — in a diff, in a review —
  exactly like one that passes: no rows at all, a pattern assigned twice, and combinations left
  unassigned. Tightening the rows themselves said nothing about the table around them. A table
  with no rows can never assert anything, whatever is written around it later, so it is
  `E_TRUTH_TABLE_EMPTY`; two rows assigning one input combination different outputs describe a
  circuit that cannot exist, so the later of the two is `E_TRUTH_TABLE_CONFLICT`. Both are errors,
  and both refuse sources that exited 0 before. A row that repeats an earlier one and agrees with
  it (`W_TRUTH_TABLE_DUPLICATE_ROW`) and a table short of combinations (`W_TRUTH_TABLE_PARTIAL`)
  are warnings: every row present is still a real constraint, and a four-input table is sixteen
  rows an author part way through is not blocked on. Each repeat answers to the *first* row
  carrying its pattern rather than to the one before it, so every finding about a combination
  sends the author to the same row to compare against — `00->0; 00->1; 00->0` is a conflict and a
  repeat, and `00->0; 00->1; 00->1` is two conflicts. A repeated row fills no combination, so one
  table can be reported as both repeating and partial: two repairs, not one said twice. Which of
  two disagreeing rows would be evaluated is not stated anywhere — the simulator is unbuilt, and
  the repair is to decide which row is wrong either way. `W_TRUTH_TABLE_PARTIAL` carries the
  combinations to write as structured data, a sample of the lowest few rather than the whole set:
  the grammar puts no ceiling on the input list, twenty inputs have a million combinations, and
  the count is arithmetic so nothing walks that space. A table wider than any integer the compiler
  carries reports its total as `2^n`. `TruthRow` gained a span for this, which is what lets a
  finding point at one row and its note at the other.

- *(redstone)* A signal binding whose value names no signal is refused. A binding used to be
  recognised by its *value*, so spelling the key right and the value wrong entered no branch at
  all: `door ... opened_by=a` reached placement as a door wired to nothing, and
  `pressure_plate ... -> foo.bar` produced no diagnostic and no scope. A `-> value` tail now says
  a sensor was meant whatever the value is, and an actuator key says a wire was meant whatever
  the value is, so the value itself is checked — as `E_LOGIC_INVALID_SIGNAL`, the code the
  `logic` left-hand side already took, because all three positions are the same rule about the
  same namespace. Every value kind the parser can put there is covered, the five a name is
  plausibly mistyped as (`a`, `"sig.a"`, `3`, `@tok`, `foo.bar`) and the three that are not near
  misses but are reachable (`true`, `2x2`, `[a,b]`), along with `sig.a.b` — a signal name has two
  segments, which the block-array pass has always required and the front end did not. A bare
  identifier is offered its namespace, `a` having a single reading; `sig` is not, being the
  namespace with the name left off rather than the reverse. The host is asked before the value:
  `walls -> a` is still the host fault and nothing else, since no edit to the value makes `walls`
  carry a tail. The `[selector]` is walked for the first time, and a bracketed pair is answered by
  whichever fault moving it out would not fix — the brackets themselves for
  `door[id=front,opened_by=sig.x]`, the host for `door[id=front,lit_by=sig.x]`, the unknown key
  (with its `did you mean`) for `door[id=front,oepend_by=sig.x]`. §14.2 writes the binding after
  the brackets, and `cairn compile` already refused that shape for the door patch. A key that is
  not a binding and a value that is not a signal reference is still nobody's finding: what an
  unknown argument key means has no answer yet.

- *(lsp)* A `textDocument/didChange` for a URI that is not open is ignored instead of opening it.
  A change after `didClose` used to re-insert the document and publish a fresh diagnostic set — a
  marker on a file the editor has no buffer left to clear — and one for a URI never opened made
  completion available for it. This also covers the case where the server itself dropped the
  `didOpen` because its payload did not match the method's schema: that document now stays
  unknown until the client opens it again, where a keystroke used to revive it.
- *(lsp)* Completion offers nothing inside a string literal. `door id="@oa"` used to answer with
  the whole material catalogue and `door label="pick mat_slot=fl"` with the theme's slot names;
  a string is free-form text, which is where the completion module already promised to invent
  nothing. A cursor past the closing quote completes as before.
- *(lsp)* A completion position one line past the end of the document is refused with
  `InvalidParams` instead of clamped to EOF. The clamp anchored every item's `textEdit` on the
  previous line, producing a range that does not contain the requested position — which editors
  discard, so the answer was already unusable.
- *(core)* A `window` is cut only where the walls actually are. Its rows have to lie inside one
  course of `walls` — `walls height=H` under `level y=N` paints the world rows `N+1 … N+H`, and
  courses that abut merge into one — so `window y=0` and a window in the air between two `level`
  courses now earn `W_DEFERRED_MEMBER` instead of carving a hole through the floor slab and
  hanging glass in open sky. The check used to be the highest wall row alone, which could see
  neither fault.
- *(core)* For a window over top-level `walls`, the port anchors a walkway on exactly the
  rectangles the openings pass carves. A window flush with the top course used to be cut into the
  wall and then refused as an anchor, and one on the ground plane was accepted as an anchor for a
  cut that never happened; both passes now call one predicate.
  `spec/components-editing-sites.md` §9.3.5 states the rule the compiler applies rather than the
  one it did. `walls` under a `level` remain invisible to port resolution, as before — the two
  passes share the predicate, not the column they apply it to.
- *(core)* A roof one course tall seats on the wall instead of capping itself. A short span of 1
  or 2 rises a single layer, and treating that layer as the apex left it entirely `half=top` — a
  half-block slit running the length of a gable and the whole perimeter of a hip, with the hip
  also losing its four `outer_*` corners and every per-edge facing.
- *(core)* An even-span gable's two-stair apex faces away from the ridge, as the generator's own
  comment already said it did. A `half=top` stair fills the upper half of its voxel plus the lower
  quarter on its facing side, so the inward-facing pair left a 0.5 x 0.5 undercut along each outer
  face for the roof's full length. An odd span's single cap keeps the low-slope facing
  (`spec/compilation.md` §4.3) and does not move.
- *(redstone)* A signal carries one driver. Two sensors bound to the same `sig.X` used to keep
  the first and drop the second in silence — the block stayed in the build, wired to nothing —
  and now refuse with `E_LOGIC_MULTIPLE_DRIVERS`. Write the two into names of their own and
  combine them with `logic sig.a = sig.a1 or sig.a2`.
- *(redstone)* `E_LOGIC_MULTIPLE_DRIVERS` anchors at whichever driver is written *later*,
  whether it is a sensor or a `logic` line. A `logic` line above the sensor it collides with used
  to be the line reported; now the sensor below it is.
- *(redstone)* A `-> sig.X` tail is refused on anything but a sensor, and each actuator key on
  anything but the component `spec/redstone` §14.2 pairs it with, as `E_LOGIC_MISPLACED_BINDING`.
  `walls ... powered_by=sig.x` and `window ... -> sig.w` used to become live ports. `lit_by=`,
  `powered_by=`, and `fired_by=` have no legal host until `lamp` / `piston` / `dispenser` become
  keywords, so they are refused wherever they are written.
- *(redstone)* A `logic` left-hand side outside the `sig.` namespace is
  `E_LOGIC_INVALID_SIGNAL`. `logic foo.bar = ...` used to lower a real gate that took a placement
  coordinate for a signal nothing could read.
- *(redstone)* An argument whose value is a `sig.` reference under a key that is not an actuator
  key is `E_LOGIC_UNKNOWN_BINDING_KEY`, with a `did you mean` note when the key is within the
  typo threshold and the list of valid keys otherwise. `door[id=x] oepend_by=sig.y` used to make
  the actuator vanish with only a `W_LOGIC_UNUSED_SIGNAL` left behind.
- *(redstone)* An `assert` naming a signal no sensor emits and no `logic` line defines is
  `E_LOGIC_UNBOUND_SIGNAL`, including in a scope whose only redstone content is that `assert`.
  The simulator that evaluates these is still unbuilt; a property over a name that does not exist
  was never waiting on it. An `assert` also counts as a consumer, so a signal it observes no
  longer earns `W_LOGIC_UNUSED_SIGNAL`.
- *(redstone)* A sensor whose signal nothing reads earns `W_LOGIC_UNUSED_SIGNAL`. A scope whose
  only content is `pressure_plate ... -> sig.a` used to synthesise in silence, which is a plate in
  the build wired to nothing.
- *(redstone)* `SynthOutput::diagnostics` is sorted by span, which is what its doc has always
  promised. `cairn synth` prints them in the order it is handed, so a module whose findings come
  from more than one collection phase reports them in a different order than before.

- *(redstone)* `PlacementIr::outputs` is now `Vec<PlacedOutputNode>` rather than
  `Vec<NetlistOutput>`. An actuator is a placed object: it carries the pad coordinate the
  placement pass assigned and the same `PlacementPhase` a cell does, so the routing, delay,
  and crossing passes fill it by the same rules. Rust API, Internal tier.
- *(redstone)* `cairn synth --stage <placement|route|delay|crossing>` emits a wider object per
  entry of `outputs[]`: `{stage, name, driver, pad}` plus `wire_length`, `delay_ticks`, and
  `buffer_coords` as each stage fills them, where before it emitted `{name, driver}` at every
  stage. `synth` is Evolving and gated behind `--experimental-logic-synth`.
- *(redstone)* `BufferCoord::port` is now a `BufferSegment` rather than a `PortName`, so a
  buffer can name the wire out to an actuator (`"out"`) as well as a cell's input port. The
  wire form stays one flat string; `"out"` is a value it could not previously take.
- *(core)* `pressure_plate` is evaluated in the fixtures phase rather than the openings phase,
  which is where `spec/compilation.md` §4.1 puts a sensor. A plate and a `window` contesting one
  cell used to resolve by whichever line came last; the plate now wins either way.
- *(core)* a structure's palette lists only the blocks its voxels name. A material whose last
  voxel a later phase covered is dropped and the remaining slots renumber, so the `.nbt` palette,
  the per-entry rows `cairn info` reports, and `resolved_ir_hash` all change for any build that
  had one.
- *(core)* two members of one phase writing one voxel to different blocks now emit
  `W_PHASE_CONFLICT`. Last-wins is unchanged — §4.1 mandates it — but a consumer that treats any
  new warning as a failure will see one where it saw none.
- *(redstone)* `logic` bindings are numbered and reported in source order across nesting, so a
  binding inside a `level` no longer takes a lower node index than one written above it at the
  top level. `E_LOGIC_MULTIPLE_DRIVERS` consequently swaps which of the two lines it anchors at.
- *(redstone)* `cairn synth` walks a module's scopes in source order rather than every `struct`,
  then every `def`, then every `site`. Both the order of `scopes[]` in a dump and the order of
  the findings change for any module that interleaves the three.
- *(cli)* `cairn lower`, `cairn info`, and `cairn compile` print a note's own `file:line:col:`
  prefix when the note points at a second place in the source, which is what `cairn check` and
  `cairn synth` already did. A scraper matching notes on a leading `  note:` will not see those
  lines any more.
- *(redstone)* `cairn_lang_redstone::DiagnosticNote` is a re-export of
  `cairn_lang_core::check::DiagnosticNote` rather than a second declaration of the same two
  fields. Source-compatible for anything that only names the path; a consumer that wrote an
  `impl` for the redstone type now writes it for core's, and one that had both is writing it
  twice for one type. Rust API, Internal tier.
- *(cli)* `cairn check --format text` writes its diagnostics to stderr. It was the one build
  command reporting on stdout, so `cairn check f.crn > out` swallowed every finding and left a
  bare exit code behind. `--format json` stays on stdout: that one is the payload, and it is
  redirected deliberately. A *parse* failure still reports on stderr in both formats and leaves
  stdout empty, which this release does not change.
- *(core)* A lockfile carrying a key the schema does not declare is refused, at every depth
  rather than only the top level. A document with `attacker_controlled: yes` beside the required
  fields used to deserialise as `Ok` with `verified: true`; a lockfile is a claim about what was
  built, and one carrying keys the reader ignores is a document whose meaning depends on who is
  reading it.
- *(core)* `Lockfile` declares `lock_schema_version` as its first field, and a document
  declaring a version above the one a build understands is refused by name rather than read as
  if the field names still meant the same thing. A document that omits the key is version 1 —
  the shape that shipped before the field existed — so every lockfile this compiler has written
  still reads. `spec` §10.6's sample carries the field in both languages, and the field order it
  pins moved deliberately.
- *(nbt)* Writing a `List` that has no items but declares an element type other than `TAG_End`
  is refused. `List`'s fields are public, so a constructor is a convenience rather than a funnel,
  and the sibling heterogeneity check runs inside the item loop that an empty list never enters —
  the writer is the one point every byte passes through. `bedrock_structure`'s `block_indices`
  built exactly that shape from a struct literal.
- *(formats)* `pack_hash` length-prefixes every field it covers, which moves
  `inputs.registry_pack_hash` for every pack. Concatenated as it was, the manifest ran straight
  into the first component name and each component's body straight into the next name, so the
  same bytes divided one place to the left or right hashed the same — precisely the rename the
  function's own doc claimed the digest resisted. A separator before the first name would have
  closed half of it; both collisions are pinned as tests.
- *(redstone)* The routing pass draws a net's dust around the components in its way instead of
  through them. It used to build a spanning tree over `{source} ∪ sinks` and render each edge as
  an L-shape, which knew nothing about what was already standing in the reservation: the tree
  reached a far cell *through* the nearer ones — a comparator hands on its own output, not the
  wire that fed it — and an L-shape crossed whatever lay between its ends, pressure plates
  included. Every sink is now a leaf, a fanout is a trunk beside the row with a tap into each
  cell, and no coord of a net's wire is a component that is not one of that net's own ends.
  Three consequences, all visible:
  - Every `wire_length`, every `delay_ticks`, and every buffer-repeater coord moves wherever the
    old wire went through something. `examples/redstone-door.crn` reads `wire_length=5` where it
    read `3`: the second sensor's pad sits behind the first one, and its dust now comes round
    rather than through.
  - A shared bus stops asking for an escape layer it should never have needed. Sixteen cells all
    reading one sensor used to want their repeater on the body of cell #13 and get it lifted onto
    a bridge; the trunk now runs beside the row, so one repeater stands on free wire at
    `(14,0,1)`.
  - A `void=1` scope with two or more cells whose first cell reads a second sensor is refused with
    `E_ROUTE_CONGESTION`. Cell #0 sits in the corner with cell #1 on one side and the sensor pad
    column on the other, and one service layer leaves the second signal no way in. It used to
    compile, with the wire drawn through the pressure plate. The refusal names both coords it
    could not join and points at `void=`.
  - `E_BUFFER_COORD_COLLISION` keeps one cause of the three it used to name. A buffer repeater
    stands strictly between the ends of a route, and the router keeps every coord strictly between
    them off the blocks, so the candidate is never a cell body or a pad any more. What still
    refuses is another net's dust holding the coord with every bridge layer in that column taken —
    and the layers can now be taken by wire, not only by an earlier repeater.

### Added

- *(redstone)* `W_WIRE_CROSSING` names two signals that share a coord of dust. Crossing
  legalization computed those coords already: it refused the scope when the `circuit region=`
  reservation was one layer high, and dropped them when it was taller, on the grounds that the
  reserved service layers left somewhere for a later pass to lift a crossing onto. No pass lifts
  one — and since the routing pass started climbing onto those layers to get past blocks, they are
  not idle either. The reservation's height does not decide whether the two signals merge, so it no
  longer decides whether anything is said: the refusal stays where there is no layer at all, since
  raising `void=` is the one thing that could change the answer, and every other crossing is now
  reported with the pair of signals and the coord they meet on. One finding per pair rather than
  per shared coord, so two nets running side by side are one report. Both redstone examples earn
  findings: `redstone-door.crn` one, and `crossbar.crn` two — the second of which anchors on a
  bridge coord, where two nets that each climbed to clear a block met, so a crossing is not a
  plane-only event and the finding is not named for one. Both still exit 0, and no coord moves,
  because the finding is a report and not a repair. Visible through
  `cairn synth --stage crossing --experimental-logic-synth` only: the crossing pass has no other
  caller, so no stage-4 diagnostic reaches `cairn compile` or `cairn check`.

- *(cli)* `cairn compile` reads the lockfile it is about to replace and reports what changed, as
  `spec` §10.6 describes. A recompile for a different target prints
  `W_PREVIOUSLY_VERIFIED_TARGET`, naming the edition only when that is what moved — two editions
  number their releases differently, so the version pair alone reads as noise across them. When
  the replaced lockfile recorded `member_version_sensitivity` entries, `W_SEMANTIC_SENSITIVITY`
  names them; it is subordinate to the target change, as `spec` §10.6 introduces it — a recompile
  for an unchanged target says nothing even when the lockfile records entries. Nothing is
  synthesised, so the line stays quiet until the constraint-catalog ingest gives it something to
  say. A lockfile written by a newer Cairn is reported in its own words rather than as a corrupt
  one: replacing it loses something, and it is not malformed. A lockfile that does not parse, or that declares a schema
  this build does not read, is reported and replaced instead of discarded in silence. Both are
  warnings and neither changes the exit code, and the read-back covers the default
  `<source>.lock` path, not only an explicit `--lock`.

### Fixed

- *(core)* A `def` member's `E_UNRESOLVED_SLOT` is reported at most once per member per bound
  theme, rather than once per placement — plus once more where the module can auto-pick a theme
  for the def's own scope. A def body is resolved once as its own scope and once again for every
  `place` that instantiates it, so a def placed twice in a single-theme file reported the same
  line, the same code and the same note three times with nothing to tell the copies apart. The
  count was a fact about the placement list rather than about the source the author has to fix.

  Not a blanket dedup, because the resolutions are not copies of one another. Two placements naming
  two themes resolve the same `mat_slot=` against two slot maps, each finding names the theme it is
  about, and both are separate edits — those still both appear. What is recorded is the finding's
  identity, the member and the slot and the theme, and it is recorded at the moment a diagnostic is
  pushed rather than when a body is walked. Two resolutions can bind one theme and still judge a
  slot differently — sibling-variant softening applies to a reference naming the logical theme and
  not to one naming a variant — so a resolution that said nothing must leave the next one free to
  speak. Which of two disagreeing resolutions produced the surviving finding is not promised, and
  `spec/lint` §11.1 now says so.

  A def nothing places keeps its finding, against the theme the module picks. A file of defs and no
  `site` is a file worth checking, and a placement abandoned before it reaches the body — an absent
  `theme=`, an origin that does not resolve — leaves the def's own resolution the only one able to
  report what is inside. `spec/lint` §11.1 now carries the code and the rule.

- *(core,cli,lsp)* A source that does not parse has a diagnostic like every other finding, under a
  new code `E_PARSE`. `--format json` promises a JSON document on stdout and delivered one for
  every input except the ones that fail to parse — the most common way a source fails — where
  stdout was empty and the reason went to stderr as prose, so a consumer parsing stdout had to
  guess from the exit code.

  It fell through because a parse failure had no shape: codes are produced by the check and resolve
  passes, and a parse failure was an error type and nothing else. Every consumer that wanted to
  show one invented its own — the CLI wrote a bare `error:` line in five copies, and the language
  server built its diagnostic directly, with no code at all.

  One code covers every shape a parse fails in, with the message saying which; a code per shape
  would make each future one a line of public contract for a distinction nothing branches on. The
  span runs from the position the parser reports to the end of that line, because a position alone
  renders in an editor as a caret with nothing under it.

  `cairn check --format json` emits the array it always did, with one element in it.
  `cairn info --format json` has a report rather than a diagnostics list as its product, so where
  there is no report it writes `{"diagnostics": [...]}` instead — told apart from a report by its
  keys — and it does that for a check-level error too, which was the same silence one pass later.
  A clean run's report is unchanged.

- *(tree-sitter)* Every line now starts with a token the grammar demands, so indentation is checked
  wherever a line begins rather than only where a level change was already expected. Two shapes the
  grammar accepted and `cairn-lang-core` refuses are refused now: a row indented under a `theme`
  row, which binds a material and opens nothing, and a line indented after a directive. Both were
  the silent direction — the file parsed, and the over-indented row landed somewhere the author did
  not write it.

  The token is hidden and zero-width and stands in front of every construct that begins a line:
  each directive, each top-level declaration, each item of a body. Withholding it is a refusal,
  because nothing else in the grammar can start a construct. It sits *after* the indent and dedent
  tokens rather than before them — before them it could only re-check what the break in front of
  the line already checks, and could not tell a legal level-plus-one from an illegal one, since
  whether a body may open there is the grammar's knowledge and not the scanner's.

  A declaration with no body may now be followed by a blank or a comment-only line. What crosses
  those lines is the scanner reading past them for the next line that carries a level, which it may
  do there because every declaration's body is optional and so an indent is still expected — not
  the new token, which crosses nothing on its own. That was its own refusal before, and it needed
  the newline handling reworked rather than patched. It still is one at the end of a file, where
  there is no construct behind the layout at all.

- *(tree-sitter)* A line may end in whitespace. The grammar refused a trailing space before a line
  break and a blank line made of spaces — three shapes `cairn-lang-core` accepts, and ones an
  editor without trim-on-save writes by accident, so a `.crn` file that compiles could fail to
  highlight.

  The external scanner is consulted before tree-sitter skips the `/ +/` extra, so a run of spaces
  is what it sees wherever one stands. At the start of a line that run is the line's indentation,
  whose length the indent logic needs; anywhere else it is separator whitespace. The two are told
  apart by where the line began, which the scanner already records — so the run is now read once
  and *counted*, and every branch works from the count rather than measuring it back off the
  column afterwards. That is what lets the run be consumed before the line-break handling instead
  of standing in front of it.

- *(core)* The volume a struct lowers into is sized for the members that will paint. Three ways a
  member could shape the array and put no block in it. A `roof` that will not draw gave its `overhang=` to
  the footprint and nothing to the height, because one of the two roof walks asked whether the roof
  would draw and the other only whether the `kind=` was a name it knew — a `5x5` struct with
  `overhang=3` shipped an `11x4x11` array with the walls moved inward and a ring of air around
  them, and not one roof voxel in it. That covers a missing `kind=` and a `kind=shed` with no
  `slope_to=` alike. `walls` whose `mat_slot=` does not resolve raised `Dims.y` by their full height, so
  a struct with no theme bound shipped a `3x7x3` array whose palette is air and nothing else. Both
  walks over the wall list move together: `spec/compilation.md` §4.7 makes the volume and the
  window carve two readings of one list, and their agreement is what keeps a member from painting
  past the end of the array it was handed. §4.7's "does not generalise" paragraph is gone with the
  counterexamples that put it there, replaced by the rule stated per term.

  A `window` or `door` in walls that paint nothing is now reported rather than cutting a hole in
  air, and a `roof` still draws over them — its material falls back where a wall's does not — now
  seated on the ground plane instead of on walls that were never there.

  `Dims` reaches the artifact: a `.nbt` is that many blocks in each axis, the lockfile records the
  figure, and a `place`'s walkway origins are derived from it, so site coordinates move with the
  extent. Sources that were building with a warning ship a smaller structure than they did, with no
  compile error to mark the change, inside `>=2026.8.2, <2027.0.0` (Cargo reads the CalVer `2026`
  as the major). Unlike a `synth` stage there is no experimental gate in front of `lower` or
  `compile`.

  Two shapes reach it, and only one of them says anything. A themeless struct has
  `W_NO_THEME_BOUND`; a `walls` with no `mat_slot=` at all is reported by no pass, so that source
  changes size in silence — the missing diagnostic is tracked on its own. The geometry moves as
  well as the extent: a roof falls back to a material of its own where a wall falls back to air, so
  `walls height=3` under `roof kind=gable` over a themeless struct goes from a ridge above a
  six-high box to one seated on the foundation slab, with nothing between the two outputs to mark
  it.

  It stays under Fixed because the extent that goes is extent nothing ever painted: the old array
  held a building smaller than itself and said nothing about the difference.

- *(core)* `W_IGNORED_ARGUMENT`: a `key=` the lowering pass could not read, on a member it went on
  without. `roof kind=gable overhang=nope` reported `W_DEFERRED_MEMBER`, which says the member did
  not lower — and the roof was in the build, flush with the wall line, exactly as if the
  `overhang=` had not been written. The new code says the value was ignored, and its note says what
  that did: the roof drawn flush, or nothing at all when the same roof was going to be dropped for
  another reason. Severity is unchanged from the code it replaces, so no exit code moves;
  `spec/lint.md` §11.3 would read a dropped value as an error, and promoting it is the same call
  an unknown argument key is waiting on.

- *(redstone)* One buffer repeater per strand of dust, and one charge for it. A Steiner tree's
  sinks share their prefix, so two segments of one net compute the same repeater candidate: two
  ports of a cell reading one signal, two cells hanging off the same 15-block point, a cell and an
  actuator. Only the actuator segments recognised the repeater already standing there. A cell
  segment escaped around it onto a `void=<N>` bridge layer — a second block on a strand of dust
  that has one — and a candidate that had been *lifted* onto a bridge was recognised by nobody, so
  the next segment of the same net lifted a second block over the same point and the two together
  exhausted the reservation. A shared bus of 16 cells was refused outright with
  `E_BUFFER_COORD_COLLISION` at `void=2`, for layers it did not need; it now legalizes, with every
  cell past the 15-block point naming the one repeater.

  The three passes that describe a cell's incoming wire also agree now that two ports reading one
  signal are one strand, and both figures count each driving net once. `logic sig.s0 = sig.a and
  sig.a` reported `wire_length: 2` for the one block of dust between the pad and the cell. On a
  segment long enough to need a repeater, `delay_ticks` charged that repeater once per port —
  for this two-port cell, twice the delay of the one block standing on the wire. `BufferCoord`'s doc states the reading
  its `{port, coord}` shape has always had — the vector is an attribution list, one entry per
  segment per repeater that segment passes through, so a coord repeats when one block serves
  several segments and a consumer counting blocks deduplicates by coord.

  These figures are printed, and their values change here with no signal to a consumer reading
  them: `cairn synth --stage route|delay|crossing` reports `wire_length`, `delay_ticks` and
  `buffer_coords`, and sources that were refused now exit 0 — inside `>=2026.8.2, <2027.0.0`,
  because Cargo reads the CalVer `2026` as the major and a month bump ships it. It stays under
  Fixed for two reasons. The old numbers described a layout nobody could build: two blocks
  standing on one strand of dust, and a signal charged for passing through each of them more than
  once. And `cairn synth` refuses to run without `--experimental-logic-synth`, whose whole purpose
  is that the shape of this output is outside the stable compatibility tier.

- *(core)* A theme selector whose attribute holds a list selects the members that carry that list.
  `ast::Value` compared its source span as well as its kind, and `ValueKind::List` holds `Value`s —
  so the derived equality recursed through that comparison and two lists spelled identically on two
  lines were never equal at any depth. A list-valued selector attribute therefore matched no member
  at all, and the author saw `E_THEME_SELECTOR_UNMATCHED`, which reads as "your filter is too
  narrow" rather than "this attribute type cannot match anything". Two byte-identical list-valued
  rows are now also recognised as a duplicate pair by `E_DUPLICATE_SELECTOR`, which inherited the
  same comparison and under-reported. Equality on `Value` is now its kind's, which is what
  `#[serde(transparent)]` already said the value was; the types that wrap one carry their own span
  and compare it, so their equality is unchanged as long as the two spans agree — which they do
  for everything `lower` builds.

  This is a behaviour change in published API that reaches a consumer without a compile error.
  `ast::Value` is public and the workspace version is CalVer, so Cargo reads `2026` as the major
  and a month bump ships inside `>=2026.8.2, <2027.0.0`. Code downstream that compares ASTs with
  `assert_eq!`, or calls `dedup` / `contains` on a `Vec<Value>`, gets the new answer with no
  signal. It stays under Fixed because the old answer was the defect: a list that is never equal
  to the same list written on another line matches no documented contract, and depending on it
  was depending on the bug.
- *(lsp)* A message arriving between `shutdown` and `exit` no longer kills the server. Anything
  but `exit` used to be a protocol error that ended the process with code 1 before the `exit`
  behind it was read, so an editor reported the language server as crashed and restarted it —
  and `$/cancelRequest` may arrive at any time, while a closing window sends `didClose` on its
  way out. Requests after `shutdown` are now answered `InvalidRequest`, notifications are
  ignored, and `exit` leaves with code 0 (still non-zero without a preceding `shutdown`).
- *(docs)* `cairn-lang-wasm`'s README documented `wasm-pack build` as working. The crate has no
  `wasm-bindgen` dependency and `cairn_version` carries no export attribute, so wasm-pack refuses
  it and a plain `wasm32-unknown-unknown` build produces a module with no callable export. The
  README, the crate docs, and the function's own doc line now say so.
- *(cli, lsp)* `cairn --version`, `cairn-lsp --version`, and every lockfile's `cairn_version`
  report the release that built them. The number was a hand-maintained constant in
  `cairn-lang-core` and had stopped tracking the workspace altogether: a `2026.8.2` build
  answered `2026.7`, which is not a released version at all, and wrote that into the one field
  the lockfile exists to make trustworthy. It is now read from the crate's own package version,
  and the tests compare it against the version cargo derived for the calling crate, so a
  constant that stops tracking the workspace fails from the other side of a crate boundary.
  Two things that shape cannot see: `[workspace.package] version` itself drifting from the tag
  that was released, since both sides of the comparison move with it, and the constant being
  reset to a literal equal to today's number, which only diverges at the next bump.
- *(tree-sitter)* The npm package ships `tree-sitter.json`. `package.json`'s `files` array left
  it out, and since tree-sitter CLI 0.24 that file is the sole declaration of `file-types` and
  the query paths — so an editor installing `tree-sitter-cairn` from the registry found no
  language for `.crn` at all. `tree-sitter parse` kept working off `src/grammar.json`, which is
  why nothing caught it, and nothing running inside the repository could: the file is on disk
  whether or not it is published. CI now packs the tarball, unpacks it outside the checkout, and
  highlights a `.crn` through it.
- *(vscode)* The extension manifest declares the workspace version. Its own changelog says
  extension versions track the CLI's CalVer tag, but the release pipeline aligned only the
  tree-sitter manifests, so the extension had been left at `2026.7.2` — two releases back. The
  alignment step now rewrites it too, `release-patch` refuses to publish while any copy of the
  version disagrees with `[workspace.package]`, and a CI job compares them on an ordinary pull
  request. The internal crate requirements in `[workspace.dependencies]` were a copy of the same
  kind, written from the number guessed before release-plz chose one and never corrected; they
  are aligned and covered too.
- *(ci)* The VS Code extension and the documentation site are built. Neither was referenced
  anywhere in the workflows, so a TypeScript error in the extension or a Starlight page that
  fails to render could reach the integration branch without turning a check red.
- *(core)* A written lockfile ends with a newline. `serde_yml` closes an empty flow sequence
  without a line break and `member_version_sensitivity` is the last field, so every lockfile
  written for a source with no sensitivity entries — which is all of them so far — ended at `]`
  mid-line, making `git diff` report `\ No newline at end of file` on every change and an
  appended byte corrupt the document.
- *(nbt)* `List::of_ints([])` and `List::of_compounds(vec![])` declare `TAG_End`. The element id
  describes the items on the wire and an empty list has none; `tag.rs` says so and `List::empty`
  already wrote `0`, so which constructor was called decided what an empty list claimed about
  itself.
- *(formats)* Both structure backends refuse a voxel whose palette index is not a slot of the
  palette, instead of writing it as an `i32` naming a slot the reader has to invent. Unreachable
  through the CLI, where `Palette::intern` is the only source of an index — but `BlockArray`'s
  fields and both `build_*_tag` entrypoints are public.
- *(cli)* `compile --help` no longer says the Bedrock backend emits stateless palettes only and
  treats a stateful entry as a hard error. The same help page's `--edition bedrock` text already
  said otherwise, and so does the code: compiling `examples/roof-hip.crn` for Bedrock exits 0,
  warns `W_INTENT_DEGRADED`, and writes `weirdo_direction` and `upside_down_bit`.
  `lower --help` and `info --help` now document the exit-1 path for an `Error`-severity
  diagnostic, which both have always taken.
- *(docs)* `cairn-lang-formats`'s README listed a `BedrockStructureError::StatefulPaletteEntry`
  that has never existed and repeated the stateless-palette claim in three places. The crate is
  published, so a consumer could have written a match arm against a variant they would never
  receive.

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

### Added

- `PlacementPhase` gains fallible mirrors of its three transitions:
  `try_route` / `try_delay` / `try_legalize`, each returning
  `Result<(), PlacementPhaseTransitionError>`. Panicking on an
  out-of-order transition is the right shape for the pipeline passes,
  where a wrong-order call in a fresh compile is always a caller-side
  bug with no recovery path — but it stops being right for the
  consumers that do have one: a cache validator that turns a stale
  entry into a rebuild-from-scratch decision, an IR ingest that must
  refuse a malformed dump with a diagnostic, a language server that
  cannot take a long-lived process down over one bad call. The
  pipeline keeps calling the panicking forms, which are now their
  `try_*` mirror plus a panic, so which transitions are legal is
  stated once and the two forms cannot disagree. The error carries the
  whole offending phase rather than just its variant name, so a
  consumer can see how far the cell actually got, and so the error's
  `Display` reproduces the panic wording byte for byte — pinned by a
  test that compares the two live rather than against a hard-coded
  copy. `PlacementPhaseTransitionError::with_context` splices a
  caller-supplied cell identity into the same position `route_at` and
  friends put theirs, so an ingest diagnostic and a pipeline panic
  about the same cell read alike. A refused transition leaves the
  phase exactly as it found it, since unlike a panicking caller a
  recovering one goes on to use it. `PlacementPhase` and the new error
  type are re-exported from the crate root.

### Changed

- The three `PlacementPhase` transitions now state one cardinality.
  `route` and `delay` panicked with "must run once per …" while
  `legalize` said "must run **at most** once per delayed IR", wording
  inherited from the release-loud `assert!` the crossing pass carried
  before the phase enum existed; all three now say "must run
  **exactly** once per …". Beyond the inconsistency, "at most once"
  understated what the guard checks: `legalize` also refuses a phase
  that never reached `Delayed`, so a skipped stage trips it just as a
  repeated one does, and only "exactly once" says so. The doc comments
  on `route` and `delay` already read "exactly once", so the panics now
  agree with the contract they document. The cardinality clause moves
  out of the three call sites into a single `TRANSITION_CARDINALITY`
  const that every transition message splices between the offending
  pass and the phase it consumes — a transition added beside these
  three picks the two nouns but not how strong the guard claims to be,
  which is what let the wording drift in the first place.

- Every `PlacedCellNode` in the JSON dump gains a leading `"stage"`
  key naming the place-and-route pass that last wrote to it —
  `placement` / `route` / `delay` / `crossing`, the same vocabulary
  `cairn synth --stage <s>` accepts, so a dump names the flag that
  produced it. Until now the stage had to be inferred from which
  optional keys were present, and that inference was not total: a
  `PlacementPhase::Delayed` cell and a `Legalized` cell whose crossing
  pass materialised zero buffers serialise to exactly the same keys,
  because an empty `buffer_coords` serde-skips. A consumer parsing the
  JSON therefore could not tell a stage-3 dump from a stage-4 dump with
  nothing to legalize. The tag resolves that without promoting the
  empty vector to a sentinel — `buffer_coords` still elides when
  empty. The "stage-N dump is a pure additive subset of the
  stage-(N+1) dump" contract the entries below describe is
  correspondingly relaxed to "additive subset apart from the `stage`
  tag": the tag is the one field whose *value* changes from stage to
  stage rather than appearing for the first time. The tag is derived
  from the phase on every serialisation rather than stored, so it
  cannot drift from the variant it names.
- `PlacementPhase::Legalized::buffer_coords` widens from
  `Vec<CellCoord>` to `Vec<BufferCoord>`, where the new
  `BufferCoord { port: PortName, coord: CellCoord }` pairs every
  implicit buffer repeater the crossing pass materialised with the
  driver port on the owning cell that the buffer sits on the segment
  for. The crossing pass already iterated `cell.drivers` when picking
  buffer coords, but dropped the port on the way out; a downstream
  block-array voxel lowering had to re-derive it from
  `drivers[i].net → source coord → floor((s - 1) /
  DUST_ATTENUATION_LIMIT)`. Attribution is now carried alongside the
  coord so the lowering can group buffers by driver directly. The JSON
  wire form of a non-empty entry shifts from
  `{"x":..,"y":..,"z":..[,"layer":..]}` to
  `{"port":"a","coord":{"x":..,"y":..,"z":..[,"layer":..]}}`, matching
  the `{port, ...}` shape `CellPortDriver` already uses on the netlist
  side. Empty `buffer_coords` still serde-skips, so a scope whose
  delay pass counted zero buffers stays byte-identical to its delayed
  IR apart from the `stage` tag above.
  `PlacedCellNode::buffer_coords()` and
  `PlacementPhase::buffer_coords()` now return `&[BufferCoord]`;
  `PlacementPhase::legalize` takes `Vec<BufferCoord>`.
- `PlacedCellNode`'s three progressive fields (`wire_length`,
  `delay_ticks`, `buffer_coords`) that M6-PR5 / M6-PR6 / M6-PR7 above
  added as parallel `Option` / `Vec` fields are collapsed into a
  single `phase: PlacementPhase` enum whose four variants (`Unrouted`,
  `Routed`, `Delayed`, `Legalized`) correspond one-to-one with the
  first four stages of the place-and-route pipeline. Illegal states
  such as "carries `delay_ticks` but no `wire_length`" or "populated
  `buffer_coords` before delay ran" are unrepresentable — each stage
  transition is expressed by `PlacementPhase::route` / `delay` /
  `legalize`, which pattern-match the current variant and panic on
  any out-of-order call (replacing the earlier scattered
  `debug_assert!` / release-`assert!` guards on the three passes with
  a uniform release-loud contract). `PlacementPhase` is
  `#[non_exhaustive]` so a future Stage-5 `EditionLegalized` variant
  is additive on the downstream `match` sites the enum's accessors
  do not already cover. The `phase` field on `PlacedCellNode` itself
  is `pub(crate)`: downstream consumers see only the flat accessor
  methods `wire_length()` / `delay_ticks()` / `buffer_coords()`,
  which return the same `Option<u32>` / `&[CellCoord]` shape the old
  fields exposed, and a hand-written `Serialize` impl flattens the
  phase back onto
  `{stage, cell, drivers, coord[, wire_length][, delay_ticks][, buffer_coords]}`
  so the values keep the flat spelling earlier revisions produced
  rather than becoming a tagged enum object; the only wire-form
  addition is the `stage` key described above.

### Added

- `PlacementStage` — the four-variant projection of `PlacementPhase`
  that backs the `"stage"` key described under *Changed*, exported
  from `cairn-lang-redstone`'s root alongside the other Placement IR
  types. `PlacementPhase::stage()` and `PlacedCellNode::stage()`
  return it; `PlacementStage::as_str` fixes the wire spelling
  (`placement` / `route` / `delay` / `crossing`) in one place the way
  `RouteLayer::as_str` already does for the layer vocabulary — the
  `--stage <name>` fragment `cairn synth` prints when it refuses a
  missing `--edition` now derives its four Placement spellings from
  the same accessor instead of repeating the literals, and a unit
  test reads the third spelling in that chain (the one clap derives
  from the `SynthStage` variant identifier, which no type ties to
  either) back out of `ValueEnum` so a variant rename cannot silently
  desynchronise the accepted flag from the emitted tag. Unlike
  the three value accessors it sits beside, `stage()` is total: every
  phase belongs to exactly one stage, including a `Legalized` with no
  buffer coords to show for itself. Both it and the `Serialize` impl
  enumerate every variant explicitly rather than falling through a
  `_ =>` arm, so adding the Stage-5 variant is a compile error at the
  two sites that must name it rather than a dump that silently
  mislabels stage-5 output as `crossing`. `PlacementStage` is
  `#[non_exhaustive]` for the same reason `PlacementPhase` is.
- `PlacementPhase::route_at` / `delay_at` / `legalize_at` — context-
  carrying twins of the three phase-transition methods, added so an
  out-of-order transition panic names the cell that tripped it. The
  existing methods already carried `#[track_caller]`, which puts the
  calling `.rs:line` in the backtrace but says nothing about *which*
  cell was already routed / delayed / legalized, leaving an operator
  to walk back from the backtrace into the IR to find out. The `_at`
  forms take any `Display` and splice it into the panic between the
  offending phase and the invariant clause, so the routing, delay,
  and crossing passes now fail with e.g. `PlacementPhase::legalize
  called on Legalized { .. } for cell #0 at (16,0,1) in struct
  `twice` — crossing legalization must run exactly once per delayed
  IR`. The breadcrumb is rendered in the same vocabulary the pass
  diagnostics already use (`cell #{index}`, `({x},{y},{z})`,
  ``{kind} `{name}` ``), built from the cell's position in
  `PlacementIr::cells`, its placement coord, and the owning scope —
  `PlacedCellNode` carries no source-level name, so that triple is a
  cell's only stable identity. The coord's `layer` renders only when
  it is not `RouteLayer::Plane` — which for a cell coord is never,
  since the placement pass stamps `Plane` and no later pass moves a
  cell body off it — so the common rendering stays short without
  letting a hand-built IR that breaks the invariant print a coord
  that reads as a plane coord. The context-free `route` / `delay` / `legalize` forms
  differ from their `_at` twins by the identity clause and nothing
  else: an absent context drops the whole ` for …` clause rather than
  rendering an empty one, so no stray separator reaches the message.
- Redstone crossing legalization + `cairn synth --stage crossing
  --edition <java|bedrock>` (M6-PR7) — the seventh slice of the M6
  redstone-simulates pipeline. `cairn-lang-redstone` grows a
  `compile_crossing(&ScopedPlacementIr) -> CrossingOutput` entry
  point that walks the delayed Placement IR produced by M6-PR6,
  re-derives every net's Manhattan Steiner tree from the same
  `NetRef → source coord` mapping the routing and delay passes use
  (routing discards its per-scope occupancy set before yielding the
  routed IR, and pushing wire coords into the shared IR would bloat
  every consumer's JSON dump), and carries out two tasks —
  stage 4 of the five-stage place-and-route pipeline (Placement →
  Steiner routing → Delay insertion → Crossing legalization →
  Edition legalization).
  Task 1 is plane-crossing detection: a wire coord (neither cell
  nor pad) owned by two distinct nets is refused with the new
  `E_CROSSING_CONGESTION` code when the `void=<N>` reservation
  offers no y-layer above the plane (`void < 2`). v1 does not lift
  the wire crossing itself onto a `Bridge` layer — the routed wire
  path is not carried on the IR, so an escape record would have
  nowhere to attach; the crossing coord set is instead used inside
  the pass to steer task 2. Task 2 is implicit buffer repeater
  coord assignment: for every driver segment the delay pass counted
  buffers on, the L-shape path is re-walked (x → z → y, matching
  the routing pass's axis order) and buffers land at
  `k * DUST_ATTENUATION_LIMIT` (`k = 1..=buffer_count`); a
  candidate that collides with a cell coord / pad coord / plane
  crossing / earlier buffer escapes to the first free
  `RouteLayer::Bridge` y-layer inside the `void=<N>` budget
  (`y in 1..void`), and if every bridge y-layer at that `(x, z)`
  is taken the pass refuses with the new `E_BUFFER_COORD_COLLISION`
  code. Both diagnostics carry the self-correction triple
  ("increase `void`", "enlarge region", "split into multiple
  `circuit` blocks") and the `CrossingCongestion` primary names the
  two conflicting nets at the anchor coord so a downstream reader
  can locate the source-level signals responsible.
  No new IR type joins the crate: the crossing pass is a field
  write per the phase table on `PlacedCellNode`. `CellCoord` grows
  a `layer: RouteLayer` tag (`Plane` / `Bridge` / `Via`; `Via` has
  no producer in v1 and is documented as reserved), and
  `PlacedCellNode` grows `buffer_coords: Vec<CellCoord>` that the
  crossing pass fills with one entry per implicit buffer repeater
  the delay pass counted. Both fields serde-skip on their defaults
  (`layer` skips when `Plane`, `buffer_coords` skips when empty),
  so a placement / routing / delay JSON dump is an additive subset
  of the legalized IR dump apart from the `stage` tag — no key
  changes for scopes with nothing to legalize, only a tag whose
  value moves on. Failed scopes elide from the output so a
  downstream block-array voxel lowering cannot silently materialise
  buffers against a layout no other stage can realise. The CLI's
  `cairn synth --stage` gains a `crossing` value; the
  `--edition <java|bedrock>` flag is required in that mode
  alongside `edition` / `placement` / `route` / `delay`, and stays
  refused on the edition-neutral `logic` / `netlist` stages (exit
  2). `--stage crossing` inherits upstream fail-loud: a scope that
  trips `E_ROUTE_CONGESTION` at routing or `E_ATTENUATION_LIMIT` at
  delay insertion is reported and exits 1 before the crossing pass
  runs. Not in scope for this PR: wire-crossing `Bridge` / `Via`
  materialisation on the IR, edition legalization, block-array
  voxel lowering, the physical-tile (tier 3) cell library, the
  tick simulator, `assert truth|always|latency` evaluation,
  sequential macros (`latch` / `pulse` / `delay` / `edge_*` /
  `counter`), and QC/BUD refusal (`E_NO_PORTABLE_IMPL`).
- Redstone delay insertion + `cairn synth --stage delay
  --edition <java|bedrock>` (M6-PR6) — the sixth slice of the M6
  redstone-simulates pipeline. `cairn-lang-redstone` grows a
  `compile_delay(&ScopedPlacementIr) -> DelayOutput` entry point that
  walks the routed Placement IR produced by M6-PR5 and promotes each
  cell's `delay_ticks` from `None` to `Some(base delay + implicit
  buffer repeater ticks)` — stage 3 of the five-stage place-and-route
  pipeline `spec/redstone` §14.5 describes (Placement → Steiner
  routing → Delay insertion → Crossing legalization → Edition
  legalization). No new IR type joins the crate: the delay pass is a
  field write per the phase table on `PlacedCellNode`, symmetrical to
  the routing pass's `wire_length` write. Base delay is a `const fn
  base_delay_ticks(self)` sibling of `EditionCell::edition(self)` so
  the numbers live next to the cell library they characterise: Java
  `ComparatorAnd` / `RepeaterOr` / `InverterTorch` and Bedrock
  `InverterTorch` carry 1 tick each, Bedrock `TorchAnd` carries 2
  ticks (two-torch NAND→NAND stacked in series), Bedrock `TorchOr`
  carries 0 ticks (bare dust merge), and every `*Unpinned` variant
  returns `UNPINNED_BASE_DELAY_TICKS` (3 ticks — strictly above the
  currently-pinned max of 2) so a future pinned rename is a one-arm
  swap that cannot silently degrade delay accounting or blend in with
  a pinned value already in the table.
  Implicit buffer repeaters cover driver segments that would breach
  the 15-block dust attenuation limit: for a segment of `s` blocks
  the pass counts `floor((s - 1) / DUST_ATTENUATION_LIMIT)` buffers,
  each contributing `BUFFER_REPEATER_TICKS` (1 tick, matching the
  default `repeater delay=1` setting). Buffers are counted, not
  materialised — the routing pass already discarded its per-scope
  occupancy set, and stage 4 (crossing legalization) is the natural
  owner of buffer coord assignment because it also needs to escape
  cross-net overlaps into a `RouteLayer::Bridge` / `Via` layer. The
  new `E_ATTENUATION_LIMIT` code fires only when a single driver
  segment exceeds `MAX_ATTENUATION_SEGMENT` (256 blocks — 16
  back-to-back buffers), the sanity cap past which stage-4
  bridge/via geometry becomes unavoidable; segments in the
  `(DUST_ATTENUATION_LIMIT, MAX_ATTENUATION_SEGMENT]` band are normal
  and absorbed by implicit buffers. Per-driver Manhattan segments
  are recomputed from the same `NetRef → source coord` mapping the
  routing pass uses (the routing pass stored only the driver-sum
  `wire_length`, deliberately — per-driver segments are cheap to
  re-walk and would bloat the JSON if stored twice); `input_pad` /
  `output_pad` / `manhattan` are promoted to `pub(crate)` so the
  routing pass stays the owner of the pad-coord convention until a
  subsequent PR promotes it to a `PlacementIr` field. Failed scopes
  elide from the output so a downstream tick simulator cannot silently
  consume a partial `delay_ticks` set. The CLI's `cairn synth
  --stage` gains a `delay` value; the `--edition <java|bedrock>`
  flag is required in that mode alongside `edition` / `placement` /
  `route`, and stays refused on the edition-neutral `logic` /
  `netlist` stages (exit 2). `--stage delay` inherits upstream
  fail-loud: a scope that trips `E_ROUTE_CONGESTION` at routing is
  reported and exits 1 before the delay pass runs. Not in scope for
  this PR: crossing legalization and the `RouteLayer::Bridge` /
  `Via` escape, edition legalization, block-array voxel lowering,
  the physical-tile (tier 3) cell library, the tick simulator,
  `assert truth|always|latency` evaluation, sequential macros
  (`latch` / `pulse` / `delay` / `edge_*` / `counter`), and QC/BUD
  refusal (`E_NO_PORTABLE_IMPL`) — each remains a follow-up that
  will build on the delayed Placement IR shape this PR pins.
- Redstone Steiner routing + `cairn synth --stage route
  --edition <java|bedrock>` (M6-PR5) — the fifth slice of the M6
  redstone-simulates pipeline. `cairn-lang-redstone` grows a
  `compile_routing(&ScopedPlacementIr) -> RoutingOutput` entry point
  that walks the Placement IR produced by M6-PR4 and lays a
  rectilinear Manhattan Steiner tree per driver net inside every
  scope's `circuit region=` reservation — stage 2 of the five-stage
  place-and-route pipeline `spec/redstone` §14.5 describes
  (Placement → Steiner routing → Delay insertion → Crossing
  legalization → Edition legalization). No new IR type joins the
  crate: the routing pass is a field write per the phase table on
  `PlacedCellNode`, promoting every cell's `wire_length` from `None`
  to `Some(sum of Manhattan distances from each driver source into
  the cell)`. `delay_ticks` stays `None` at this stage because §14.4
  ties delay to the routed wire length plus the physical cell choice
  — that is stage 3's concern. The v1 algorithm keeps to the smallest
  set of concepts that still exercises the shape the follow-up passes
  need: net collection ("source coord → sink coords" per NetRef),
  Kou-Markowsky-style rectilinear MST (Kruskal over the complete
  Manhattan graph on `{source} ∪ sinks`, deterministic weight/index
  tie-break so the regression story pins), L-shape rendering
  (x-then-z-then-y for stability), a per-scope `HashSet<CellCoord>`
  occupancy set seeded with every cell coord + input / output pad,
  and a wire-only footprint sum for the congestion budget. Input pad
  coordinates land at `(x=0, y=0, z=1+i)` and output pad coordinates
  at `(x=width-1, y=0, z=1+k)`, both saturating at `depth-1` for
  degenerate regions — a v1 convention that stays crate-private today
  and joins `PlacementIr` as `#[non_exhaustive]`-safe `input_pads` /
  `output_pads` fields once a subsequent PR needs them outside
  routing. The existing `E_ROUTE_CONGESTION` code fires again here,
  now against the actual post-routing footprint (`cells.len() *
  CELL_FOOTPRINT + unique wire coords > reserved_area`) rather than
  the cell-only pessimistic budget the placement pass used — the
  primary reads `routed netlist occupies ~N.Mx the reserved area
  (void=V, region WxD)` so a downstream reader can tell placement's
  fail-loud apart from routing's; the footer keeps the §14.5
  three-fix triple verbatim. Placement's pessimism (cells × 4) means
  a scope routing to `E_ROUTE_CONGESTION` almost always packed cells
  right at the reservation boundary and needed only a Manhattan step
  of new wire to flip — the intentional cost model, not a
  double-detection oversight. Failed scopes elide from the output so
  a downstream pass cannot silently consume a partial routed layout,
  matching the fail-loud cascade policy the earlier stages use. The
  CLI's `cairn synth --stage` gains a `route` value; the `--edition
  <java|bedrock>` flag is required in that mode alongside `edition`
  and `placement`, and stays refused on the edition-neutral `logic`
  / `netlist` stages (exit 2). Not in scope for this PR: delay
  insertion (repeater buffers), attenuation-limit detection
  (`E_ATTENUATION_LIMIT`, dust segments > 15), crossing legalization
  and the `RouteLayer::Bridge` / `Via` escape, edition legalization,
  block-array voxel lowering, the physical-tile (tier 3) cell
  library, the tick simulator, `assert truth|always|latency`
  evaluation, sequential macros (`latch` / `pulse` / `delay` /
  `edge_*` / `counter`), and QC/BUD refusal
  (`E_NO_PORTABLE_IMPL`) — each remains a follow-up that will build
  on the routed Placement IR shape this PR pins.
- Redstone Placement IR + `cairn synth --stage placement
  --edition <java|bedrock>` (M6-PR4) — the fourth slice of the M6
  redstone-simulates pipeline. `cairn-lang-redstone` grows a
  `compile_placement(&ScopedEditionNetlistIr, &IntentModule)` entry
  point that walks the Edition Netlist IR produced by M6-PR3 and lays
  each edition-tagged cell out inside its scope's `circuit region=`
  reservation — stage 1 of the five-stage place-and-route pipeline
  described by `spec/redstone` §14.5 (Placement → Steiner routing →
  Delay insertion → Crossing legalization → Edition legalization).
  Cells are placed in the topological order the Edition Netlist IR
  already carries (`NetRef::Cell(j)` in `cells[i]` satisfies `j < i`),
  stamped with `x = i`, `y = 0`, `z = 0` — a 1D layout the routing
  pass will lift to pseudo-2.5D once crossings and fanout enter the
  picture. `wire_length` and `delay_ticks` are reserved as `Option`s
  on `PlacedCellNode` and stay `None` at this stage because §14.4
  ties delay to the actual routed wire length, which is the routing
  pass's output; a follow-up PR fills them in as a value change, not
  a schema change, so downstream JSON consumers see a stable wire
  shape today. `CircuitRegionReservation` captures the `region=<label>
  void=<N>` reservation together with the enclosing scope's
  `size=WxH` footprint copied verbatim from the Intent IR so the
  routing pass has one type to consume. Two new diagnostic codes fire
  per `spec/lint` §11's self-correction triple:
  `E_NO_CIRCUIT_REGION` when a scope has cells to place but declared
  no `circuit region=` line (or the enclosing scope has no `size=`),
  and `E_ROUTE_CONGESTION` when the netlist needs more area than the
  reservation offers — the primary quotes the ratio and reservation
  shape (`synthesized netlist needs ~1.3x the reserved area
  (void=1, region 3x3)`), the footer names the three fixes §14.5
  suggests (`increase void, enlarge region, or split into multiple
  circuit blocks`). Congestion / missing-region failures elide the
  offending scope from the output so a downstream consumer cannot
  silently accept a partial layout, matching the fail-loud cascade
  policy the synth pass uses on unbound signals. `cairn-lang-core`
  gains a small `intent::circuit_regions(&IntentModule) -> Vec<CircuitRegion>`
  API that lifts the already-validated `circuit region=` fixtures out
  of the Intent IR so the redstone crate has one entry point instead
  of re-parsing `member.intent_state` in a second place — the
  block-array pass's `recognize_circuit_region` still owns the
  per-shape `W_DEFERRED_MEMBER` diagnostics, so the two consumers
  agree on the happy-path shape without either firing a duplicate
  diagnostic for the same source line. The CLI's `cairn synth
  --stage` gains a `placement` value; the `--edition <java|bedrock>`
  flag is required in that mode alongside `edition` and stays refused
  on the edition-neutral `logic` / `netlist` stages (exit 2). Not in
  scope for this PR: Steiner routing / wire-length determination,
  delay insertion (repeater buffers), crossing legalization,
  edition legalization, block-array voxel lowering, the physical-tile
  (tier 3) cell library, the tick simulator, `assert truth|always|
  latency` evaluation, sequential macros (`latch` / `pulse` / `delay`
  / `edge_*` / `counter`), and QC/BUD refusal
  (`E_NO_PORTABLE_IMPL`) — each remains a follow-up that will build
  on the Placement IR shape this PR pins.
- Redstone Edition Netlist IR + `cairn synth --stage edition
  --edition <java|bedrock>` (M6-PR3) — the third slice of the M6
  redstone-simulates pipeline. `cairn-lang-redstone` grows a
  `compile_edition_netlist(&ScopedNetlistIr, Edition)` entry point that
  walks the Netlist IR produced by M6-PR2 and picks the target-edition
  realisation of each `LogicalCell` — the middle tier of the three-tier
  cell library documented in `spec/redstone` §14.6 (`Logical Cell →
  Edition Cell → Physical Tile`). The pass is a pure structural lookup:
  drivers, `NetRef`s, inputs, outputs, and `signal_defs` are copied
  verbatim from the source Netlist IR, and the topological invariant
  (`NetRef::Cell(j)` in `cells[i]` still satisfies `j < i`) carries
  through by construction. `EditionCell` names both the target edition
  and the physical implementation family so a bug that pairs a Java AND
  cell with a Bedrock torch tile is a type error, not a runtime mishap
  — `and` maps to Java `ComparatorAnd` / Bedrock `TorchAnd`, `or` maps
  to Java `RepeaterOr` / Bedrock `TorchOr`, and `not` maps to Java /
  Bedrock `InverterTorch` (structurally shared but edition-tagged so a
  later placer can pick the correct tile orientation, one of the
  edition-absorbed differences per §14.6). The parser-unreachable
  cells (`xor` / `nand` / `nor` / `mux`) each land as a per-edition
  `*Unpinned` placeholder variant (`JavaXorUnpinned`,
  `BedrockXorUnpinned`, ...) rather than one edition-agnostic
  catch-all, so container / cell edition parity is enforced by naming
  and the eventual parser change renames the placeholder in the one
  match arm that produces it. The `(Edition, LogicalCell)` match is
  fully exhaustive with no wildcard, so adding a third `Edition`
  variant (Education) triggers a compile error at every mapping site
  instead of a silent Java fallthrough. Per `spec/redstone` §14.4 /
  §14.8 the Edition Netlist IR still carries no delay: repeater
  insertion is a Placement IR concern. The pass emits no diagnostics
  — CSE, cycle detection, and unbound-signal reporting ran in M6-PR1
  and Logical Cell selection ran in M6-PR2, so this stage is a pure
  structural rewrite. The CLI's `cairn synth --stage` gains an
  `edition` value alongside `logic` / `netlist`; the `--edition
  <java|bedrock>` flag is required in that mode and refused on
  `logic` / `netlist` (exit 2) rather than silently ignored, so the
  stage-vs-edition axes cannot drift out of sync in a caller's head.
  Not in scope for this PR: place-and-route, repeater insertion,
  tick simulator, `assert truth|always|latency` evaluation,
  sequential macros (`latch` / `pulse` / `delay` / `edge_*` /
  `counter`), `circuit region=... void=N` congestion detection
  (`E_ROUTE_CONGESTION`), and QC/BUD refusal (`E_NO_PORTABLE_IMPL`) —
  each remains a follow-up that will build on the Edition Netlist IR
  shape this PR pins.
- Redstone combinational Netlist IR + `cairn synth --stage netlist`
  (M6-PR2) — the second slice of the M6 redstone-simulates pipeline.
  `cairn-lang-redstone` grows a `compile_netlist(&ScopedLogicIr)` entry
  point that walks the Logic IR produced by M6-PR1 and rewrites every
  `GateNode` into a `CellNode` tagged with a `LogicalCell` (`and` / `or`
  / `not` reachable today; `xor` / `nand` / `nor` / `mux` reserved on
  the enum matching the Logic IR side). Cells carry canonical port
  drivers (`[A, B]` for two-input gates, `[A]` for `Not`, `[Sel, A, B]`
  for `Mux`) so a downstream simulator or placer can index by position
  without inspecting `PortName`. `NetRef` mirrors the Logic IR's arena
  `SignalRef` split so the topological invariant (every `NetRef::Cell(j)`
  in `cells[i]` satisfies `j < i`) carries through the rewrite as a
  single forward walk. Per `spec/redstone` §14.6, only the top of the
  three-tier cell library (`Logical Cell → Edition Cell → Physical
  Tile`) is chosen here — Java `ComparatorAND` vs Bedrock `TorchAND`
  edition selection stays for a later pass so the IR remains
  edition-neutral. Per §14.4 / §14.8 the Netlist IR still carries no
  delay: repeaters are inserted only at the Placement IR stage. The
  netlist pass emits no diagnostics of its own — CSE, cycle detection,
  and unbound-signal reporting have already run in M6-PR1, so this
  stage is a pure structural rewrite. The CLI's `cairn synth` gains a
  `--stage <logic|netlist>` flag (defaults to `logic` for backwards
  compatibility) still gated behind `--experimental-logic-synth`;
  future placement / route / simulator stages will keep landing on the
  same flag rather than sprouting new subcommands. Not in scope for
  this PR: Edition Cell selection, place-and-route, tick simulator,
  `assert truth|always|latency` evaluation, sequential macros
  (`latch` / `pulse` / `delay` / `edge_*` / `counter`),
  `circuit region=... void=N` congestion detection
  (`E_ROUTE_CONGESTION`), and QC/BUD refusal (`E_NO_PORTABLE_IMPL`) —
  each remains a follow-up PR that will build on the Netlist IR shape
  this PR pins.
- Redstone combinational Logic IR + `cairn synth` (M6-PR1) — the first
  slice of the M6 redstone-simulates pipeline. `cairn-lang-redstone` grows
  a `synthesize(&IntentModule)` entry point that walks every
  struct / def / site body, collects sensor bindings (`pressure_plate ...
  -> sig.X`, and any future sensor whose surface `-> sig.Y` tail parses
  to a `DotRef`) into `InputPort`s, collects actuator arguments
  (`opened_by=` / `powered_by=` / `lit_by=` / `fired_by=`, per
  `spec/redstone` §14.2) into `OutputPort`s, and lowers each
  `logic sig.X = <expr>` line into a topologically ordered DAG of
  `GateNode`s. Combinational primitives cover `and` / `or` / `not`
  (reachable from the current AST directly); `xor` / `nand` / `nor` /
  `mux` sit on the `GateKind` enum ready for a follow-up parser PR that
  teaches the surface call-expression syntax. Common subexpression
  elimination collapses `sig.a or sig.b` written on two `logic` lines
  to one shared OR gate so downstream placement pays no fanout tax
  the source never asked for. Four new diagnostic codes fire fail-loud
  per `spec/lint` §11's self-correction triple:
  `E_LOGIC_UNBOUND_SIGNAL` when a reference names no sensor / earlier
  binding (with a `Valid signals in scope: ...` footer listing the
  reachable alternatives), `E_LOGIC_MULTIPLE_DRIVERS` when two `logic`
  lines share an LHS or a `logic` LHS collides with a sensor,
  `E_LOGIC_CYCLE` when a combinational dependency chain closes on
  itself, and `W_LOGIC_UNUSED_SIGNAL` on a bare-ref or gate-producing
  binding whose LHS no actuator or downstream logic consumes. Cascade
  suppression tracks the failed-LHS set so a single unbound signal at
  the root fires exactly one diagnostic, not once per consumer. The
  CLI ships a matching internal-tier `cairn synth <file>
  --experimental-logic-synth` subcommand that dumps the per-scope
  Logic IR as JSON — the gate is mandatory until the pipeline reaches
  a stable compatibility tier (netlist, placement, route, simulator
  are still to come). Not in scope for this PR: the Netlist IR, cell
  library, place-and-route, tick simulator, `assert truth|always`
  evaluation, and sequential macros (`latch` / `pulse` / `delay` /
  `edge_*` / `counter`) — each is a follow-up PR that will build on
  the Logic IR shape this PR pins.
- Cairn VS Code extension and `cairn-lsp` binary distribution (M5-PR3) —
  closes the M5 developer-experience milestone. A new `editors/vscode/`
  TypeScript extension (published as `.vsix`, not to the Marketplace in
  this PR) activates on `onLanguage:cairn` / `workspaceContains:**/*.crn`,
  resolves `cairn-lsp` from the `cairn.serverPath` setting or the OS
  `PATH` (falling back to a single actionable notification linking to the
  release page rather than silently no-op-ing), spawns it over stdio with
  `vscode-languageclient@9`, and logs the server's `--version` string at
  activation so bug reports carry a version tag without extra ceremony.
  A minimal TextMate grammar (`source.cairn`) colours comments (`#`),
  directives (`@cairn`/`@requires`/`@intended_targets`), top-level
  keywords (`theme`/`def`/`site`/`struct`) and member keywords (mirrors
  `cairn-lang-core::intent::known_keywords` — `floor`/`walls`/`door`/
  `window`/`roof`/`stair`/`level`/`pressure_plate`/`circuit`/`place`/
  `connect`), material tokens (`@name.dotted`), attribute keys (`k=`),
  the `->` slot-binding arrow, and quoted strings; syntax lives next to
  the LSP-driven diagnostics/completion the two previous PRs already
  ship. `cairn-lsp` gains a small `--version` (and `-h`/`--help`) flag —
  aligned with `cairn --version` and covered by a new integration test
  in `crates/cairn-lang-lsp/tests/version_flag.rs` — so the extension
  and support triage can identify the server without opening it.
  `.github/workflows/publish.yml` now cross-compiles `cairn-lsp`
  alongside `cairn` for all six release targets and stages both binaries
  into one archive per target; the existing sigstore signature covers
  the pair so the asset count, `.sha256`, and `.sigstore` layout are
  unchanged. Not in scope: Marketplace / Open VSX publishing, bundling
  the binary inside the `.vsix`, and a semantic-tokens provider — all
  three are deferred to M6 or later PRs.
- `cairn-lsp` completion (M5-PR2) — `textDocument/completion` over the
  language's closed vocabularies, advertised at `initialize` with trigger
  characters `@`, `=`, and `.`. Four cursor contexts are recognised:
  line-opening keywords (top-level `theme`/`def`/`site`/`struct`, member
  commands inside `struct`/`def`/`site` bodies, and `slot` + selector
  keywords inside `theme` bodies), `mat_slot=` values (the union of slot
  names declared by the document's themes, so `_java`/`_bedrock` variant
  themes union naturally — matching how unpinned `cairn check` treats
  slot presence), and `@` material tokens fed from the built-in registry
  union (java ∪ bedrock): every abstract token with its resolved
  canonical id as the item detail, plus the deduplicated canonical ids
  from the catalog's value column (the full canonical vocabulary waits
  on a registry blocks table that does not exist yet). Context detection
  is a line-local text heuristic — Cairn is strictly line-oriented, so
  the line prefix is grammatically sufficient — which keeps completion
  working while the document fails to parse, the normal state
  mid-keystroke; a drift-guard test pins the `slot NAME -> TARGET` scan
  to the parser's view of every shipped example. Items replace the
  partial token under the cursor via `TextEdit` (UTF-16-correct ranges)
  and carry `sortText` freezing the curated declaration/catalog order;
  prefix filtering is left to the client, and positions without a closed
  set (comments, free-form values, header directives) return no items
  rather than inventing a vocabulary (principles P3). The server now
  keeps a `DocumentStore` (URI → last synced text) so requests can read
  documents outside a change notification; asking about a never-opened
  document, or a position further than one line past the document's
  end, is refused loud with `InvalidParams` (one line past still
  answers — a `didChange` can race the request). `cairn-lang-lsp`
  gains a `cairn-lang-formats` dependency for the registry packs.
- `cairn-lsp` (M5-PR1) — the first working cut of the language server:
  a `[[bin]]` target of `cairn-lang-lsp` speaking standard LSP over
  stdio. It advertises full-content document sync at `initialize`,
  runs the same `parse → lower → check` pipeline as `cairn check`
  (edition unpinned, so slot presence checks union the per-edition
  theme variants) on every `didOpen`/`didChange`, and pushes
  `textDocument/publishDiagnostics`; `didClose` publishes an empty set
  so no stale squiggles survive. Check findings keep the stable
  `E_*`/`W_*` string in the LSP `code` field with `source: "cairn"`,
  span-carrying notes surface as `relatedInformation`, spanless notes
  (the "valid candidates" / "Suggested fix:" footers) fold into the
  message as `note:` lines so the self-correction triple reaches the
  editor verbatim, and structured `data` payloads pass through for
  future quick-fixes. A parse/lex failure pre-empts the check passes
  and yields exactly one error diagnostic spanning to the end of the
  offending line. Positions are converted from core's byte spans to
  the protocol's 0-based line / UTF-16 code-unit coordinates by a new
  `line_index::LineIndex`, keeping UTF-16 knowledge out of
  `cairn-lang-core`. Transport is `lsp-server` + `lsp-types`
  (rust-analyzer's synchronous stdio scaffold — no async runtime
  enters the workspace). Completion followed as M5-PR2 (above); the
  VS Code extension is the remaining M5 piece (M5-PR3), and binary
  distribution in the publish pipeline lands with the extension.
- `cairn-lang-formats::portability` — palette-entry portability counters
  backing the `edition_portability` axis of `cairn info` (spec
  versioning-editions §10.5). `portability_for_bedrock` runs every
  non-air palette entry through `bedrock_state::translate_states` and
  folds the outcome into `{portable, degraded, unsupported}`: a
  lossless translation counts portable, a translation carrying a
  degradation note (`shape != straight` on stairs today) counts
  degraded, and a `BedrockStateError` counts unsupported.
  `portability_for_java` reports every non-air entry as portable
  (Java is the base per §10.3). The counting granularity is
  per-palette-entry so the figures track what the `.mcstructure`
  writer actually emits — a member whose lowering interns several
  distinct palette entries contributes one row per entry.
- `cairn-lang-core::Edition` — cross-cutting edition marker (`Java` /
  `Bedrock`) shared by the resolver and the CLI so a future third
  edition adds one variant in one place. `FromStr` gates unknown edition
  strings loud (`unknown edition `{input}`. Valid: java, bedrock. Fix:
  ...`), and `cairn info --editions foo` now exits 2 before running the
  dry-run lowering rather than silently forwarding an unrecognised
  edition to a zero-fill portability row.
- `cairn-lang-core::resolve` — per-edition theme fallback (spec
  versioning-editions §10.7 hierarchy #2). A theme whose name ends in
  `_java` / `_bedrock` declares an edition variant of a logical theme
  (`theme shop_java:` and `theme shop_bedrock:` share the logical name
  `shop`). `resolve` now takes an `edition: Option<Edition>` argument
  and auto-picks the matching variant per struct/def scope, falling
  back to an unsuffixed theme of the same logical name when the
  requested variant is absent. Unsuffixed themes (the `theme medieval:`
  shape used by every existing example) resolve unchanged under both
  editions. Under `resolve(ir, None)` — the `cairn check` path where
  no edition has been picked — the resolver unions slot names across
  variants of one logical theme so `mat_slot=NAME` presence checks do
  not spuriously fire on slots that only one variant declares. Selector
  matching in the `None` case is scoped per-picked variant to preserve
  the per-theme DI contract from §7. `resolve(&ir)` callers migrate to
  `resolve(&ir, edition)`; `check(&module, &ir)` migrates to
  `check(&module, &ir, edition)`.
- `cairn info --editions java,bedrock` now populates the `degraded` /
  `unsupported` columns from a per-edition dry-run lowering (one
  `lower_to_block_array` per requested edition, materials resolved
  against the matching built-in pack, palette fed into
  `portability_for_*`) instead of the hard-coded zeros. On
  `themed-tower.crn` the eave's `shape=outer_left` stair now surfaces
  as `Bedrock: degraded: >=1`; `cottage.crn` stays at zero across both
  axes. The `EditionPortability` JSON / text shape is unchanged so
  `--format json` consumers see real values without a wire break;
  `compute_axes` in `cairn-lang-core::resolve` gained a
  `Vec<EditionPortability>` argument that carries the per-edition
  figures from the caller (the CLI, since `core` does not depend on
  `formats`).
- `cairn check --edition java|bedrock` — optional edition pin so a
  `mat_slot=X` reference to a slot only the *other* variant declares
  fires `E_UNRESOLVED_SLOT`. When `--edition` is omitted, the resolver
  unions slot names across both variants of one logical theme so the
  file passes `check` regardless of which edition it later compiles
  for.
- `examples/edition-fallback.crn` (+ `.crn.lock`) — a `shop` logical
  theme with `shop_java` binding the `floating_text` slot to
  `@sign.oak` and `shop_bedrock` binding it to `@sign.oak_wall`,
  demonstrating spec §10.7 hierarchy #2 end-to-end without introducing
  the entity concept the spec's illustrative `text_display` example
  would require. The Java compile writes `oak_sign` into the palette;
  the Bedrock compile writes `oak_wall_sign`. New material tokens
  `sign.oak` / `sign.oak_wall` land in both built-in packs.
- `cairn-lang-formats::bedrock_state` — per-edition blockstate translation
  for the Bedrock backend, the follow-up the `.mcstructure` writer deferred.
  `translate_states` maps the **stair family** (the only block kind the
  lowering interns with properties today) from Java's `facing` / `half`
  string properties to Bedrock's typed `states` — `weirdo_direction`
  (`east=0, west=1, south=2, north=3`, verified against the wiki
  `Stairs/BS` listing) and `upside_down_bit` (`top=1, bottom=0`). Stair
  `shape` has no Bedrock state: `straight` (the Bedrock default) drops
  losslessly, while a corner shape drops with a `ParityNote` the CLI
  surfaces as `warning[W_INTENT_DEGRADED]` (spec versioning-editions §10.3
  `dropped_states: [shape]` / §10.7, never a silent drop per §10.4). A
  block with properties outside a mapped family, or a stair state value
  outside the Java domain, still fails loud with the self-correction triple.
  `build_mcstructure_tag` now returns `(Compound, Vec<ParityNote>)` and
  writes real `states` per palette entry instead of the old empty compound;
  `cottage.crn` (all-`straight` gable roof) compiles clean for
  `--edition bedrock`, and `themed-tower.crn` compiles with one
  `W_INTENT_DEGRADED` for its non-straight eave corners. The
  `BedrockStructureError::StatefulPaletteEntry` hard error is replaced by
  a transparent `BedrockStructureError::State(BedrockStateError)`.
- `cairn-lang-nbt::bedrock::write_bedrock_uncompressed` — a little-endian
  NBT writer for Bedrock's uncompressed `.mcstructure` on-disk form. The
  byte-level encoder is refactored into a single endian-parameterised core
  (`writer.rs`) shared with the Java writer, so the two dialects differ
  only in scalar byte order and can never drift apart on validation rules
  (`InvalidString` / `HeterogeneousList` / `LengthOverflow`). The Java
  public API (`write_java_uncompressed` / `write_java_gzip`) and its error
  type are unchanged.
- `cairn-lang-formats::bedrock_structure` — a `.mcstructure` serialiser
  mirroring `java_structure`. `build_mcstructure_tag` lowers a
  `BlockArray` into the Bedrock root shape (`format_version`, `size`,
  `structure.block_indices` two-layer Z-fastest arrays with a `-1`-filled
  waterlog layer, `structure.palette.default.block_palette` of
  `{ name, states, version }`, `structure_world_origin`), and
  `write_mcstructure` writes it uncompressed. This first cut emits
  **stateless palettes only**: a palette entry carrying blockstate
  properties fails loud with `BedrockStructureError::StatefulPaletteEntry`
  (spec versioning-editions §10.4 forbids silent substitution/dropping),
  its message carrying the self-correction triple. Per-edition state
  mapping (`facing` / `half` / `shape`) lands in a follow-up.
- `cairn-lang-formats` builtin Bedrock registry pack
  (`registry-data/bedrock/`) plus `builtin_bedrock` / `load_builtin_bedrock`
  and `data_version::{BedrockTarget, resolve_bedrock_target}`. The pack's
  `data_versions` column carries the `.mcstructure` block-palette `version`
  integer (`(major << 24) | (minor << 16) | (patch << 8) | revision`); the
  materials catalog covers the same abstract tokens the Java pack lifts.
  Target resolution reuses the Java pack's machinery (`latest` alias,
  Damerau-Levenshtein suggestion), and `UnsupportedTarget` now names the
  edition whose version table was consulted.
- `cairn compile --edition bedrock` writes `.mcstructure` artifacts and a
  lockfile whose `target.edition = bedrock`, `data_version = block_version`,
  and `registry_pack_hash` pins the Bedrock pack bytes. The Java `.nbt`
  path is byte-for-byte unchanged. A `ResolvedTarget` enum threads the
  edition through artifact naming (`OutputExt`), tag building, the writer
  (gzip vs uncompressed), and the lockfile so a future edition slots in at
  one site.

- `cairn-lang-core::block_array::lower` — `level y=N` blocks now
  participate in phase-bucketed voxelisation. A new `flatten_members`
  pre-pass expands each `level` into `(y_offset, child)` pairs so a
  `walls` / `door` / `window` / `stair` nested inside a `level` reaches
  the massing / openings / envelope phases with its authored `y` shifted
  up by the level's `y=`. `max_wall_height` becomes `max_wall_top` and
  now aggregates over the flattened list so a `level y=N walls id=X
  height=H` correctly extends the struct's roof plane to `y = N + H`.
  Nested `level` blocks defer with `W_DEFERRED_MEMBER` (depth 1 only).
- `cairn-lang-core::block_array::lower` — `MemberRole::Stair` gains a
  minimal `fill_stair` implementation targeting the eave pattern
  `themed-tower.crn` uses: `kind=stairs`, `side=front|back|left|right`,
  `half=top|bottom`, `facing=out|in`, `shape=straight|outer_left|outer_right`,
  and an optional `y=` local offset. The stair band paints along the
  wall's overhang row (one voxel outside the wall) at
  `y = y_offset + local_y`, taking its base id from the resolved
  `mat_slot=` state and falling back to `spruce_stairs` otherwise. Any
  other `kind=` / `half=` / `facing=` / `shape=` still defers with a
  targeted `W_DEFERRED_MEMBER`.
- `cairn-lang-core::block_array::lower` — `fill_window` supports the
  `repeat=N step=M` arrow-slit pattern themed-tower's second-floor
  windows use. The rectangle is stamped `N` times along the wall,
  advancing by `step` voxels between stamps; `repeat` collapses to 1 when
  absent, and `repeat>=2 step=0` defers so instances cannot overlap.
  Windows without a `mat_slot=` binding now carve air instead of dropping
  silently, so `class=arrow_slit` slits produce actual openings in the
  wall. Windows with an explicit `mat_slot=` are unaffected.
- `crates/cairn-lang-formats/tests/themed_tower_level_lower.rs` — new
  integration test that lowers `examples/themed-tower.crn` end-to-end
  through the built-in registry pack and pins dims, palette (five
  resolved ids including `dark_oak_stairs` and `dark_oak_planks`), the
  upper-wall ring, the eave stair band, the arrow-slit air carve
  pattern, and the "zero `W_DEFERRED_MEMBER`" contract. Lives in
  `cairn-lang-formats` because `cairn-lang-core` cannot depend on
  `cairn-lang-formats` for the materials resolver without a cycle.
- `cairn-lang-core::block_array::lower` — `MemberRole::PressurePlate`
  gains a minimal `fill_pressure_plate` implementation covering the
  fixture shape `redstone-door.crn` authored: an `at=<side>.outside` /
  `at=inside.<side>` compound anchor (two-segment `DotRef`), non-negative
  `offset=N` along the wall axis, non-negative `y=N` from the floor, and
  an optional `mat_slot=` that resolves to a bare block id (`oak_pressure_plate`
  fallback). `<side>.outside` on a struct without overhang falls back to
  the wall's foundation cell so the anchor still lowers cleanly. The
  `-> sig.<name>` binding on `Member.binding` is parsed but intentionally
  read-through until the redstone lowering pass lands. Any other `at=`
  shape or a resolved state with bracketed properties still fires
  `W_DEFERRED_MEMBER`.
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs`
  — new integration test that lowers `examples/redstone-door.crn` end-to-end
  through the built-in registry pack and pins gatehouse dims (7x4x5), the
  presence of `minecraft:oak_pressure_plate` in the palette, both plate
  voxels (front-wall corner at (0,0,4) for the `outside` anchor and one
  voxel inward at (0,0,3) for the `inside.front` anchor), and the
  "zero `W_DEFERRED_MEMBER` on `pressure_plate`" contract.
- `cairn-lang-core::block_array::lower` — `MemberRole::Circuit` gains a
  minimal `recognize_circuit_region` implementation covering the fixture
  shape `redstone-door.crn` authored: `region=<label>` (an `Ident` or
  `Str` naming the region a later logic pass will look up) and
  `void=<N>` (a `u32` service-layer height, `N >= 1`). No voxels are
  painted — spec/redstone.md §14.5 / §14.8 places dust / repeater /
  cell tiles on the future `logic_synth → logic_place → logic_route`
  passes — so the recogniser only guards the surface shape. `region=`
  absent, `region=` present but of a non-label kind (integer, boolean,
  size, token, reference, list), `region=""` empty, `void=` absent,
  `void=0`, and non-`u32` `void=` each fire `W_DEFERRED_MEMBER` with a
  targeted primary that names the missing / invalid key (the offending
  kind is included in the region-kind-mismatch primary).
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs`
  — new `redstone_door_circuit_line_emits_no_deferred_warning` test
  pins the "zero `W_DEFERRED_MEMBER` on `circuit`" contract on the
  `circuit region=floor void=2` line, mirroring the shape of the
  neighbouring pressure_plate zero-defer test.
- `cairn-lang-core::block_array::lower` — `MemberRole::Door` members
  whose surface line is the selector form (`door[id=X] opened_by=…`)
  are now recognised as **actuator patches** before phase-bucketing.
  A new `recognize_actuator_patch` guard peels these lines off the
  `openings` phase so `carve_door`'s `side_of` check no longer
  false-positives "missing `side=`" on a patch line. The recogniser
  validates the surface shape only (spec/redstone.md §14.2): the
  `[selector]` must carry an `id=<label>` naming a physical door
  declared in the same `flatten_members` view (level-nested doors
  are selectable), and `opened_by=` must resolve to a two-segment
  `sig.<name>` `DotRef`. Missing / non-label / unknown `id=`,
  missing `opened_by=`, or an `opened_by=` value that is not a
  `sig.<name>` reference each fire `W_DEFERRED_MEMBER` with a
  targeted primary; the unknown-id primary lists every physical
  door id declared in the scope so the author can spot near-misses.
  Only `door[id=…] opened_by=` is covered — `lit_by=` / `powered_by=`
  / `fired_by=` on lamps / pistons / dispensers land with their
  keywords in a future PR. Unknown selector attributes and unknown
  intent-state keys on a patch also defer (not silently accepted) so a
  future `powered_by=` cannot retroactively change the meaning of
  source that shipped meanwhile. `redstone-door.crn`'s
  `door[id=front] opened_by=sig.open` actuator-patch line now compiles
  clean; the last surviving `W_DEFERRED_MEMBER` on that example is
  gone.
- `cairn-lang-core::block_array::walkway` — new `route_path` ground-plane
  router for `connect` walkways. When the straight Manhattan L between
  two ports would cross a placement floor, `lower_connects` now searches
  for a detour instead of skipping the colliding cells: Dijkstra over
  `(cell, direction)` states with the lexicographic cost
  `(path length, turn count)`, so the strip takes the shortest route
  around the obstacle and, among equal-length routes, the one with the
  fewest turns. Ties are broken by a fixed expansion order and a
  monotonic queue sequence — never by hash iteration order — so the
  same source always lays the same strip and the lockfile stays
  reproducible. The search area is the bounding box of the blocked
  cells on the walk plane plus both endpoints, inflated by one cell,
  with a 4-million-cell cap that degrades pathological inputs to the
  skip-and-warn fallback. `village.crn`'s `home1.entry ↔ home3.entry`
  row — whose L used to cut a 7-cell hole through home1's floor —
  now detours around home1's east face and the example compiles with
  zero warnings. `route_path` returns `Result<_, RoutePathError>`
  (buried endpoint / unreachable target / area cap / coordinate
  overflow) so the caller can match the warning note to the actual
  cause, and takes a `BlockedIndex` — built once per lowering — so the
  per-plane bounding rectangle comes from a single scan of the blocked
  set instead of one full re-scan per `connect` row (a site with many
  colliding rows would otherwise multiply that scan into an effective
  DoS on user input).

### Changed

- `cairn-lang-core::block_array::lower` — `fill_roof` no longer emits a
  `W_DEFERRED_MEMBER` when a `mat_slot=` binding resolves to an id other
  than the roof kind's canonical hardcode. Instead, the resolved id lands
  in the palette verbatim for `gable`, `shed`, `hip`, and `flat` roofs,
  giving `themed-tower.crn`'s `slot roof -> @roof.dark_wood` its
  dark-oak stairs without a warning. A `mat_slot=` state whose
  `properties` are non-empty still fires a deferred warning (the
  geometry generator owns `facing` / `half` / `shape`).
- `crates/cairn-lang-cli/tests/cli_compile.rs` — `c14b`
  "`W_DEFERRED_MEMBER` still fires on themed-tower" is replaced by
  `c14e` "themed-tower compiles without deferred warnings", pinning the
  same shape as `c14` (cottage) and `c21` (village).
- `crates/cairn-lang-cli/tests/cli_lower.rs::lower_3_deferred_member_warnings_print_to_stderr`
  moves off the `pressure_plate` snippet (now clean) onto a bare
  `circuit region=floor void=2` snippet — that is the next role whose
  lowering has not yet been spec'd, so it carries the deferred-warning
  regression from here.
- `crates/cairn-lang-cli/tests/cli_compile.rs` — new `c14f` pins that
  `redstone-door.crn` compiles without a `pressure_plate` deferred
  warning while `circuit` is still surfaced, mirroring the same shape
  `c14e` uses for themed-tower.
- `crates/cairn-lang-cli/tests/cli_lower.rs::lower_3_deferred_member_warnings_print_to_stderr`
  moves off the `circuit` snippet (now recognised) onto a
  `stair kind=stairs side=front shape=inner_left` snippet — the stair
  path lowers `straight`, `outer_left`, and `outer_right` but still
  defers `inner_left` / `inner_right`, so an inner-corner stair carries
  the deferred-warning regression from here.
- `crates/cairn-lang-cli/tests/cli_compile.rs::c14f_redstone_door_pressure_plate_paints_without_deferring`
  drops the `circuit` / `pressure_plate` substring checks and pins the
  `warning[W_DEFERRED_MEMBER]` primary count against a baseline of one
  (the actuator patch on line 25's `door[id=front] opened_by=…`
  Member, which `carve_door` still surfaces as `missing side=`). A
  substring check would false-positive the catalogue note that follows
  each warning (the note lists every supported role by name) and
  false-negative a refactor that stops naming the role in the primary
  text; the baseline pin catches both.
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs::redstone_door_circuit_line_emits_no_deferred_warning`
  applies the same baseline-pin refactor: it now asserts exactly one
  `DeferredMember` diagnostic (the actuator patch) rather than
  filtering primaries for `"circuit"` — the void-overflow path routes
  through `nonneg_int_or_defer` whose primary never mentions
  `"circuit"`, so a substring filter would silently pass regressions
  on that arm.
- `crates/cairn-lang-cli/tests/cli_compile.rs::c14f_redstone_door_pressure_plate_paints_without_deferring`
  is renamed to `c14f_redstone_door_compiles_without_deferred_warnings`
  and drops the baseline of one in favour of pinning
  `stderr.matches("W_DEFERRED_MEMBER").count() == 0`, matching the
  shape `c14` (cottage) and `c14e` (themed-tower) already use. The
  `gatehouse.nbt` existence assertion is retained so a regression
  that turns lowering silent still fails loud on the missing artefact.
- `crates/cairn-lang-formats/tests/redstone_door_pressure_plate_lower.rs::redstone_door_circuit_line_emits_no_deferred_warning`
  is renamed to `redstone_door_lowers_without_deferred_warnings` and
  drops the "exactly one deferred (the actuator patch)" baseline in
  favour of "zero deferred" — the actuator patch is now recognised
  alongside the plate paint and the circuit region marker, so the
  whole example lowers clean.
- `W_WALKWAY_BLOCKED` now only fires when the detour search finds **no**
  unobstructed route between the two ports (a port buried under another
  placement's floor, a fully enclosed target, or the area cap); the row
  then falls back to the straight L with the colliding cells skipped,
  exactly as before, so the `data: { kind: "walkway_blocked",
  skipped: N }` payload and the "skipped N cells" primary text are
  unchanged. The note now names the concrete cause — which port is
  buried, an enclosed target, the search-area cap (with both numbers),
  or coordinate overflow — each with its own remedy, instead of one
  catch-all gap-widening suggestion that cannot fix three of the four.
- `crates/cairn-lang-core/src/block_array/lower.rs` — the
  `walkway_blocked_cells_skip_with_w_walkway_blocked_count` fixture
  gains a third placement whose floor buries the `from` port (the old
  two-place fixture now routes around `b` cleanly and moved to the new
  `walkway_routes_around_obstructed_l_path_without_warning` /
  `walkway_detour_is_deterministic_across_lowerings` tests).
- `crates/cairn-lang-core/tests/village_lower.rs` — the home1↔home3
  walkway pins move from the straight strip (`footprint 1×15`) to the
  detour around home1's east face (`footprint 6×15`, still anchored at
  home3's front port), and a new
  `village_emits_zero_walkway_blocked_warnings` test pins the
  "village compiles warning-free, 25 unbroken gravel cells" contract.










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

The first publicly-numbered release will be **`2026.7.0`** (planned). Until then this section
records what has been built into the repository in preparation for that release. No `cairn-lang-*`
crate has been published to crates.io yet; the workspace version stays at the `0.0.0` placeholder
on `canary`, and `cargo publish` only fires when the monthly-minor release PR — the one that
carries the real CalVer version — is merged.

### Changed

- **BREAKING (lockfile schema):** `LockWalkway.from` and `LockWalkway.to`
  in `build.cairn.lock` are now `{ place, port }` objects rather than
  `"PLACE.PORT"` joined strings. The wire format for a single endpoint
  becomes
  ```yaml
  - site: hamlet
    from:
      place: home1
      port: entry
    to:
      place: home2
      port: entry
  ```
  No on-disk lockfiles exist in the wild yet (the lockfile section
  landed alongside walkway lowering in the same `[Unreleased]` window),
  so no compatibility shim is provided.
- `cairn-lang-core::ids` — new `PlaceId` / `PortId` / `SiteName` /
  `WalkwayEndpoint` / `WalkwayScopeKey` newtypes the resolver
  (`PortRef`, `ValidatedConnect`), block-array IR (`Walkway`,
  `Placement`, `BlockArrayIr.walkways` key), and lockfile DTOs
  (`LockPlacement`, `LockWalkway`) all share. Each identifier newtype
  rejects `.`, `:`, and whitespace at construction so a port id
  containing `.` (which would otherwise make
  `walkway::SITE::a.b.c__...` silently ambiguous) is caught at the
  type boundary rather than re-parsed later. Wire format for
  identifier scalars is unchanged thanks to `#[serde(transparent)]`.
- `cairn-lang-core::resolve::ResolvedConnect` was renamed to
  `ValidatedConnect`. `path` is still a `ValueWithSpan` — per-edition
  lifting to a `BlockState` stays in the lowering layer because the
  registry pack resolver lives in `cairn-lang-formats`, downstream of
  `resolve`.
- `cairn-lang-core::block_array::Walkway` replaces `dims: Dims` with
  `footprint: Footprint { x, z }`. Walkways are always one block
  thick, so the `y = 1` invariant is now visible in the type;
  `Footprint::to_dims_y1` re-attaches the implicit `y` at the single
  CLI site that emits a lockfile entry.
- `cairn-lang-core::block_array::build_walkway_array` returns a named
  `WalkwayLayout { array, origin, blocked_count }` instead of a bare
  `(BlockArray, (i32, i32, i32), usize)` tuple, so callers cannot
  silently rebind the origin and the blocked count.

### Added

- `door at=` now accepts the named anchors `center`, `left`, and
  `right` in addition to the previously-supported `center`. The new
  `left` anchor pins the openings cut and any walkway port to the
  wall-local axis origin (`u = 0`); `right` pins them to the far
  corner (`u = wall_length - 1`). `center` behaviour is unchanged
  (`u = wall_length / 2`, round-down on even widths), so existing
  examples and lockfiles are unaffected. `super::walkway::door_anchor_offset`
  and `super::lower::carve_door` share the same vocabulary, so the
  walkway port and the carved opening always resolve to the same
  column. Numeric offsets (`at=N`) remain reserved for a future
  extension and continue to cascade through `W_DEFERRED_MEMBER` whose
  defer message now lists the three accepted anchors. New
  `examples/at-side-walkway.crn` plus
  `crates/cairn-lang-core/tests/at_side_walkway_lower.rs` pin both
  corner anchors at the integration boundary. See
  `spec/components-editing-sites.md` §9.3.5 and `spec/syntax.md` §5.4.
- `cairn-lang-core::block_array::walkway::port_world_position` — walkway
  port endpoints can now be declared on `window` members in addition to
  `door` members (door behaviour is unchanged). For a `window` the
  wall-local anchor is the rectangle's geometric centre
  (`offset + size.w / 2`), and the port stays pinned at the placement's
  ground row (`place_origin.1`) so the walkway's 1-voxel-thick
  flat-strip invariant (`from.y == to.y`) is preserved regardless of
  the window's authored `y=`. The window must fit both horizontally
  (`offset + size.w ≤ wall_length`) and vertically
  (`y + size.h ≤ walls.height`); a window that would not even be
  carved by the openings pass cannot anchor a walkway either, and the
  row drops with a `W_DEFERRED_MEMBER` whose notes list the
  door / window / reserved-role contracts in turn. A `sym=true` window
  contributes a single port at the primary `offset` side. Stair / roof
  ports remain reserved for a future extension. See
  `spec/components-editing-sites.md` §9.3.5. The function's `port_id`
  argument is now `&PortId` instead of `&str`, closing the last
  `String`-primitive hole from the #34 newtype migration.
- `cairn-lang-core::check::DiagnosticData` — new public enum that
  carries the machine-readable payload for a `Diagnostic`. The first
  variant (`WalkwayBlocked { skipped }`) ships alongside
  `W_WALKWAY_BLOCKED`, exposing the skip count as
  `data.skipped` in the `cairn check --format json` output so LSP
  quick-fixes and CI annotators no longer need to re-parse the
  `"skipped N cells"` substring out of the human-readable `primary`
  message. The `data` key is omitted entirely when a diagnostic
  carries no payload, keeping the JSON contract additive for
  existing consumers. `spec/lint.md` §11.2 documents the full JSON
  shape. `Diagnostic` itself also gains `#[non_exhaustive]` so
  future field additions are no longer breaking for external
  callers — in-crate sites continue to build the struct directly.
- `cairn-lang-core::block_array::lower` — endpoint-skip cascade for
  walkways. When a `connect` row points at a placement that did not
  lower (e.g. its def has no `size=`, or a theme reference failed
  upstream), `lower_connects` now emits a `W_DEFERRED_MEMBER` whose
  message names the offending side instead of dropping the strip
  silently. The remediation note points back at the original
  `W_DEF_NO_SIZE` / `W_DEFERRED_MEMBER` / `E_UNRESOLVED_PLACE_REF` so
  the chain is easy to follow. Walkway IR / lockfile output is
  unchanged for healthy inputs.
- New regression tests under `crates/cairn-lang-core` pin the walkway
  surface end-to-end: `W_WALKWAY_BLOCKED` skip-count contract,
  abstract-token walkway lift / deferred / unknown-token paths
  (`walkway_abstract_path_*`), endpoint-cascade warning, and
  symmetric `from`/`to` sad-path coverage for `E_UNRESOLVED_PORT` and
  `E_UNRESOLVED_PLACE_REF` with span-anchor assertions. Tests for
  `village.crn` additionally pin walkway `origin`/`dims` so an axis
  swap or off-by-one in the overhang shift fails loud at the
  per-walkway entry.
- `cairn-lang-core::block_array::walkway` — `connect a.PORT to b.PORT
  path=@MATERIAL` rows now lower into per-walkway `BlockArray`s under a
  new `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT` IR key, so
  `village.crn` round-trips end-to-end through `cairn compile --edition
  java` (one `.nbt` per placement plus one per `connect` row). The port
  model is "one block outside the door's `side=` wall, at the ground
  row": M3-PR4 only exposes ports on `door` members (window / stair /
  roof ports land in a later PR), `at=center` is the only supported
  wall-local offset, and `front` / `back` / `left` / `right` map to
  `+z` / `-z` / `-x` / `+x` per `spec/components-editing-sites.md`
  §9.3.1. Walkways follow a Manhattan L at the two ports' shared Y
  (x-axis leg first, then z-axis leg) — 3D path search and stair
  approaches are intentionally out of scope so the port surface lands
  in one piece. Cells that overlap an existing structure floor are
  skipped and the row earns one `W_WALKWAY_BLOCKED` warning per
  collision so the author can widen the placement gap. The
  `BlockArrayIr` gains a parallel `walkways: IndexMap<…, Walkway>` map
  pinning the world origin, dims, and canonical path material (lifted
  through the same `resolve_block_state` pipeline `mat_slot=` uses, so
  both concrete `@gravel` and registry-backed `@path.gravel` work). The
  lockfile gains a matching `walkways:` section under the existing
  `placements:` block.
- `cairn-lang-core::resolve` — site-scope resolution now produces one
  `ResolvedConnect` per validated `connect` row (`Resolution.connects`)
  carrying both `PortRef`s and the `path=` value as a `ValueWithSpan`.
  The pass emits `E_UNRESOLVED_PORT` (Error, with a nearest-match note)
  when the right-of-dot port id is not declared by the referenced def,
  `E_AMBIGUOUS_PORT` (Error) when the def exposes the same `id=` on
  more than one member, and `E_MISSING_PATH_MATERIAL` (Error) when the
  row omits `path=`. The left-of-dot place id reuses the existing
  `E_UNRESOLVED_PLACE_REF` so the unknown-place code family stays
  single-sourced. Failed connects are dropped from `connects` so the
  walkway voxeliser only ever sees rows it can lay safely.
- Two advisory diagnostic codes on the lowering side:
  `W_WALKWAY_BLOCKED` (Warning) when the L-shaped path crosses an
  existing structure floor; the colliding cells stay air and the rest
  of the strip still lays. `W_DUPLICATE_WALKWAY` (Warning) when the
  same `(from, to)` port pair has already been laid in this site; the
  duplicate row is dropped silently so re-laying the same gravel strip
  cannot double-write voxels. The duplicate guard sorts the two
  endpoints so `a.entry → b.entry` and `b.entry → a.entry` collapse to
  one walkway.
- `cairn-lang-formats::java_structure::output_filename` now recognises
  the `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT` IR key
  shape and writes it as `SITE_walkway_FROM_PLACE_FROM_PORT__TO_PLACE_TO_PORT.nbt`,
  flattening the `.` separator so the on-disk name stays a single
  identifier token across operating systems.

- `cairn-lang-core::block_array::lower` — site lowering closes the
  `village.crn` round-trip. `lower_to_block_array` now iterates
  `intent.sites` after the existing struct loop: each `place` resolves its
  `use=DEF` against the module's defs, applies the place-local `theme=` to
  the def's body (cross-scope theme resolution), and emits a per-place
  `BlockArray` under the new `site::SITE::PLACE_ID` key so the existing
  `prepare_artifacts` → `write_compound_gzip` path writes one `.nbt` per
  placement (`home1.nbt`, `home2.nbt`, `home3.nbt`). The topological
  coordinate solver turns `at=origin` / `east_of=ID gap=N` /
  `north_of=ID gap=N` into absolute `(x, y, z)` origins under the
  `front`-is-`+z` convention (`spec/components-editing-sites.md` §9.3.1):
  `east` advances along `+x` past the prior placement's full inflated
  `dims.x` plus gap, `north` retreats along `-z` by `dims.z` plus gap. The
  resolved per-place origin lands in `BlockArrayIr.placements: IndexMap<…,
  Placement>` and in the lockfile under a new top-level `placements`
  section so a downstream consumer can rebuild the village layout without
  re-running the solver. (`connect` rows resolve and voxelise in the
  M3-PR4 walkway entry above.)
- `cairn lower` and `cairn compile` now surface resolver-emitted
  diagnostics (`E_UNRESOLVED_PLACE_REF`, `E_UNRESOLVED_THEME_REF`,
  `E_DUPLICATE_PLACE_ID`, `E_INVALID_PLACE_ORIGIN`, `W_UNUSED_DEF`,
  `E_UNRESOLVED_SLOT`, ...) on stderr alongside the lowering deferrals.
  Resolver `Error`-severity findings now fail the compile exit code, so a
  `place use=cottag` typo no longer produces zero `.nbt` files at exit 0.
- Six new diagnostic codes covering the site surface:
  `E_UNRESOLVED_PLACE_REF` (Error) on a `place use=X` whose `X` is not a
  declared def, on `east_of=Y` / `north_of=Y` whose `Y` is not a prior
  place id in the same site, with a nearest-match note via the existing
  `suggest::nearest_match`; `E_UNRESOLVED_THEME_REF` (Error) on
  `place theme=X` whose `X` is not declared, also with a nearest-match
  note; `E_DUPLICATE_PLACE_ID` (Error) on two `place` rows sharing an
  `id=` inside one site, with a span pointer back to the first
  declaration; `E_INVALID_PLACE_ORIGIN` (Error) on a `place` line that
  carries no origin selector, more than one of `at` / `east_of` /
  `north_of`, or an `at=` value other than `origin`; `W_UNUSED_DEF`
  (Warning) on a `def` that no `place use=NAME` ever references, so a
  typo on the `use=` side does not silently produce an empty build;
  `W_DEF_NO_SIZE` (Warning) on a `def` referenced by a `place` without
  a `size=WxH` header (the placement is skipped because the voxel
  footprint is underivable). Origin checks `return false` so a placement
  with a structural mistake is skipped entirely rather than landing a
  `.nbt` at exit non-zero. Spec §9.3.2 / §9.3.3 enumerate the rules these
  codes guard.
- `cairn-lang-core::lock::LockPlacement` and
  `Lockfile.placements: Vec<LockPlacement>` — per-`place` site coordinates
  resolved from the topological constraint chain land in the lockfile
  alongside `member_version_sensitivity`. Each entry pins `site`, `id`,
  `def`, `theme`, `origin: [i32; 3]` (negative `z` for `north_of`
  placements), and `dims: [u32; 3]`. The field is
  `skip_serializing_if = "Vec::is_empty"` so cottage / themed-tower locks
  remain byte-identical to pre-PR3 builds, and the existing
  `hash_resolved_ir` automatically picks up the new IR field via
  serde-json's structural walk. Spec §9.3.4 documents this as the
  re-resolution-free source of truth for site layouts (2027.1.0).
- `cairn-lang-formats::java_structure::output_filename` learns the
  `site::HAMLET::home1` → `home1.nbt` mapping alongside its existing
  `struct::cottage` → `cottage.nbt` rule. Per-place placements share an
  output directory with sibling structs; multi-site flat-namespace
  collisions are out of scope for M3 and the spec carves them out
  explicitly.
- `cairn-lang-formats::registry::materials` — abstract material catalog
  component of a Java registry pack. A flat list of
  `(token, block)` rows mapping every `@KIND.FAMILY.SPECIES` abstract
  token from `spec/materials-themes.md` §7.2 to a canonical Minecraft
  block id. The built-in catalog lives at
  `data/registry/java/materials.json` and is embedded via `include_str!`
  alongside `data_versions.json`; `pack.json::files.materials` references
  it as an `Option<String>` component, so a `--registry-pack <dir>`
  without a `materials` entry still loads (older packs ride on
  `MaterialsIndex::empty`). `MaterialsIndex::from_catalog` rejects a
  duplicate `token` with `RegistryError::Materials` /
  `MaterialsError::DuplicateMaterialEntry` at load time and ignores
  silent overwrites. Entries that name an explicit `namespace:` keep
  their override; bare ids inherit the catalog's top-level `namespace`
  (matching `BlockState` resolution for canonical tokens). The catalog
  bytes feed into `RegistryPack::bytes_hash` via `pack_hash`'s
  multi-component path, so the lockfile's `inputs.registry_pack_hash`
  shifts when a pack swaps its materials catalog.
- `cairn-lang-core::block_array::AbstractMaterialResolver` — trait the
  block-array lowering pass calls through to lift abstract material
  tokens (`@floor.wood.broadleaf`) into canonical [`BlockState`]s.
  `cairn-lang-formats::registry::MaterialsIndex` implements it, keeping
  `core → formats` free of a reverse import while letting the CLI hand
  the built-in pack into lowering. `MaterialDeferred` gains an
  `UnknownAbstract { token, suggestion }` variant for the
  pack-was-offered-but-the-token-is-missing path; `Abstract` survives
  for library callers (LSP highlight, `cairn check` without a pack)
  that intentionally do not pass a resolver. `lower_to_block_array`
  takes `materials: Option<&dyn AbstractMaterialResolver>` so the CLI
  surface can wire `builtin_java().materials` through without forcing
  every internal caller to construct one.
- `E_UNKNOWN_ABSTRACT_TOKEN` (Error) — fires when a `mat_slot=`
  resolves to an abstract token the registry pack's materials catalog
  does not declare. The diagnostic carries a `did you mean \`@X\`?`
  note populated by `nearest_match`'s Damerau-Levenshtein candidate
  (same edit cap and tie-break rules `2026.12-PR2` uses for `--target`
  versions and slot names), plus a static pointer to
  `spec/materials-themes.md` §7.2. `cairn lower` and `cairn compile`
  both exit `1` on any `Severity::Error` lowering diagnostic so the
  fail-loud expectation now applies to the lowering pass, not just to
  resolver/parse failures. `examples/themed-tower.crn` now lowers
  without any `W_ABSTRACT_TOKEN_DEFERRED` because the built-in catalog
  covers every token it binds (`floor.wood.broadleaf` →
  `oak_planks`, `wall.stone.cobble` → `cobblestone`, `wood.dark` →
  `dark_oak_planks`, `roof.dark_wood` → `dark_oak_stairs`); roof
  hardcoding still emits a `W_DEFERRED_MEMBER` against the gable
  generator and `level` blocks remain deferred, but the abstract
  resolution itself is now clean (2027.1.0).
- `cairn-lang-core::block_array::roof` — `shed`, `hip`, and `flat` roof
  voxelisers join the existing `gable` generator, closing the
  `spec/compilation.md` §4.3 carve-out that previously deferred
  "the broader roof taxonomy". `RoofKind::from_ident` parses
  `kind=gable|shed|hip|flat`; the `fill_roof` dispatcher in
  `block_array::lower` routes each kind through its dedicated generator
  and intern table. `shed kind=shed` requires a new `slope_to=front|
  back|left|right` argument (the high edge of the slope) and rises
  `slope_span` voxels above the wall top with stairs facing the high
  side; `hip` rises `ceil(short_span / 2)` voxels and emits an inset
  rectangle frame per layer with `shape=outer_left|outer_right`
  corners and a long-axis ridge row on rectangular footprints; `flat`
  is a single solid layer of `minecraft:spruce_planks` at
  `wall_top + 1` covering the full inflated bounding box. Every kind
  carries the existing overhang convention and the
  hardcoded-material → `mat_slot=` mismatch warning (sloped roofs emit
  `minecraft:spruce_stairs`, flat emits `minecraft:spruce_planks`;
  per-theme roof species follow with the registry pack). New
  `examples/roof-shed.crn`, `examples/roof-hip.crn`, and
  `examples/roof-flat.crn` fixtures pin the new kinds against the CLI
  (2027.1.0).
- `cairn-lang-core::suggest` — `nearest_match(input, candidates)` finds the
  closest entry in a closed vocabulary under Damerau-Levenshtein distance
  with a length-scaled cap (≤ 1 edit for 1–3 char inputs, ≤ 2 for 4–6, ≤ 3
  beyond), case-sensitive comparison (DSL identifiers are case-sensitive),
  first-in-iteration tie-break. Three diagnostic surfaces now lead their
  notes with `did you mean \`X\`?` when a candidate sits inside the cap,
  while keeping the existing closed-set listing as the fallback for typos
  too far from any candidate (the `expected one of: ...` line on
  `E_UNKNOWN_KEYWORD`, the slot-remediation line on `E_UNRESOLVED_SLOT`).
  `E_UNKNOWN_KEYWORD` pulls candidates from `known_keywords()`; the
  `mat_slot=` resolver pulls from the applied theme's declared slots only
  (proposing a slot from another theme would point the user at code that
  cannot bind across themes). `cairn-lang-formats::data_version`'s
  `UnsupportedTarget` grows a `suggestion: String` field carrying a
  pre-formatted `"did you mean \`1.21.4\`? "` prefix that the `thiserror`
  `Display` template interleaves into the wider error so the CLI's
  `cairn compile --target 1.21.5` exits with a targeted fix rather than
  the bare supported-list dump. Pool is every `mc_version` plus the
  `"latest"` alias because both are equally legitimate `--target` inputs.
  Closes the second half of `spec/glossary.md` "Fail-loud" — errors now
  return both the closed set of valid candidates *and* a suggested DSL
  fix when one is within reach (2026.12.0).
- `cairn-lang-formats::registry` — registry pack loader covering the
  manifest (`pack.json`) and the `(mc_version, DataVersion)` table
  (`data_versions.json`). The built-in Java pack lives under
  `data/registry/java/` and is embedded into the binary via
  `include_str!`; `load_from_dir` is the seam a later
  `--registry-pack <dir>` flag will use. Subsequent 2026.12.0 PRs extend
  `PackFiles` with `Option`-typed entries for blocks, items, tags, and the
  semantic-sensitivity catalog without breaking older packs. Validation at
  load time catches schema_version drift, empty version tables, a
  `latest` value that does not appear in `versions`, and an edition
  mismatch between manifest and loader. The pack's bytes hash
  (`sha256` over manifest + each named component) is exposed as
  `RegistryPack::bytes_hash` and lands in the lockfile under
  `inputs.registry_pack_hash`.
- `cairn compile examples/cottage.crn --edition java` now produces a
  complete cottage: floor, walls, gable roof with overhang, front door
  opening, and a symmetric pair of front windows. The block-array
  lowering pass implements `spec/compilation.md` §4.1 phase ordering
  (massing → envelope → openings) so a `door` written before `walls`
  still cuts a real opening, and inflates `Dims` by `2 * overhang` on
  the x/z axes while shifting floor/walls/openings inward so the
  authored `size=WxH` keeps its meaning. Gable roofs hard-code
  `minecraft:spruce_stairs` with `facing` derived from the slope side
  (`south` on `-z`, `north` on `+z`) and cap the ridge with a `half=top`
  stair on odd spans or a pair of opposing `half=top` stairs on even
  spans (so even-span apex rows do not leave an open V). Doors carve at
  most up to the wall top so a short-walled struct cannot punch through
  roof voxels, and refuse to carve at all without a `walls` member.
  `at=center` rounds half-up on even-width walls. `sym=true` windows
  emit a `W_DEFERRED_MEMBER` when the mirror would overlap the primary.
  Missing or mistyped `side=` on a door or window now produces an
  explicit diagnostic instead of dropping the member silently, and a
  `roof kind=gable` whose `mat_slot=` resolves to anything other than
  `minecraft:spruce_stairs` warns that the binding was not applied.
  The cottage example lowers without `W_DEFERRED_MEMBER` warnings;
  other roof kinds (`shed`, `hip`, `flat`) and door blockstate
  placement remain deferred for later PRs. Closes M2 cottage
  end-to-end milestone (2026.11.0).
- `cairn compile <file> --edition java [--target <mc_version>] [--out <dir>]
  [--lock <path>]` CLI subcommand closes M2 — it lowers a `.crn` source
  through the existing pipeline (`parse → lower → resolve →
  lower_to_block_array`) and writes one Java vanilla structure `.nbt`
  file per `struct` along with a `build.cairn.lock` next to the source.
  `--edition` is required by spec §4.2 (`--target` alone is forbidden);
  `--target` accepts the literal versions named in the M2 backend table
  plus the `latest` alias. `--edition bedrock` exits 1 with an explicit
  "not implemented" message so the surface is stable now and the
  Bedrock backend can grow into it. Lowering warnings
  (`W_DEFERRED_MEMBER`, `W_ABSTRACT_TOKEN_DEFERRED`) surface on stderr
  but do not affect the exit code, matching `cairn lower`.
- `cairn-lang-nbt` Java writer — owned tag tree
  (`Tag`/`Compound`/`List`) plus `write_java_uncompressed` and
  `write_java_gzip` entrypoints. Strings, numerics, and list element
  ids follow the Java big-endian wire format; the gzip variant uses
  `flate2`'s default compression level (matches Mojang's output, so
  byte-identical snapshots against samples extracted from the game
  remain possible). Bedrock little-endian and the streaming reader are
  follow-up work.
- `cairn-lang-formats::java_structure` — `BlockArray → Java vanilla
  structure NBT` lowering. Emits the `size` / `palette` / `blocks` /
  `entities` / `DataVersion` root keyed compound in the order
  `spec/architecture.md` §3.1 names. AIR cells are included in the
  `blocks` list (matches the Mojang structure block; keeps "void" vs
  "explicit air" distinguishable for M3 site placement). Abstract
  palette tokens that survive lowering raise
  `JavaStructureError::AbstractPaletteEntry` rather than silently
  emitting an air block.
- `cairn-lang-formats::data_version` — `(mc_version, DataVersion)`
  resolution. Initially covered 1.20.4, 1.21, and 1.21.4 plus the
  `latest` alias from a hardcoded array; the registry pack ingest above
  is now the source of truth, and `resolve_java_target` / `supported_list`
  delegate to the built-in pack via a `OnceLock`. The CLI surface is
  unchanged.
- `cairn_lang_core::lock` — `build.cairn.lock` reader/writer matching
  `spec/versioning-editions.md` §10.6. Keys appear in the spec-printed
  order (`source_hash, cairn_version, target, inputs,
  resolved_ir_hash, verified, member_version_sensitivity`).
  `hash_source` and `hash_resolved_ir` (sha256 over UTF-8 source bytes
  and over the IR's JSON serialisation, respectively) give the lockfile
  its reproducibility anchor. `inputs.registry_pack_hash` is now filled
  by the registry pack ingest above; `inputs.constraint_catalog_hash`
  stays zero until that catalog lands, and `LockInputs::zero()` remains
  available for fixtures and tests that need a known empty shape.
- `cairn info <file>` CLI subcommand reports the three version axes for a
  `.crn` source — registry-compatible range, per-edition portability, and
  semantic-sensitive members — as defined in `spec/versioning-editions.md`
  §10.5. `--editions java,bedrock` controls which editions appear (default
  `java,bedrock`); `--format text|json` switches between the human report
  and a `VersionAxes` JSON payload. M2-PR3 derives the registry range from
  `@requires version>=X` headers; portability and semantic-sensitivity
  catalog data land with the registry pack (2026.12.0).
- `cairn_lang_core::resolve` module — semantic layer over the Intent IR.
  Walks every `theme`, `def`, `struct`, and `site` to produce a
  `Resolution` that pairs each `mat_slot=NAME` with its theme's
  `slot NAME -> VALUE`, matches theme selectors against members, and
  classifies slot targets as canonical or abstract material tokens
  (`spec/materials-themes.md` §7.2). `cairn check` now runs `resolve()`
  as part of its pipeline so theme-binding hygiene shows up alongside
  syntactic findings.
- Three new diagnostic codes: `E_UNRESOLVED_SLOT` (Error; `mat_slot=`
  references a slot the applied theme does not declare),
  `E_UNKNOWN_SLOT_TARGET` (Warning; `slot X -> VALUE` where `VALUE` is
  neither a canonical nor an abstract token), and
  `E_THEME_SELECTOR_UNMATCHED` (Warning; selector binds to no member).
  `DiagnosticCode::severity()` now matches per variant rather than
  returning `Error` unconditionally.
- `cairn check` CLI subcommand and `cairn_lang_core::check` module collect
  syntactic validation findings without short-circuiting and emit them in
  gcc-style `file:line:col: error[CODE]: message` form (or pretty JSON via
  `--format json`, with `line` / `col` / `end_line` / `end_col` populated
  so downstream tooling consumes the same contract as the text format).
  Initial M2 codes: `E_DUPLICATE_SIZE`, `E_DUPLICATE_SLOT`,
  `E_DUPLICATE_ARG`, `E_DUPLICATE_ID`, `E_UNKNOWN_KEYWORD`,
  `E_TYPE_MISMATCH_LABEL`, `E_TYPE_MISMATCH_SIZE`. `E_DUPLICATE_ID` is scoped
  per immediate body, so `level y=0` blocks have their own namespace.
  `E_UNKNOWN_KEYWORD` covers both struct/def/site bodies (via
  `MemberRole::Other`) and the leading keyword of `theme` selector rules.
- `span: Span` on every AST node visible at parse time (`Header`, `Item`,
  `Statement`, `ThemeRule`, `Arg`, `Value`) and on the corresponding Intent
  IR types (`StructIr`, `DefIr`, `SiteIr`, `ThemeIr`, `Member`, `Size`,
  `LogicBinding`, `AssertIr`, `SelectorRule`). New `ValueWithSpan` wrapper
  carries values + their byte range through `IntentState` and IR argument
  maps. `Value` is now `{ kind: ValueKind, span }`; the wire shape is
  unchanged because the wrapper is `#[serde(transparent)]`.
- Core model: declare intent, the compiler resolves blockstate, coordinates, and physics.
- Three-layer IR (Intent → Semantic/Theme → block-array pivot), phase-ordered evaluation.
- Syntax: leading keyword + mandatory `key=value`; selectors; optional headers (`@cairn`,
  `@requires`, `@intended_targets`).
- Blockstate: derive-by-default with override-promotion; `intent_state` / `resolved_state`.
- Materials & themes: `mat_slot` slots, two-tier canonical vocabulary, CSS-like theme binding.
- Entities: first-class decoration entities plus a generic `spawn`; anchor conventions.
- Components, editing (stable addresses + patch grammar), and multi-building `site` placement.
- Versioning & editions: `(edition, version)` compile-time target; recompile-don't-transcode;
  fail-loud with nearest-valid suggestions; DataVersion as the canonical ordering key (absorbs
  Minecraft's move to date-based versions); provenance + lockfile.
- Java/Bedrock from one source via per-edition backends and a QC-free safe cell library.
- Redstone: logical sub-language (signal graph → synthesis → place-and-route), combinational plus
  curated sequential macros; verification by a headless tick simulator.
- Ecosystem interop: export to common formats; import as faithful transliteration with LLM lift.
- Evaluation: headless geometry/redstone simulator drives quantitative spec iteration.
- Documentation: per-crate READMEs, the
  [Developer Guide](https://cairn.kage1020.com/development/), the
  [Tutorial](https://cairn.kage1020.com/tutorial/), worked
  [examples](https://cairn.kage1020.com/examples/), and a cross-cutting
  [Glossary](https://cairn.kage1020.com/spec/glossary/).
- Japanese mirror of the user-facing documents (README, CONTRIBUTING, CHANGELOG, spec chapters,
  glossary, tutorial, examples index). English remains the source of truth.
- Documentation site under [`website/`](website/README.md) (Astro + Starlight, en + ja),
  deployed to Cloudflare Pages at <https://cairn.kage1020.com/>. The spec, tutorial, developer
  guide, and examples index are authored directly in
  [`website/src/content/docs/`](website/src/content/docs/); a placeholder playground page is
  wired to the future `cairn-lang-wasm` bindings; Cloudflare's Git integration auto-deploys on
  every push to `main`.
- Release strategy: monthly minor (`YYYY.M.0`) by GitHub Actions cron at 04:17 UTC on the 1st,
  plus on-demand patches (`YYYY.M.N`) triggered by qualifying commits on `canary`. The release
  PR (`release-plz-*` → `canary`) is merged after human review; release-plz publishes and the
  workflow fast-forwards `main` to `canary` so `main` mirrors only released state.
- Workspace versioning unified through `[workspace.package].version` and
  `[workspace.dependencies]`. Binaries are cross-compiled for Linux/macOS/Windows on
  `x86_64`/`aarch64`, signed with keyless sigstore, and attached to the GitHub Release.
- Crate prefix: `cairn-lang-*` (`cairn-lang-core`, `cairn-lang-cli`, `cairn-lang-nbt`,
  `cairn-lang-formats`, `cairn-lang-redstone`, `cairn-lang-lsp`, `cairn-lang-wasm`). The
  user-facing binary installed by `cargo install cairn-lang-cli` is still named `cairn`.
- Compatibility tiers documented in
  [spec/compatibility](https://cairn.kage1020.com/spec/compatibility/): every public surface sits
  in **Stable**, **Evolving**, or **Internal**, with a milestone-indexed table showing when each
  surface graduates.
- [Roadmap](https://cairn.kage1020.com/roadmap/) published, with M1–M6 milestones and a monthly
  scope plan through `2027.6.0`.

### Changed (Java backend Rust API — affects `cairn-lang-formats` consumers)

- `cairn_lang_formats::JavaTarget` is no longer `Copy`. The struct now
  owns its `mc_version: String` (sourced from a registry pack at runtime
  rather than the previous `&'static str` table), so the type implements
  `Clone` only. Direct callers of `build_structure_tag` /
  `write_structure_gzip` must pass `&JavaTarget` instead of moving the
  value. The CLI surface is unchanged.

### Added (executable slice for M1 — *source parses*)

- `cairn-lang-core::lex` — indent-aware lexer producing tokens with byte spans and 1-based
  line/column positions; rejects tab indentation and odd-spaced indents.
- `cairn-lang-core::ast` — surface-level AST (`Module`, `Header`, `Item`, `ThemeRule`,
  `Command`, `Arg`, `Value`, `Extra`, `Expr`) with `serde::Serialize` derived throughout.
- `cairn-lang-core::parse` — hand-rolled recursive-descent parser covering headers
  (`@cairn`, `@requires`, `@intended_targets`), `theme` / `def` / `site` / `struct`
  blocks, nested commands, bracketed selectors, sensor `-> binding` tails, positional
  args (for `connect a to b`), and the `logic` / `assert truth|always` special forms.
- `cairn parse <file> [--format json|debug]` — CLI subcommand backed by `clap` derive.
  Errors are emitted in `gcc`/`clang` style (`error: file:line:col: message`) so editors
  can jump straight to the offending location.
- End-to-end coverage: 17 lexer tests, 27 parser unit tests, 4 `insta` snapshots over the
  files in `examples/`, and 6 CLI integration tests that round-trip every example through
  the binary.

### Robustness

- Lexer accepts `\n`, `\r\n`, and lone `\r` as a single logical newline (so files saved on
  Windows with `core.autocrlf=true` lex the same as on Linux).
- Column counter tracks Unicode scalar values, not bytes — `日本語` in a string literal no
  longer poisons the column number of every subsequent token.
- `UnexpectedChar` reports the actual `char` (multi-byte UTF-8 included) instead of a
  truncated single byte cast to `char`.
- A command line may carry at most one `-> binding` tail; the second `->` is now a hard error
  instead of silently overwriting the first binding.
- `@cairn` / `@requires` / `@intended_targets` reject an empty value, and
  `@intended_targets` rejects trailing tokens after the list literal.
- Parser error messages use a human-friendly `TokenKind` display
  (`expected `=`, got identifier `foo``) instead of leaking the Rust `Debug` form.
- All public enums in `ast`, `lex`, and `error` are `#[non_exhaustive]`, reserving room to
  add variants in later milestones without breaking downstream crates.
- `LexError` / `ParseError` expose `position()` and `user_message()` accessors so callers
  (CLI, future LSP) can compose diagnostics without re-parsing the Display string.

### Changed (AST surface — affects `cairn parse` JSON / YAML output)

- `TruthRow.output` is now serialised as a JSON boolean (`true` / `false`) instead of the
  numeric `0` / `1` it shipped with. Any external tool reading `cairn parse --format json`
  output and treating that field as an integer must be updated.
- `Position.line` / `Position.col`, `Value::Size.w` / `Value::Size.h`, and the `within` bound
  of `assert always(...)` carry stricter Rust types (`NonZeroU32`); on the wire the
  serialisation is still a plain integer, so consumers should see no change to the JSON shape.
- `@cairn` and `@requires` header values are wrapped in `RawVersion` / `RawRequirement`
  newtypes on the Rust side; they serialise transparently as the raw string, so external
  consumers see no shape change.
