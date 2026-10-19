# Cairn

Cairn compiles `.crn` build descriptions into Minecraft structure files. The specification under
`website/src/content/docs/spec/` is normative. When code and spec disagree, the code is wrong, and
a change to behaviour the spec describes starts as a spec change.

[CONTRIBUTING.md](CONTRIBUTING.md) is the contributor guide and
[development.md](website/src/content/docs/development.md) the developer guide. This file is what
to keep in mind while editing.

## Workspace

| Crate | Role | Depends on |
|---|---|---|
| `cairn-lang-core` | lex, parse, AST, Intent IR, check, resolve, block-array lowering | — |
| `cairn-lang-nbt` | NBT codec | — |
| `cairn-lang-formats` | `.nbt` and `.mcstructure` writers, registry packs, portability | core, nbt |
| `cairn-lang-redstone` | logic IR, netlist, place-and-route | core |
| `cairn-lang-cli` | the `cairn` binary | core, docs, formats, redstone |
| `cairn-lang-lsp` | the `cairn-lsp` server | core, formats |
| `cairn-lang-wasm` | browser bindings, a placeholder | core |
| `cairn-lang-tree-sitter` | editor grammar, separate from core's parser | — |
| `cairn-lang-docs` | the site's Markdown pages, embedded; its manifest is `website/Cargo.toml` | — |
| `xtask` | `cargo xtask lint`, unpublished | docs |

Dependencies point one way. A library never depends on `cli` or `lsp`, so code both front ends
need lives in the library they call. Modules inside a crate form a DAG too: when two modules need
the same item, it moves down into a module both can import. `cargo xtask lint module-cycles` holds
the modules under each parent to that.

## Commands

The `verify` skill lists everything CI runs. While iterating:

```sh
cargo clippy -p <crate> --all-targets -- -D warnings
cargo test -p <crate>
cargo xtask lint
```

## Code

- Name things with the spec's vocabulary (`intent_state`, `resolved_state`, `mat_slot`, canonical
  token), never a synonym.
- No Minecraft edition or version in language semantics: the `(edition, version)` target is a
  compile-time parameter.
- A diagnostic says what is wrong, what would be valid, and how to fix it. The `diagnostic-code`
  skill covers adding one; `syntax-change` covers changing the syntax.
- Every public item has a `///` doc. No `#[allow]`: fix the lint, or put
  `#[expect(lint, reason = "...")]` on the one item it is wrong for.
- `unsafe` is forbidden outside the tree-sitter binding's `ffi` module.

## Comments

The code says how, the commit message says what, and a comment says only the why the code cannot.

- Default to no comment. Write one where a reader would otherwise make a wrong change, in a line.
- A `///` doc is one sentence of what the item is or returns; `# Errors` and `# Panics` get a line
  each.
- Never defend the code: no alternatives weighed, no "rather than", no list of callers, no
  restating what a name, type, assertion or test name already says.
- Never narrate history ("used to", "no longer", "previously"), or mention an issue, pull request,
  milestone, acceptance id or anything else that only means something to the session that wrote
  it.
- Cite the spec by chapter and section title, never by number: `spec/lint` "Error vs warning".
- `cargo xtask lint comment-length` caps a block at 3 lines of `//`, 4 of `///` and 10 of `//!`.
  The cap is a ceiling, not a target.
- A change that makes a comment false fixes the comment.

## Tests

- Test behaviour through the narrowest public surface that shows it. A module's unit tests that
  outgrow a screen move to a sibling `tests.rs` (`foo.rs` → `foo/tests.rs`) behind
  `#[cfg(test)] mod tests;`.
- A crate's integration tests build as a few binaries, `tests/<area>/main.rs` with one module per
  theme, sharing helpers through a `common` module rather than copying them.
- Assert the exact result, such as the full list of diagnostic codes with their positions, not a
  substring that something else would also satisfy.
- Name a test for the behaviour it pins, without ordinal or acceptance-id prefixes.
- Pin each behaviour in the crate that owns it. A CLI test covers what only the CLI does.
- A check that reads files rather than running code (README tables, spec citations, manifest
  settings) is a rule in `xtask/src/rules/`, not a test.
- Snapshots use insta. Read the `.snap.new` before accepting it.

## Pull requests

Target `canary`. The `pull-request` skill covers the title, the changelog fragment and the checks
to run first.
