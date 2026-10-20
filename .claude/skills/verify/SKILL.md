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
before pushing: every crate depends on `cairn-lang-core`, so a core change can break any of them.

Add these when the change reaches them:

- `crates/cairn-lang-tree-sitter/grammar.js` or its corpus: `pnpm install`, `pnpm run generate`
  (the generated files must come out unchanged after you commit them) and `pnpm test`, in that
  crate's directory.
- `website/`: `pnpm install`, `pnpm run check` and `pnpm run build` in `website/`, then
  `cargo xtask lint --site website/dist site-parity` from the root.
- `editors/vscode/`: `pnpm install`, `pnpm run package` and `pnpm test` in `editors/vscode/`.
  `pnpm test` opens VS Code, so without a display run it as `xvfb-run -a pnpm test`.
- A new dependency or a recently stabilised API: CI's `MSRV` job runs
  `cargo +<rust-version> check --workspace --locked --all-features` at the `rust-version` in
  `Cargo.toml`.

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
