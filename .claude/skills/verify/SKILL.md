---
name: verify
description: Run the checks Cairn's CI runs, before pushing a branch or opening a pull request.
---

# Verify a change

CI treats every warning as an error. Run the steps in order, fix the first one that fails, and
rerun from there.

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo test -p cairn-lang-core --bench lowering
cargo test -p cairn-lang-redstone --bench place_and_route
cargo xtask lint
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features --document-private-items
python3 .github/scripts/changelog.py check
python3 -m unittest discover -s .github/scripts
```

While iterating on one crate, `-p <crate>` on clippy and test is enough. Run the whole list once
before pushing: most crates depend on `cairn-lang-core`, so a core change can break any of them.

Add these when the change reaches them:

- `crates/cairn-lang-tree-sitter/grammar.js` or its corpus: `pnpm install`, `pnpm run generate`
  (the generated files must come out unchanged after you commit them) and `pnpm test`, in that
  crate's directory.
- `website/`: `pnpm install`, `pnpm run check` and `pnpm run build` in `website/`, then
  `cargo xtask lint --site website/dist site-parity` from the root.
- `editors/vscode/`: `pnpm install`, `pnpm run package` and `pnpm test` in `editors/vscode/`.
  `pnpm test` opens VS Code and skips on Windows. CI runs it on Linux only, as
  `xvfb-run -a pnpm test`, which is also how to run it without a display.
- `examples/`, `cairn-lang-core`, `cairn-lang-nbt`, `cairn-lang-formats` and its registry packs,
  `cairn-lang-cli` or `Cargo.lock`: the `Minecraft` workflow's `Examples load in Minecraft Java`
  check loads every example's `.nbt` in the game at each shipped Java target. To run it, with Java
  21 (or `--java <path>`) and network access to Mojang: `cargo build -p cairn-lang-cli`, then
  `python3 .github/scripts/minecraft_load.py --cairn ./target/debug/cairn`.
- A new dependency or a recently stabilised API: CI's `MSRV` job runs
  `cargo +<floor> check --workspace --locked --all-features`, where the floor is the `rust-version`
  in `Cargo.toml` with `.0` appended (`1.95` → `1.95.0`), since rustup reads `1.95` as the newest
  `1.95.x`.

## When a step fails

- **clippy**: change the code. Never add `#[allow]`; when a lint is wrong for one item, use
  `#[expect(lint, reason = "...")]` on that item.
- **insta snapshot**: read the `.snap.new` next to the old snapshot. Accept it (`cargo insta
  accept`, or rename it over the `.snap`) only when the new output is the behaviour you meant.
- **`cargo xtask lint`**: each finding names the file, line and rule, and
  `cargo xtask lint --list` says what each rule holds. Fix the file it names. Run a single rule
  with `cargo xtask lint <rule>`.
- **rustdoc**: usually a public doc linking to a private item, or an intra-doc path that no longer
  resolves.
- **No space left on device**: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` roughly
  halves `target/`.
