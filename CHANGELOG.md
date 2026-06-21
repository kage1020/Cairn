# Changelog

> Language: **English** ([日本語](CHANGELOG.ja.md))

All notable changes to Cairn are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) so `release-plz` can append release
entries cleanly. Cairn uses date-based versioning (CalVer) `YYYY.0M[.PATCH]`. This is the version
of the language + reference compiler + standard library + registry/constraint packs as a bundle,
and is a separate axis from the Minecraft target version.

## [Unreleased]

The first publicly-numbered release will be **`2026.07.0`** (planned). Until then this section
records what has been built into the repository in preparation for that release. No `cairn-lang-*`
crate has been published to crates.io yet; `[workspace.package].publish` is `false` so the `0.0.0`
placeholder cannot leak out. The `2026.07.0` release PR will flip publish to `true`.

### Added

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
- `cairn-lang-formats::data_version` — hardcoded
  (`mc_version`, `DataVersion`) table covering 1.20.4, 1.21, and 1.21.4
  plus the `latest` alias. The 2026.12.0 registry pack ingest replaces
  this table with values pulled from a versioned data file.
- `cairn_lang_core::lock` — `build.cairn.lock` reader/writer matching
  `spec/versioning-editions.md` §10.6. Keys appear in the spec-printed
  order (`source_hash, cairn_version, target, inputs,
  resolved_ir_hash, verified, member_version_sensitivity`).
  `hash_source` and `hash_resolved_ir` (sha256 over UTF-8 source bytes
  and over the IR's JSON serialisation, respectively) give the lockfile
  its reproducibility anchor. `LockInputs::zero()` ships zero hashes
  until the registry pack and constraint catalog land.
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
- Release strategy: monthly minor (`YYYY.0M.0`) by GitHub Actions cron at 04:17 UTC on the 1st,
  plus on-demand patches (`YYYY.0M.N`) triggered by qualifying commits on `canary`. The release
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
  scope plan through `2027.06.0`.

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
