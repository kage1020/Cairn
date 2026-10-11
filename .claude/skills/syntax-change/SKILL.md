---
name: syntax-change
description: Change Cairn's surface syntax (a keyword, member, directive, statement or literal) across the spec, core's parser, the tree-sitter grammar and the editors.
---

# Change the syntax

The language has two parsers: core's, which is the reference, and the tree-sitter grammar editors
use. A syntax change lands in both, in one pull request, after the spec says what the syntax is.

1. **Spec.** Write the syntax in `spec/syntax`, and any new refusal in `spec/lint` (see the
   `diagnostic-code` skill).
2. **Core.** The lexer, parser and AST are `crates/cairn-lang-core/src/{lex,parse,ast}.rs`.
   Member keywords and their roles are `KNOWN_KEYWORDS` and `role_of` in
   `crates/cairn-lang-core/src/intent/keyword_table.rs`. Add parser tests for both the accepted
   and the refused forms.
3. **tree-sitter.** In `crates/cairn-lang-tree-sitter`:
   - Edit `grammar.js` (indentation tokens live in `src/scanner.c`), then run `pnpm install` once
     and `pnpm run generate`. Commit the regenerated `src/parser.c`, `src/grammar.json` and
     `src/node-types.json`; never edit them by hand.
   - Add corpus cases under `test/corpus/` and run `pnpm test`. A new `field(...)` also needs a
     `(field labels)` case that spells the label out, or `cargo xtask lint grammar-fields` fails.
   - Add the new forms, accepted and refused, to the fixtures in `tests/parser_parity.rs`, which
     feeds each one to both parsers.
   - Update `queries/highlights.scm` when the new token should be highlighted.
4. **Editors.** Add new keywords to `editors/vscode/syntaxes/cairn.tmLanguage.json` and to the
   completion lists in `crates/cairn-lang-lsp/src/completion.rs`.
5. **Examples.** A new file under `examples/` changes the `SHIPPED_EXAMPLES` count that the
   example sweeps in core, formats, cli and tree-sitter assert; bump all four.
6. Run the `verify` skill, plus `pnpm test` in the tree-sitter crate.
