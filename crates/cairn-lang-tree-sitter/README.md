# cairn-lang-tree-sitter

tree-sitter grammar for the Cairn build description language.

- Rust: `use cairn_lang_tree_sitter::LANGUAGE;` (also exports `HIGHLIGHTS_QUERY`,
  `LOCALS_QUERY`, `INJECTIONS_QUERY`).
- Node: `require('tree-sitter-cairn')`.
- WASM: attached to each GitHub Release as `tree-sitter-cairn.wasm`.

See [../../docs/superpowers/specs/2026-07-23-tree-sitter-cairn-design.md](../../docs/superpowers/specs/2026-07-23-tree-sitter-cairn-design.md)
for the design and [../../docs/superpowers/plans/2026-07-23-tree-sitter-cairn.md](../../docs/superpowers/plans/2026-07-23-tree-sitter-cairn.md)
for the implementation plan.

## Development

```bash
cd crates/cairn-lang-tree-sitter
pnpm install
pnpm run generate  # regenerate src/parser.c after grammar.js edits
pnpm test          # run tree-sitter corpus tests
cargo test -p cairn-lang-tree-sitter  # run Rust integration tests
```

Grammar edits without a matching `pnpm run generate` will fail the
`generate-check` job in `.github/workflows/tree-sitter.yml`.
