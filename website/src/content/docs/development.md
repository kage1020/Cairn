---
title: "Cairn — Developer Guide"
---

This guide documents the **Rust workspace** and the day-to-day developer flow. It is the entry
point for contributors writing Rust code; for contributors working on the language spec itself,
start in [`spec/`](../spec/README) and consult [CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md).

> Cairn is at the design stage. The spec is the source of truth, the Rust skeleton implements it
> chapter by chapter. Many crates are still empty.

## Workspace layout

```
Cairn/
├── Cargo.toml              # workspace root, shared lints, release profile
├── rust-toolchain.toml     # pinned to stable + rustfmt + clippy
├── rustfmt.toml            # edition = 2024, max_width = 100
├── crates/
│   ├── cairn-core/         # parser, IR, resolver, lint    (lib)
│   ├── cairn-cli/          # `cairn` binary                (bin)
│   ├── cairn-nbt/          # Java/Bedrock NBT codec        (lib)
│   ├── cairn-formats/      # .nbt / .litematic / .schem / .mcstructure  (lib)
│   ├── cairn-redstone/     # logic synth + P&R + tick sim  (lib)
│   ├── cairn-lsp/          # Language Server Protocol      (lib)
│   └── cairn-wasm/         # WebAssembly bindings          (cdylib + rlib)
├── spec/                   # normative specification (Markdown)
├── docs/                   # this guide, tutorial, etc.
├── examples/               # worked .crn examples
└── website/                # Astro + Starlight documentation site
```

Each crate has its own README that maps it back to the spec chapter it implements:

- [`cairn-core`](../crates/cairn-core/README)
- [`cairn-cli`](../crates/cairn-cli/README)
- [`cairn-nbt`](../crates/cairn-nbt/README)
- [`cairn-formats`](../crates/cairn-formats/README)
- [`cairn-redstone`](../crates/cairn-redstone/README)
- [`cairn-lsp`](../crates/cairn-lsp/README)
- [`cairn-wasm`](../crates/cairn-wasm/README)

## Crate dependency graph

```
                 ┌──────────────┐
                 │  cairn-core  │  ← parser, IR, resolver, lint
                 └──────┬───────┘
        ┌───────────────┼──────────────┬──────────────┬─────────────┐
        │               │              │              │             │
┌───────▼──────┐ ┌──────▼──────┐ ┌─────▼──────┐ ┌─────▼──────┐ ┌────▼────────┐
│ cairn-cli    │ │ cairn-       │ │ cairn-     │ │ cairn-lsp  │ │ cairn-wasm  │
│ (`cairn`)    │ │ formats      │ │ redstone   │ │ (LSP)      │ │ (WASM)      │
└──────────────┘ └─────┬────────┘ └────────────┘ └────────────┘ └─────────────┘
                       │
                 ┌─────▼──────┐
                 │ cairn-nbt  │  ← byte-level NBT codec only
                 └────────────┘
```

Rules of thumb:

- **Nothing depends on `cairn-cli`, `cairn-lsp`, or `cairn-wasm`.** They are leaf integrations.
- **`cairn-core` knows nothing about NBT, file formats, redstone simulation, or editor protocols.**
  The block-array IR is the universal pivot ([architecture
  §3.1](../spec/architecture)); everything beyond it lives in a sibling crate.
- **`cairn-nbt` is just the byte codec.** Litematica regions, schematic palettes, and Bedrock's
  `.mcstructure` quirks live in `cairn-formats`.
- **`cairn-redstone` reuses `cairn-core` sensor/actuator placement** but owns its own IR layers
  (Logic / Netlist / Placement; see [redstone §14.8](../spec/redstone)).

## Toolchain

| Tool | Pinned by | Notes |
|---|---|---|
| Rust stable | [`rust-toolchain.toml`](../rust-toolchain.toml) | `rustfmt` + `clippy` components |
| Edition 2024, MSRV 1.95 | [`Cargo.toml`](../Cargo.toml) | Workspace package metadata |
| Formatting | [`rustfmt.toml`](../rustfmt.toml) | `max_width = 100`, Unix line endings |
| Lints | `[workspace.lints]` in [`Cargo.toml`](../Cargo.toml) | `unsafe_code = forbid`, `missing_docs = warn`, `clippy::all` + `clippy::pedantic` |

`unsafe_code` is **forbidden workspace-wide**. There is no escape hatch; if a use case ever appears
that needs unsafe, it goes through a focused PR that lifts the lint on a single module with
documented invariants — never with `#[allow]` sprinkled at a call site.

## Build, test, lint

The CI pipeline (`.github/workflows/ci.yml`) runs the four commands below on Linux, macOS, and
Windows. Run them locally before opening a PR.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
```

`RUSTFLAGS=-D warnings` is set in CI, so any new warning fails the build. Locally, you can match CI
with:

```sh
RUSTFLAGS="-D warnings" cargo build --workspace --locked
```

### WASM build

`cairn-wasm` builds with [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and is consumed by
the website:

```sh
wasm-pack build crates/cairn-wasm --target web --release
```

The website expects the resulting `pkg/` directory to live at
`website/src/wasm/` (see [`website/README.md`](../website/README)).

## Conventions for Rust code

The conventions here complement [CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md) — that document governs the
spec; this section governs the Rust code that implements it.

- **The spec is the source of truth.** When the spec and the implementation disagree, fix the
  implementation; if the spec is genuinely wrong, send a spec PR first. Do not let the
  implementation silently drift.
- **No linter-ignore directives.** `#[allow(clippy::…)]`, `#[allow(dead_code)]`, etc., are not
  allowed. If a lint fires, the design is the bug.
- **Stable, meaning-based names.** Type and module names mirror the spec terminology
  (`IntentState`, `ResolvedState`, `MatSlot`, `CanonicalToken`, `BlockArrayIr`). Do not invent
  parallel vocabulary; lift the spec's terms verbatim.
- **`missing_docs` is a warning everywhere.** Every public item gets a `///` line. Module-level
  `//!` blocks summarize each crate's role.
- **No external constants for the Minecraft target.** The version/edition pair is a CLI-level
  parameter and must never appear in the language semantics
  ([compilation §4.2](../spec/compilation)).
- **Errors carry the self-correction triple.** Diagnostic messages MUST be in the "what is wrong
  / valid candidates / suggested fix" shape so they can feed the lint loop
  ([lint](../spec/lint)).

## TDD discipline

The project follows the t_wada-style TDD order documented in the global rules:

1. **Design** — read the relevant spec chapter; restate the slice you are about to implement in
   plain prose.
2. **Acceptance Criteria** — write ACs as bullet points first, before any code or test code.
3. **Test code** — translate the ACs into `#[test]` functions.
4. **Implementation** — make the tests pass.
5. **Iterate** — keep tests/implementation in lockstep until green.

There is no value in skipping ahead. The spec is intentionally compact; an AC list almost always
fits in a few lines.

## Adding a new format backend

Format support lives in [`cairn-formats`](../crates/cairn-formats/README). To add a new file
type, you only need to:

1. Add a reader from the bytes to the block-array IR.
2. Add a writer from the block-array IR to the bytes.
3. Stamp the `(edition, version)` provenance on import
   ([ecosystem-interop §12.4](../spec/ecosystem-interop)).

If you find yourself reaching into `cairn-core` to add format-specific fields, that is a sign the
block-array IR is leaking format concerns — push back and discuss before merging.

## Adding redstone primitives

The v1 vocabulary is closed ([redstone §14.1](../spec/redstone)): combinational gates plus a
curated macro list (`latch` / `pulse` / `delay` / `edge_rising` / `edge_falling` / `counter`).
Adding to this list is a **spec change**, not just an implementation change. Open a spec PR with:

- the new primitive's signal-graph semantics,
- whether it is combinational or sequential,
- the per-edition cell library entry it lowers to,
- truth-table / latency / temporal assertions it must satisfy in the headless simulator
  ([evaluation §13.4](../spec/evaluation)).

## Where to ask questions

Open an issue against the relevant spec chapter. Implementation-only questions can reference the
crate README; design questions (vocabulary, IR shape, error message wording) belong against the
spec.
