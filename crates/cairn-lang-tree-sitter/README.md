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

## Corpus conventions

`tree-sitter test` strips field labels from the parsed tree whenever the expected
S-expression has none, so most cases in `test/corpus/` stay terse and pin node
shape only. Dropping a `field(...)` from `grammar.js` leaves that shape intact,
which is why every rule that declares fields also has exactly one case whose
expected tree spells the labels out. Those cases carry `(field labels)` in their
name — for example `def declaration (field labels)`:

```
(source_file
  (def_decl
    name: (identifier)
    args: (attribute_list
      (attribute key: (identifier) value: (size_literal (integer) (integer))))
    body: (struct_body
      ...)))
```

Labelling is all-or-nothing per case: one label anywhere in the expected tree
makes the whole tree compare with labels, so a partially labelled case fails.
When editing a `(field labels)` case, keep every label in place; when adding a
`field(...)` to `grammar.js`, label one representative case for the new rule.
