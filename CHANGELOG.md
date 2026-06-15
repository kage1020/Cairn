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
