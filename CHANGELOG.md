# Changelog

> Language: **English** ([日本語](CHANGELOG.ja.md))

All notable changes to Cairn are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) so `release-plz` can append release
entries cleanly. Cairn uses date-based versioning (CalVer) `YYYY.M[.PATCH]`. This is the version
of the language + reference compiler + standard library + registry/constraint packs as a bundle,
and is a separate axis from the Minecraft target version.

## [Unreleased]

### Added

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
  per `spec/lint` §11.4's self-correction triple:
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
  per `spec/lint` §11.4's self-correction triple:
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
