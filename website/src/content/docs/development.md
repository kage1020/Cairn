---
title: "Developer Guide"
---

This guide covers the Rust workspace and the day-to-day contributor flow. If you are working on the
language spec rather than the compiler, start at the [Specification](/spec/) and
[CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md).

> The spec is the source of truth, and the Rust workspace implements it chapter by chapter: what it
> does not cover yet, the spec marks as not implemented. `cairn-lang-wasm` is still a placeholder.

## Workspace layout

| Path | Contents |
|---|---|
| `Cargo.toml` | Workspace root: shared lints, release profile, MSRV. |
| `rust-toolchain.toml` | The exact Rust version CI runs, with `rustfmt` and `clippy`. |
| `rustfmt.toml` | Edition 2024, `max_width = 100`. |
| `crates/` | The Rust workspace (below). |
| `editors/vscode/` | VS Code extension. |
| `examples/` | Worked `.crn` examples. |
| `website/` | This site: Astro + Starlight, including the specification, and the `cairn-lang-docs` crate that embeds its pages. |

Each crate maps back to the spec chapter it implements:

| Crate | Role | Kind |
|---|---|---|
| [`cairn-lang-core`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-core/README.md) | Parser, IR, resolver, lint | lib |
| [`cairn-lang-cli`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-cli/README.md) | The `cairn` binary | bin |
| [`cairn-lang-nbt`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-nbt/README.md) | Java / Bedrock NBT codec | lib |
| [`cairn-lang-formats`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-formats/README.md) | `.nbt` / `.mcstructure` writers, registry packs, portability | lib |
| [`cairn-lang-redstone`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-redstone/README.md) | Logic synthesis and place-and-route; tick simulation is planned | lib |
| [`cairn-lang-lsp`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-lsp/README.md) | Language Server Protocol | lib + bin |
| [`cairn-lang-wasm`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-wasm/README.md) | WebAssembly bindings | cdylib + rlib |
| [`cairn-lang-tree-sitter`](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-tree-sitter/README.md) | Grammar for editor highlighting, also published to npm | lib |
| [`cairn-lang-docs`](https://github.com/kage1020/Cairn/blob/main/website/rust/README.md) | This site's Markdown pages, embedded for `cairn docs` | lib |

## Dependency rules

`cairn-lang-core` sits at the root. `cairn-lang-cli`, `cairn-lang-lsp`, and `cairn-lang-wasm` are
leaf integrations that nothing depends on. `cairn-lang-formats` is the only crate that pulls in
`cairn-lang-nbt`. `cairn-lang-docs` depends on nothing. Its manifest is `website/Cargo.toml`, so the
pages it embeds are the site's own sources, with no copy to drift.

- **`cairn-lang-core` knows nothing about NBT, file formats, redstone simulation, or editor
  protocols.** The block-array IR is the universal pivot ([Architecture](/spec/architecture/));
  everything beyond it lives in a sibling crate.
- **`cairn-lang-nbt` is the byte codec and nothing more.** Litematica regions, schematic palettes,
  and Bedrock's `.mcstructure` quirks belong in `cairn-lang-formats`.
- **`cairn-lang-redstone` reads sensors, actuators, and `circuit` reservations from core's Intent
  IR**, then lays its own pads and owns its own IR layers
  ([Redstone §14.8](/spec/redstone#148-connection-to-the-ir-and-phases)).

## Toolchain

| Tool | Pinned by | Notes |
|---|---|---|
| Rust | `rust-toolchain.toml` | An exact version, not a channel. With `rustfmt` and `clippy`. |
| Edition 2024, MSRV | `Cargo.toml` | Workspace package metadata, inherited by every crate. |
| Formatting | `rustfmt.toml` | `max_width = 100`, Unix line endings. |
| Lints | `[workspace.lints]` in `Cargo.toml` | `unsafe_code = forbid`, `missing_docs = warn`, `clippy::all` + `clippy::pedantic`. |

The Rust version is exact because CI treats every clippy finding as fatal. On a channel, a Rust
release would turn every open branch red by itself, with findings in files the branch never
touched. The pin decides when new lints arrive, not whether they do: bumping it is a pull request of
its own, carrying whatever the new release found.

The pin is not the MSRV. `rust-version` is the floor a consumer needs, and raising the pin does not
raise it. That number lives only in `Cargo.toml`. CI's `MSRV` job reads it back out with
`cargo metadata` and checks the workspace with that compiler, so a change that reaches for a newly
stabilised API goes red in CI rather than for a consumer.

Every crate except the tree-sitter one inherits these with `[lints] workspace = true` and writes no
`[lints.*]` table of its own; the next paragraph explains why the tree-sitter crate carries a copy.
Inheritance is opt-in per crate and all-or-nothing. A crate without that line receives none of the
workspace lints, so a lint added to the workspace later would silently skip it.

`unsafe_code` is forbidden everywhere except one module: `ffi` in the tree-sitter crate's Rust
binding, which is the only way to reach the generated C parser. `forbid` cannot be lifted, so that
crate alone carries a copy of the workspace lints with `unsafe_code` at `deny`, and `ffi` lifts it
with `#![expect(unsafe_code)]`. `cargo xtask lint` keeps the exception that narrow:

- its `manifest` rule fails if the workspace level is anything but `forbid`, if a crate does not
  inherit the workspace lints, or if the tree-sitter copy differs from them in anything else;
- its `unsafe-code` rule fails if an `allow`, `expect`, `warn` or `cfg_attr` attribute names
  `unsafe_code` outside that module.

If another use case ever needs `unsafe`, it lifts the lint on a single module with documented
invariants and adds that file and module to the rule's `ALLOWED` list. It never uses `#[allow]` at a
call site.

## Build, test, lint

CI runs these on Linux, macOS, and Windows, checks the workspace once more at the
declared MSRV, and builds the API docs on Linux (below). Run them before opening a PR.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
cargo test -p cairn-lang-core --bench lowering
cargo test -p cairn-lang-redstone --bench place_and_route
```

`--workspace` does not select bench targets, so the last two run each bench once, as an untimed
test.

CI's `Lint` job also runs `cargo xtask lint`, which compares files with each other rather than
testing behaviour: README inventories, spec citations, the diagnostic catalog, the copies of the
version, and the lint configuration. `cargo xtask lint --list` names the rules, and each rule's
module in `xtask/src/rules/` explains why it exists.

CI sets `RUSTFLAGS=-D warnings`, so any new warning fails the build. To match it locally:

```sh
RUSTFLAGS="-D warnings" cargo build --workspace --locked
```

CI also builds the API docs twice on Linux, with rustdoc's warnings fatal, because clippy does not
see a doc link to a private item or to a path that no longer resolves. The first run covers the
public surface, and it is the only one that flags a public doc linking to a private item. The
second also covers the docs only a contributor reads:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features --document-private-items
```

Two benches time the passes a build spends most of its time in: `lowering` in `cairn-lang-core`
and `place_and_route` in `cairn-lang-redstone`. They call the passes in-process, since timing the
CLI over an example would mostly time process startup. They generate their sources, since an
example-sized source goes through a pass faster than the timer can tell a change apart. They take
their settings from the release profile, except `panic`: Cargo builds benches with unwinding, as it
does tests.

CI runs the benches once as tests, as above, but never times them. They exist to measure a change to
`[profile.release]`, above all to `opt-level`, before it is made.
[CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md) walks through
comparing two profiles with them. To run both:

```sh
cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route
```

`cairn-lang-wasm` has nothing to build for the browser yet: it has no `wasm-bindgen` binding layer,
so `wasm-pack` refuses it. Its
[README](https://github.com/kage1020/Cairn/blob/main/crates/cairn-lang-wasm/README.md) says what is
missing and gives the command for when the binding layer lands.

### The examples in Minecraft

`cargo test` holds what Cairn writes to Cairn's own reading of each format. Whether the game reads
a file the same way is the `Minecraft` workflow's job. It runs on a pull request that touches the
examples, lowering, the writers or the registry packs, on `canary`, and weekly. For each targetable
row of the built-in Java pack, it compiles every example and has that version's server jar load
each `.nbt` through its data generator. `--dev` turns the files into SNBT, and `--server` turns
them back into structures through the DataFixer and the template loader `/place template` uses. A
block, property, `DataVersion` or size the game reads differently from what was written fails the
job, which names the file and the cell.

To run it locally, with Java 21 and network access to Mojang:

```sh
cargo build -p cairn-lang-cli
python3 .github/scripts/minecraft_load.py --cairn target/debug/cairn --target 1.21.4
```

Leave out `--target` to check every shipped version. The script downloads each server jar from
Mojang, checks it against the SHA-1 Mojang publishes, and keeps it under `~/.cache/cairn-minecraft`.
The jars are used under the Minecraft EULA and are never committed. The data generator writes no
world, so it needs no `eula.txt`.

## Conventions for Rust code

- **The spec is the source of truth.** When spec and implementation disagree, fix the
  implementation. If the spec is wrong, send a spec PR first.
- **No linter-ignore directives.** `#[allow(clippy::…)]`, `#[allow(dead_code)]`, and friends are not
  allowed. If a lint fires, the design is the bug. Where a lint is wrong for one item, put
  `#[expect(lint, reason = "…")]` on that item alone.
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

The spec is compact enough that an acceptance-criteria list almost always fits in a few lines, so
there is nothing to gain by skipping ahead.

## Adding a format backend

Format support lives in `cairn-lang-formats`. A new file type needs a writer from the block-array IR
to bytes. Importing it, the reverse direction, needs two more things, and no format has them yet:

1. A reader from bytes to the block-array IR.
2. An `(edition, version)` provenance stamp on import
   ([Ecosystem Interop §12.4](/spec/ecosystem-interop#124-import-stamping-and-pitfalls)).

If you find yourself reaching into `cairn-lang-core` to add format-specific fields, format concerns
are leaking into the block-array IR. Discuss it before merging.

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

Open an issue against the relevant spec chapter. A question about implementation alone can point
at the crate README instead. Design questions about vocabulary, IR shape, or error message wording
belong against the spec.
