---
title: "Compatibility Tiers"
---

Cairn ships a single release train under [date-based versioning](versioning-editions#101-the-target-is-a-compile-time-parameter)
(`YYYY.0M[.PATCH]`). Because CalVer has no semver-style "major" axis, the **scope of what is and
is not safe to break in a release** is set by this document rather than by version numbers.

Every public surface area of the project sits in exactly one of three tiers: **Stable**,
**Evolving**, or **Internal**. The tier sets the rules; the version number only records when
something happened.

## C.1 Tier definitions

### Stable

The contract is: **breaking changes are announced one release in advance with a `W_DEPRECATED`
warning, and removed in the following monthly minor at the earliest.**

- A breaking change reaching `Stable` MUST be referenced from the next month's CHANGELOG.
- Renames MUST keep the old name working for the deprecation window.
- Default values MAY change with a warning; semantic meaning MUST NOT.
- Downstream consumers can pin a minor version (`cairn ~= 2026.07`) and expect at least the
  next monthly minor to compile their inputs unchanged.

Stable surfaces:

- `.crn` syntax that is normative in the spec (keywords, headers, block kinds, blockstate
  primitives, theme/material primitives, edition guards).
- `build.cairn.lock` file format (fields, hash inputs, the `verified` semantics defined in
  [§10.6](versioning-editions#106-provenance-and-lock-reproducibility)).
- `cairn compile`, `cairn check`, `cairn info` — their flag names, argument shapes, JSON
  output schemas, and process exit codes.
- The canonical material vocabulary's tier-1 tokens (the names users write in source).
- Error/warning codes documented in the spec (`E_*`, `W_*`).

### Evolving

The contract is: **breaking changes ship in any monthly minor and are listed in the CHANGELOG.**
No deprecation window is promised.

- Consumers SHOULD read the CHANGELOG before bumping the minor.
- Patches MUST NOT introduce Evolving-tier breaks; only monthly minors may.
- A surface MAY graduate from Evolving to Stable by being moved in this document plus
  CHANGELOG; demotion (Stable → Evolving) is forbidden and constitutes a spec amendment.

Evolving surfaces:

- Spec chapters or sections explicitly marked as draft (the
  [Open Issues](open-issues) list and any section flagged "subject to change" in its prose).
- New `cairn` subcommands during their first three monthly minors after introduction.
- registry pack and constraint catalog file layout (their *hashes* feed Stable lock semantics,
  but the *internal structure* is Evolving).
- Tier-2 canonical material tokens (the implementation-level names that tier-1 names resolve
  to).
- The CLI's human-readable diagnostic format and prose (the *codes* are Stable; the *prose*
  is not).
- The `--features` set on `cairn-lang-cli` (cargo features) and `[features]` on workspace crates.

### Internal

The contract is: **no promise. Any release may change anything. Consumers depending on Internal
surfaces are responsible for their own pinning.**

Internal surfaces:

- The Rust API of every workspace crate (`cairn-lang-core`, `cairn-lang-nbt`, `cairn-lang-formats`,
  `cairn-lang-redstone`, `cairn-lang-lsp`, `cairn-lang-wasm`). These crates are published to crates.io from
  the first monthly minor that contains them, but every item that is not the CLI's transitive
  public dependency is marked `#[doc(hidden)]`.
- The compiler's intermediate representation (Intent IR, Semantic IR, block-array pivot
  layouts).
- The on-disk shape of incremental build caches (`target/`-equivalents inside the project
  workspace).
- The internal protocol of the language server (its on-wire LSP protocol with VS Code etc. is
  Stable; the way `cairn-lang-lsp` decomposes work internally is not).

## C.2 Tier of each surface, by milestone

The tier of a given surface is not fixed for all time — it earns its way to Stable as the
project matures along the [roadmap](/roadmap/). The table is the canonical record.

| Surface | Today (pre-M1) | At M2 (minimal build) | At M3 (examples work) | At M5 (DX) | At M6 (redstone) |
|---|---|---|---|---|---|
| `.crn` syntax (chapters 5-9) | Evolving | Evolving | **Stable** | Stable | Stable |
| `build.cairn.lock` format | Evolving | **Stable** | Stable | Stable | Stable |
| `cairn compile/check/info` flags | Evolving | Evolving | **Stable** | Stable | Stable |
| Diagnostic codes (`E_*`, `W_*`) | Evolving | Evolving | **Stable** | Stable | Stable |
| Tier-1 material vocabulary | Evolving | Evolving | **Stable** | Stable | Stable |
| LSP wire protocol (LSP std) | — | — | — | **Stable** | Stable |
| Redstone DSL (chapter 14) | Evolving | Evolving | Evolving | Evolving | **Stable** |
| Tier-2 vocabulary, registry pack layout | Evolving | Evolving | Evolving | Evolving | Evolving |
| Rust API (every crate) | Internal | Internal | Internal | Internal | Internal |

Reading the table: **the column where a row first turns Stable is the soft commitment.** Earlier
columns being Evolving means the project reserves the right to change those surfaces without a
deprecation window until that column lands.

The Rust API row never graduates within the planned roadmap. Cairn is consumed through the
`cairn` CLI binary and the language server, not as a Rust library. If a downstream consumer
wants a stable embedding API the project will treat that as a new, separately-tracked surface.

## C.3 How a break is communicated

Regardless of tier, every breaking change MUST appear in the CHANGELOG in a section named
`Breaking changes`. For Stable surfaces this is the second appearance — the first is the
preceding release's `Deprecations` section.

```text
## 2026.11.0 — 2026-11-01

### Breaking changes
- `cairn compile --java-target` removed (deprecated in 2026.10.0). Use `--target` with
  `--edition java`.

### Deprecations
- `slot` keyword without an arrow form is now deprecated; emits `W_DEPRECATED_SLOT_ARROW`.
  Will be removed no earlier than 2026.12.0.
```

`W_DEPRECATED` and `E_BREAKING` codes are themselves Stable. Adding a new code is not a
breaking change; changing the meaning of an existing code is.

## C.4 What is not covered by tier

Two classes of change sit outside this matrix entirely:

- **Bug fixes** that align behaviour with the spec are never breaking, even if a consumer was
  depending on the buggy behaviour. The spec, not the implementation, defines the contract.
- **Output bit-equality** of emitted `.nbt` / `.litematic` / `.schem` / `.mcstructure` is not
  promised at any tier. Two releases may produce structurally-different files for the same
  source; what matters is that the result is valid for the target `(edition, version)` and
  matches the lockfile's `resolved_ir_hash`. See
  [§10.6](versioning-editions#106-provenance-and-lock-reproducibility).
