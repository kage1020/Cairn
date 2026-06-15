# Changelog

> Language: **English** ([日本語](CHANGELOG.ja.md))

All notable changes to Cairn are documented here. Cairn uses date-based versioning (CalVer)
`YYYY.0M[.PATCH]`. This is the version of the language + reference compiler + standard library +
registry/constraint packs as a bundle, and is a separate axis from the Minecraft target version.

## [Unreleased]

Working toward **2026.07.0** (M1 — *source parses*). The first executable slice of the reference
compiler lands here: `cairn-core` gains a lexer, parser, and AST, and `cairn parse <file>` emits
the AST as JSON for every example under `examples/`.

### Added

- `cairn-core::lex` — indent-aware lexer producing tokens with byte spans and 1-based
  line/column positions; rejects tab indentation and odd-spaced indents.
- `cairn-core::ast` — surface-level AST (`Module`, `Header`, `Item`, `ThemeRule`, `Command`,
  `Arg`, `Value`, `Extra`, `Expr`) with `serde::Serialize` derived throughout.
- `cairn-core::parse` — hand-rolled recursive-descent parser covering headers
  (`@cairn`, `@requires`, `@intended_targets`), `theme` / `def` / `site` / `struct`
  blocks, nested commands, bracketed selectors, sensor `-> binding` tails, positional
  args (for `connect a to b`), and the `logic` / `assert truth|always` special forms.
- `cairn parse <file> [--format json|debug]` — CLI subcommand backed by `clap` derive.
- End-to-end coverage: 11 lexer tests, 14 parser unit tests, 4 `insta` snapshots over the
  files in `examples/`, and 6 CLI integration tests that round-trip every example through
  the binary.

## 2026.06 (draft)

Initial public design specification. The language is being designed in the open; no reference
compiler exists yet.

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
  wired to the future `cairn-wasm` bindings; Cloudflare's Git integration auto-deploys on every
  push to `main`.
