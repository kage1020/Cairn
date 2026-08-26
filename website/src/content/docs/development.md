---
title: "Developer Guide"
---

This guide covers the Rust workspace and the day-to-day contributor flow. If you are working on the
language spec rather than the compiler, start at the [Specification](/spec/) and
[CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md).

> Cairn is at the design stage. The spec is the source of truth and the Rust skeleton implements it
> chapter by chapter. Several crates are still empty.

## Workspace layout

| Path | Contents |
|---|---|
| `Cargo.toml` | Workspace root: shared lints, release profile, MSRV. |
| `rust-toolchain.toml` | Pinned to stable, with `rustfmt` and `clippy`. |
| `rustfmt.toml` | Edition 2024, `max_width = 100`. |
| `crates/` | The Rust workspace (below). |
| `editors/vscode/` | VS Code extension. |
| `examples/` | Worked `.crn` examples. |
| `website/` | This site: Astro + Starlight, including the specification. |

Each crate maps back to the spec chapter it implements:

| Crate | Role | Kind |
|---|---|---|
| [`cairn-lang-core`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-core/README.md) | Parser, IR, resolver, lint | lib |
| [`cairn-lang-cli`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-cli/README.md) | The `cairn` binary | bin |
| [`cairn-lang-nbt`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-nbt/README.md) | Java / Bedrock NBT codec | lib |
| [`cairn-lang-formats`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-formats/README.md) | `.nbt` / `.litematic` / `.schem` / `.mcstructure` | lib |
| [`cairn-lang-redstone`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-redstone/README.md) | Logic synthesis, place-and-route, tick simulation | lib |
| [`cairn-lang-lsp`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-lsp/README.md) | Language Server Protocol | lib + bin |
| `cairn-lang-wasm` | WebAssembly bindings | cdylib + rlib |
| `cairn-lang-tree-sitter` | Grammar for editor highlighting | grammar |

## Dependency rules

`cairn-lang-core` sits at the root. `cairn-lang-cli`, `cairn-lang-lsp`, and `cairn-lang-wasm` are
leaf integrations that nothing depends on. `cairn-lang-formats` is the only crate that pulls in
`cairn-lang-nbt`.

- **`cairn-lang-core` knows nothing about NBT, file formats, redstone simulation, or editor
  protocols.** The block-array IR is the universal pivot ([Architecture](/spec/architecture/));
  everything beyond it lives in a sibling crate.
- **`cairn-lang-nbt` is the byte codec and nothing more.** Litematica regions, schematic palettes,
  and Bedrock's `.mcstructure` quirks belong in `cairn-lang-formats`.
- **`cairn-lang-redstone` reuses core's sensor and actuator placement** but owns its own IR layers
  ([Redstone §14.8](/spec/redstone#148-connection-to-the-ir-and-phases)).

## Toolchain

| Tool | Pinned by | Notes |
|---|---|---|
| Rust stable | `rust-toolchain.toml` | With `rustfmt` and `clippy`. |
| Edition 2024, MSRV 1.95 | `Cargo.toml` | Workspace package metadata. |
| Formatting | `rustfmt.toml` | `max_width = 100`, Unix line endings. |
| Lints | `[workspace.lints]` in `Cargo.toml` | `unsafe_code = forbid`, `missing_docs = warn`, `clippy::all` + `clippy::pedantic`. |

`unsafe_code` is forbidden workspace-wide with no escape hatch. If a use case ever needs it, it goes
through a focused PR that lifts the lint on a single module with documented invariants, never
`#[allow]` at a call site.

## Build, test, lint

CI runs these four on Linux, macOS, and Windows. Run them before opening a PR.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
```

CI sets `RUSTFLAGS=-D warnings`, so any new warning fails the build. To match it locally:

```sh
RUSTFLAGS="-D warnings" cargo build --workspace --locked
```

`cairn-lang-wasm` builds with [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and the website
expects the resulting `pkg/` at `website/src/wasm/`:

```sh
wasm-pack build crates/cairn-lang-wasm --target web --release
```

## Conventions for Rust code

- **The spec is the source of truth.** When spec and implementation disagree, fix the
  implementation. If the spec is wrong, send a spec PR first.
- **No linter-ignore directives.** `#[allow(clippy::…)]`, `#[allow(dead_code)]`, and friends are not
  allowed. If a lint fires, the design is the bug.
- **Lift the spec's terms verbatim.** `IntentState`, `ResolvedState`, `MatSlot`, `CanonicalToken`,
  `BlockArrayIr`. Do not invent parallel vocabulary.
- **`missing_docs` is a warning everywhere.** Every public item gets a `///` line, and every crate a
  `//!` block.
- **No Minecraft target constants.** The `(edition, version)` pair is a CLI parameter and must never
  appear in the language semantics ([Compilation Model §4.2](/spec/compilation#42-target-axes)).
- **Errors carry the self-correction triple:** what is wrong / valid candidates / suggested fix
  ([Lint](/spec/lint/)).

## TDD discipline

1. **Design.** Read the relevant spec chapter and restate the slice you are implementing in plain
   prose.
2. **Acceptance criteria.** Write them as bullets, before any code.
3. **Tests.** Translate the ACs into `#[test]` functions.
4. **Implementation.** Make them pass.
5. **Iterate.** Keep tests and implementation in lockstep until green.

The spec is compact enough that an AC list almost always fits in a few lines. There is no value in
skipping ahead.

## Adding a format backend

Format support lives in `cairn-lang-formats`. A new file type needs three things:

1. A reader from bytes to the block-array IR.
2. A writer from the block-array IR to bytes.
3. An `(edition, version)` provenance stamp on import
   ([Ecosystem Interop §12.4](/spec/ecosystem-interop#124-import-stamping-and-pitfalls)).

If you find yourself reaching into `cairn-lang-core` to add format-specific fields, the block-array
IR is leaking format concerns. Discuss before merging.

## Adding redstone primitives

The v1 vocabulary is closed ([Redstone §14.1](/spec/redstone#141-two-tiers-and-the-v1-boundary)):
combinational gates plus `latch`, `pulse`, `delay`, `edge_rising`, `edge_falling`, and `counter`.
Adding to it is a **spec change**. Open a spec PR with:

- the new primitive's signal-graph semantics,
- whether it is combinational or sequential,
- the per-edition cell library entry it lowers to,
- the truth-table, latency, and temporal assertions it must satisfy in the headless simulator
  ([Evaluation Framework §13.4](/spec/evaluation#134-redstone-verification)).

## Where to ask

Open an issue against the relevant spec chapter. Implementation-only questions can reference the
crate README. Design questions about vocabulary, IR shape, or error message wording belong against
the spec.
