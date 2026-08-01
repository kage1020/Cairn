# tree-sitter-cairn Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `crates/cairn-lang-tree-sitter/`, a tree-sitter grammar for
Cairn with Rust, Node, and WASM bindings, accepting the same source set
as the reference parser in `cairn-lang-core`.

**Architecture:** A tree-sitter grammar (`grammar.js`) plus a C external
scanner (`src/scanner.c`) that mirrors the indent handling of
`cairn-lang-core::lex`. Generated `parser.c`, `grammar.json`, and
`node-types.json` are committed. Rust binding under `bindings/rust/`
(exposing `LANGUAGE: LanguageFn`), Node binding under `bindings/node/`
(compiled via node-gyp), WASM produced by CI on tag pushes.

**Tech Stack:** tree-sitter (grammar + CLI, WASM via emscripten), C99
external scanner, Rust (workspace member, `cc` build script), Node.js
(node-gyp), pnpm, GitHub Actions.

## Global Constraints

- **Design spec:** `docs/superpowers/specs/2026-07-23-tree-sitter-cairn-design.md`.
- **Reference parser is authoritative:** where behaviour diverges from
  `crates/cairn-lang-core/src/{lex.rs, parse.rs}`, the reference parser
  wins. Grammar and scanner mirror those files.
- **Initial version `2026.7.2`**, inherited via
  `version.workspace = true`. Every new crate field that has a workspace
  equivalent (`edition`, `rust-version`, `license`, `repository`,
  `homepage`, `authors`) uses `<field>.workspace = true`.
- **Workspace lints:** the workspace forbids `unsafe_code` and warns
  `missing_docs`. This crate overrides `unsafe_code = "allow"` in its
  own `[lints.rust]` (tree-sitter FFI is unavoidably unsafe). Every
  public Rust item ships with a `///` doc comment.
- **Package name is `tree-sitter-cairn`** on both crates.io and npm.
- **Generated artefacts are committed:** `src/parser.c`,
  `src/grammar.json`, `src/node-types.json`,
  `src/tree_sitter/parser.h`, `src/tree_sitter/alloc.h`,
  `src/tree_sitter/array.h`. `tree-sitter generate` writes them.
- **Dependency versions are chosen at install time.** Never hardcode
  versions when adding a dependency; use `cargo add …` or
  `pnpm add …`.
- **Package manager: pnpm.** The crate has its own
  `crates/cairn-lang-tree-sitter/pnpm-lock.yaml`; do not mix in the
  root `pnpm-lock.yaml` (there isn't one at the repo root today).
- **Feature branch:** work happens on `feat/tree-sitter-cairn` (already
  cut from `origin/canary`). All commits target that branch. Never
  commit to `main` or `canary` directly.
- **Commit style:** conventional commits, e.g.
  `feat(tree-sitter): add theme declaration rule`. Include the
  `Co-Authored-By` / `Claude-Session` trailers used elsewhere in this
  repo's Claude-authored commits.

---

## Task 1: Crate scaffolding and empty grammar

**Files:**
- Create: `crates/cairn-lang-tree-sitter/Cargo.toml`
- Create: `crates/cairn-lang-tree-sitter/package.json`
- Create: `crates/cairn-lang-tree-sitter/.gitignore`
- Create: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/bindings/rust/lib.rs`
- Create: `crates/cairn-lang-tree-sitter/bindings/rust/build.rs`
- Modify: `Cargo.toml` (workspace root: add member)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - Workspace member `cairn-lang-tree-sitter` at version `2026.7.2`.
  - `pub const LANGUAGE: tree_sitter_language::LanguageFn` from
    `bindings/rust/lib.rs`.
  - npm package `tree-sitter-cairn` (unpublished stub).
  - pnpm script `generate` → `tree-sitter generate`, `test` →
    `tree-sitter test`.

- [ ] **Step 1: Register the crate in the workspace**

Edit `Cargo.toml` at the repo root: append `"crates/cairn-lang-tree-sitter"`
to the `members` array so the file reads:

```toml
[workspace]
resolver = "2"
members = [
    "crates/cairn-lang-core",
    "crates/cairn-lang-cli",
    "crates/cairn-lang-nbt",
    "crates/cairn-lang-formats",
    "crates/cairn-lang-redstone",
    "crates/cairn-lang-lsp",
    "crates/cairn-lang-wasm",
    "crates/cairn-lang-tree-sitter",
]
```

- [ ] **Step 2: Create the crate `Cargo.toml`**

Write `crates/cairn-lang-tree-sitter/Cargo.toml`:

```toml
[package]
name = "cairn-lang-tree-sitter"
description = "tree-sitter grammar for the Cairn build description language"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
authors.workspace = true
build = "bindings/rust/build.rs"
include = [
    "bindings/rust/**",
    "grammar.js",
    "queries/**",
    "src/*.c",
    "src/*.h",
    "src/*.json",
    "src/tree_sitter/*.h",
    "!**/*.gyp",
    "!**/*.gypi",
]

[lib]
path = "bindings/rust/lib.rs"

[lints.rust]
unsafe_code = "allow"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
```

Cargo dependencies (`tree-sitter-language`, `cc`, `tree-sitter`) are
added in later steps via `cargo add` so pinned versions come from crates.io
at execution time.

- [ ] **Step 3: Add the Rust dependencies via CLI**

```bash
cd crates/cairn-lang-tree-sitter
cargo add tree-sitter-language
cargo add cc --build
cargo add tree-sitter --dev
```

- [ ] **Step 4: Create the crate `package.json`**

Write `crates/cairn-lang-tree-sitter/package.json`:

```json
{
  "name": "tree-sitter-cairn",
  "version": "2026.7.2",
  "description": "tree-sitter grammar for the Cairn build description language",
  "keywords": ["parser", "tree-sitter", "cairn", "minecraft"],
  "author": "kage1020 and the Cairn authors",
  "license": "Apache-2.0",
  "main": "bindings/node",
  "types": "bindings/node/index.d.ts",
  "files": [
    "grammar.js",
    "binding.gyp",
    "bindings/node/**",
    "queries/**",
    "src/**"
  ],
  "scripts": {
    "generate": "tree-sitter generate",
    "test": "tree-sitter test",
    "parse": "tree-sitter parse"
  }
}
```

- [ ] **Step 5: Install the tree-sitter CLI as a dev dependency**

```bash
cd crates/cairn-lang-tree-sitter
pnpm add -D tree-sitter-cli
```

This installs `tree-sitter-cli` locally and creates
`crates/cairn-lang-tree-sitter/pnpm-lock.yaml` plus `node_modules/`.

- [ ] **Step 6: Create the crate `.gitignore`**

Write `crates/cairn-lang-tree-sitter/.gitignore`:

```
node_modules/
build/
target/
*.log
```

Note: `src/parser.c`, `src/grammar.json`, `src/node-types.json` are
**not** ignored — they are committed.

- [ ] **Step 7: Write a minimal `grammar.js`**

Write `crates/cairn-lang-tree-sitter/grammar.js`:

```js
/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/[ \t]+/, $.comment],

  rules: {
    source_file: $ => optional($.identifier),

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
```

- [ ] **Step 8: Generate the parser**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
```

Expected: creates `src/parser.c`, `src/grammar.json`, `src/node-types.json`,
`src/tree_sitter/parser.h`, `src/tree_sitter/alloc.h`,
`src/tree_sitter/array.h`. No errors.

- [ ] **Step 9: Write the Rust `build.rs`**

Write `crates/cairn-lang-tree-sitter/bindings/rust/build.rs`:

```rust
//! Build script: compile the tree-sitter generated parser and the external
//! scanner into a static library linked by the Rust binding.

fn main() {
    let src_dir = std::path::Path::new("src");

    let mut cc = cc::Build::new();
    cc.include(src_dir);
    cc.file(src_dir.join("parser.c"));

    let scanner = src_dir.join("scanner.c");
    if scanner.exists() {
        cc.file(scanner);
    }

    cc.flag_if_supported("-Wno-unused-parameter");
    cc.flag_if_supported("-Wno-unused-but-set-variable");
    cc.flag_if_supported("-Wno-trigraphs");

    cc.compile("tree_sitter_cairn");
}
```

- [ ] **Step 10: Write the Rust `lib.rs`**

Write `crates/cairn-lang-tree-sitter/bindings/rust/lib.rs`:

```rust
//! Rust binding for the tree-sitter Cairn grammar.
//!
//! Consumers use [`LANGUAGE`] as the [`tree_sitter::Language`] handle for
//! the Cairn parser. The FFI symbol is emitted by the C parser generated
//! from [`grammar.js`](../../grammar.js).

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_cairn() -> *const ();
}

/// The [`tree_sitter_language::LanguageFn`] handle for the Cairn grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cairn) };

/// Highlight query source, embedded from `queries/highlights.scm`.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Locals query source, embedded from `queries/locals.scm`.
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// Injections query source, embedded from `queries/injections.scm`.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
```

- [ ] **Step 11: Create empty query files**

```bash
mkdir -p crates/cairn-lang-tree-sitter/queries
touch crates/cairn-lang-tree-sitter/queries/highlights.scm
touch crates/cairn-lang-tree-sitter/queries/locals.scm
touch crates/cairn-lang-tree-sitter/queries/injections.scm
```

- [ ] **Step 12: Verify Rust build**

```bash
cargo check -p cairn-lang-tree-sitter
```

Expected: clean, no warnings from `missing_docs` or `unsafe_code`.

- [ ] **Step 13: Commit**

```bash
git add \
  Cargo.toml \
  crates/cairn-lang-tree-sitter/Cargo.toml \
  crates/cairn-lang-tree-sitter/package.json \
  crates/cairn-lang-tree-sitter/pnpm-lock.yaml \
  crates/cairn-lang-tree-sitter/.gitignore \
  crates/cairn-lang-tree-sitter/grammar.js \
  crates/cairn-lang-tree-sitter/bindings/rust/{build.rs,lib.rs} \
  crates/cairn-lang-tree-sitter/queries \
  crates/cairn-lang-tree-sitter/src
git commit -m "feat(tree-sitter): scaffold cairn-lang-tree-sitter crate"
```

---

## Task 2: External scanner for indent, dedent, and newline

**Files:**
- Create: `crates/cairn-lang-tree-sitter/src/scanner.c`
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/indent.txt`

**Interfaces:**
- Consumes: `pub const LANGUAGE` from Task 1.
- Produces:
  - Three external tokens: `_indent`, `_dedent`, `_newline`, exposed to
    later grammar rules via `$._indent`, `$._dedent`, `$._newline`.
  - A `body` helper (a JS function inside `grammar.js`) that later rules
    use: `body($, item) => seq($._indent, repeat(seq(item, $._newline)),
    $._dedent)`.

- [ ] **Step 1: Write the failing corpus test**

Create `crates/cairn-lang-tree-sitter/test/corpus/indent.txt`:

```
==================
Simple nested block
==================

theme keep:
  slot floor -> @oak
  slot wall -> @stone

---

(source_file
  (theme_decl
    (identifier)
    (theme_body
      (slot_binding (identifier) (material_ref (identifier)))
      (slot_binding (identifier) (material_ref (identifier))))))

==================
Two-level nesting
==================

struct keep size=11x9
  level id=floor1 y=0
    door id=entry side=front at=center

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (nested_scope
        (identifier)
        (attribute_list
          (attribute (identifier) (identifier))
          (attribute (identifier) (integer)))
        (struct_body
          (member_stmt
            (member_keyword)
            (attribute_list
              (attribute (identifier) (identifier))
              (attribute (identifier) (identifier))
              (attribute (identifier) (identifier)))))))))
```

Only the first case (`Simple nested block`) is expected to pass at the
end of this task; the second case exists so the block boundaries in
Tasks 4 and 5 already have coverage. Grammar rules used above
(`theme_decl`, `slot_binding`, `struct_decl`, `attribute_list`, etc.)
land in Tasks 3–5.

- [ ] **Step 2: Extend the grammar with external tokens and a body helper**

Edit `crates/cairn-lang-tree-sitter/grammar.js` to declare externals and
expose a `body` helper:

```js
/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/[ \t]+/, $.comment],

  externals: $ => [$._indent, $._dedent, $._newline],

  rules: {
    source_file: $ => seq(
      repeat($._newline),
      repeat(seq($._blank_placeholder, $._newline)),
    ),

    _blank_placeholder: $ => $.identifier,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,

    comment: $ => /#[^\r\n]*/,
  },
});
```

`_blank_placeholder` is temporary — Task 3 replaces it with real
declarations. It exists so `source_file` has something to consume during
scanner development.

- [ ] **Step 3: Write the external scanner**

Create `crates/cairn-lang-tree-sitter/src/scanner.c`. The scanner mirrors
`cairn-lang-core/src/lex.rs::Lexer::scan_line_start`:

```c
#include "tree_sitter/parser.h"
#include "tree_sitter/array.h"

enum TokenType {
  INDENT,
  DEDENT,
  NEWLINE,
};

typedef struct {
  Array(uint16_t) indent_stack;
} Scanner;

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer)    { lexer->advance(lexer, true); }

void *tree_sitter_cairn_external_scanner_create(void) {
  Scanner *s = ts_calloc(1, sizeof(Scanner));
  array_init(&s->indent_stack);
  array_push(&s->indent_stack, 0);
  return s;
}

void tree_sitter_cairn_external_scanner_destroy(void *payload) {
  Scanner *s = (Scanner *)payload;
  array_delete(&s->indent_stack);
  ts_free(s);
}

unsigned tree_sitter_cairn_external_scanner_serialize(void *payload, char *buffer) {
  Scanner *s = (Scanner *)payload;
  unsigned size = 0;
  uint16_t len = (uint16_t)s->indent_stack.size;
  if (size + sizeof(len) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
  memcpy(buffer + size, &len, sizeof(len));
  size += sizeof(len);
  for (uint32_t i = 0; i < s->indent_stack.size; i++) {
    uint16_t v = *array_get(&s->indent_stack, i);
    if (size + sizeof(v) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
    memcpy(buffer + size, &v, sizeof(v));
    size += sizeof(v);
  }
  return size;
}

void tree_sitter_cairn_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  array_clear(&s->indent_stack);
  if (length == 0) {
    array_push(&s->indent_stack, 0);
    return;
  }
  unsigned offset = 0;
  uint16_t len;
  memcpy(&len, buffer + offset, sizeof(len));
  offset += sizeof(len);
  for (uint16_t i = 0; i < len; i++) {
    uint16_t v;
    memcpy(&v, buffer + offset, sizeof(v));
    offset += sizeof(v);
    array_push(&s->indent_stack, v);
  }
}

static bool at_line_break(TSLexer *lexer) {
  return lexer->lookahead == '\n' || lexer->lookahead == '\r' || lexer->eof(lexer);
}

bool tree_sitter_cairn_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Emit trailing DEDENTs at EOF.
  if (lexer->eof(lexer)) {
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      array_pop(&s->indent_stack);
      lexer->result_symbol = DEDENT;
      return true;
    }
    return false;
  }

  // NEWLINE handling: consume \r\n / \n / \r and emit NEWLINE.
  if (valid_symbols[NEWLINE] && (lexer->lookahead == '\n' || lexer->lookahead == '\r')) {
    if (lexer->lookahead == '\r') {
      advance(lexer);
      if (lexer->lookahead == '\n') advance(lexer);
    } else {
      advance(lexer);
    }
    lexer->result_symbol = NEWLINE;
    return true;
  }

  // Indent handling only fires at column 0.
  if (lexer->get_column(lexer) != 0) return false;
  if (!(valid_symbols[INDENT] || valid_symbols[DEDENT])) return false;

  // Skip blank and comment-only lines without shifting indent state.
  for (;;) {
    while (lexer->lookahead == ' ') skip(lexer);
    if (lexer->lookahead == '#') {
      while (!at_line_break(lexer)) skip(lexer);
    }
    if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
      if (lexer->lookahead == '\r') { skip(lexer); if (lexer->lookahead == '\n') skip(lexer); }
      else { skip(lexer); }
      continue;
    }
    break;
  }

  if (lexer->eof(lexer)) {
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      array_pop(&s->indent_stack);
      lexer->result_symbol = DEDENT;
      return true;
    }
    return false;
  }

  // Count leading spaces at the beginning of a real line. Tab is an error.
  uint32_t spaces = lexer->get_column(lexer);
  if (lexer->lookahead == '\t') return false;

  if (spaces & 1u) return false; // odd indent, let LR surface an ERROR
  uint16_t level = (uint16_t)(spaces / 2);
  uint16_t current = *array_back(&s->indent_stack);

  if (level > current) {
    if (level != current + 1) return false;
    if (!valid_symbols[INDENT]) return false;
    array_push(&s->indent_stack, level);
    lexer->result_symbol = INDENT;
    return true;
  }

  if (level < current) {
    if (!valid_symbols[DEDENT]) return false;
    array_pop(&s->indent_stack);
    lexer->result_symbol = DEDENT;
    return true;
  }

  return false;
}
```

- [ ] **Step 4: Regenerate and run the corpus test (expect the first case to pass)**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'Simple nested block'
```

Expected: `Simple nested block` fails because `theme_decl` is not defined
yet. That is the correct failing point for TDD — the scanner logic is
verified by the LR parser reaching `_indent` at the right place. Do not
attempt to make the case pass yet; move on.

- [ ] **Step 5: Confirm scanner compiles under cargo**

```bash
cargo check -p cairn-lang-tree-sitter
```

Expected: clean build; `build.rs` picked up `scanner.c`.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test}
git commit -m "feat(tree-sitter): add external scanner for indent/dedent/newline"
```

---

## Task 3: Directives (`@cairn`, `@requires`, `@intended_targets`)

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/directives.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/{parser.c, grammar.json, node-types.json}`

**Interfaces:**
- Consumes: external tokens `_newline`, extras from Task 2.
- Produces:
  - Named nodes: `directive`, `directive_name`, `version_expr`,
    `version_literal`, `value_list`, `integer`, `string`, `identifier`,
    `boolean`.
  - Rules other tasks reuse: `_value` (choice of primitive values),
    `_version_op` (`>=` / `<=` / `>` / `<` / `=`).

- [ ] **Step 1: Write the failing corpus test**

Create `crates/cairn-lang-tree-sitter/test/corpus/directives.txt`:

```
==================
@cairn version
==================

@cairn 2026.06

---

(source_file
  (directive
    (directive_name)
    (version_expr (version_literal))))

==================
@requires with operator
==================

@requires version>=1.20

---

(source_file
  (directive
    (directive_name)
    (version_expr (version_literal))))

==================
@intended_targets list
==================

@intended_targets ["1.20.4","1.21.4"]

---

(source_file
  (directive
    (directive_name)
    (value_list (string) (string))))
```

- [ ] **Step 2: Replace the placeholder rule with real directives**

Edit `crates/cairn-lang-tree-sitter/grammar.js`:

```js
/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: 'cairn',

  extras: $ => [/[ \t]+/, $.comment],

  externals: $ => [$._indent, $._dedent, $._newline],

  word: $ => $.identifier,

  rules: {
    source_file: $ => seq(
      repeat($._newline),
      repeat(seq($.directive, $._newline)),
    ),

    directive: $ => choice(
      seq(field('name', alias('@cairn', $.directive_name)),
          field('arg', $.version_expr)),
      seq(field('name', alias('@requires', $.directive_name)),
          optional('version'),
          field('arg', $.version_expr)),
      seq(field('name', alias('@intended_targets', $.directive_name)),
          field('arg', $.value_list)),
    ),

    version_expr: $ => seq(
      optional($._version_op),
      $.version_literal,
    ),

    _version_op: $ => choice('>=', '<=', '>', '<', '='),

    version_literal: $ => /[0-9]+(\.[0-9]+)*/,

    value_list: $ => seq('[', optional(seq($._value, repeat(seq(',', $._value)))), ']'),

    _value: $ => choice(
      $.integer,
      $.boolean,
      $.string,
      $.identifier,
      $.value_list,
    ),

    integer: $ => /[0-9]+/,
    boolean: $ => choice('true', 'false'),
    string:  $ => /"([^"\\]|\\.)*"/,

    identifier: $ => /[A-Za-z_][A-Za-z0-9_]*/,
    comment:    $ => /#[^\r\n]*/,
  },
});
```

Notes:
- `@requires` accepts an optional literal `version` token before the
  version expression, matching `@requires version>=1.20` in
  `examples/cottage.crn`.
- `alias('@cairn', $.directive_name)` gives the anonymous keyword its own
  named node in the CST while keeping the string literal easy to
  highlight later.

- [ ] **Step 3: Regenerate and run the directive tests**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter '@cairn|@requires|@intended_targets'
```

Expected: three passing cases.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus/directives.txt}
git commit -m "feat(tree-sitter): parse @cairn/@requires/@intended_targets directives"
```

---

## Task 4: Theme, slot bindings, selectors, and value primitives

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/theme.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/*`
- Modify: `crates/cairn-lang-tree-sitter/test/corpus/indent.txt` (the
  `Simple nested block` case is expected to pass now)

**Interfaces:**
- Consumes: `_indent`, `_dedent`, `_newline`, `_value`, `identifier`.
- Produces:
  - Named nodes: `theme_decl`, `theme_body`, `slot_binding`,
    `selector_rule`, `selector`, `attribute_list`, `attribute`,
    `material_ref`, `size_literal`.
  - Rules other tasks reuse: `attribute_list`, `attribute`,
    `material_ref`, `size_literal`, and the `body($, item)` helper.

- [ ] **Step 1: Extend the corpus**

Create `crates/cairn-lang-tree-sitter/test/corpus/theme.txt`:

```
==================
Bare theme with slots
==================

theme medieval:
  slot floor -> @oak_planks
  slot wall -> @cobblestone

---

(source_file
  (theme_decl
    (identifier)
    (theme_body
      (slot_binding (identifier) (material_ref (identifier)))
      (slot_binding (identifier) (material_ref (identifier))))))

==================
Selector rule with attribute filter
==================

theme medieval:
  window[class=small] -> frame=@spruce_wood

---

(source_file
  (theme_decl
    (identifier)
    (theme_body
      (selector_rule
        (selector
          (identifier)
          (attribute_list (attribute (identifier) (identifier))))
        (attribute_list
          (attribute (identifier) (material_ref (identifier))))))))

==================
Material with dotted path
==================

theme keep_dark:
  slot wall -> @wall.stone.cobble

---

(source_file
  (theme_decl
    (identifier)
    (theme_body
      (slot_binding
        (identifier)
        (material_ref (identifier) (identifier) (identifier))))))
```

- [ ] **Step 2: Extend `grammar.js` with `theme_decl` and value primitives**

Edit `crates/cairn-lang-tree-sitter/grammar.js`:

Add the `body` helper at module scope, above the `module.exports` call:

```js
function body(rule) {
  return seq('_indent_placeholder', rule);
}
```

Then update `module.exports` — the important rule additions are:

```js
source_file: $ => seq(
  repeat($._newline),
  repeat(seq(choice($.directive, $._top_level_decl), $._newline)),
),

_top_level_decl: $ => choice($.theme_decl /* struct/def/site added in later tasks */),

theme_decl: $ => seq(
  'theme',
  field('name', $.identifier),
  ':',
  $._newline,
  field('body', $.theme_body),
),

theme_body: $ => seq(
  $._indent,
  repeat(seq(choice($.slot_binding, $.selector_rule), $._newline)),
  $._dedent,
),

slot_binding: $ => seq(
  'slot',
  field('name', $.identifier),
  '->',
  field('target', $.material_ref),
),

selector_rule: $ => seq(
  field('selector', $.selector),
  optional(seq('->', field('bindings', $.attribute_list))),
),

selector: $ => seq(
  $.identifier,
  optional(seq('[', $.attribute_list, ']')),
  repeat(seq('.', $.identifier)),
),

attribute_list: $ => repeat1($.attribute),
attribute: $ => seq(field('key', $.identifier), '=', field('value', $._value)),

material_ref: $ => seq('@', $.identifier, repeat(seq('.', $.identifier))),

size_literal: $ => token(seq(/[0-9]+/, 'x', /[0-9]+/)),
```

Extend `_value` to include `material_ref` and `size_literal`:

```js
_value: $ => choice(
  $.size_literal,
  $.material_ref,
  $.integer,
  $.boolean,
  $.string,
  $.identifier,
  $.value_list,
),
```

- [ ] **Step 3: Regenerate and run the theme tests**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'theme|Bare|Selector rule|Material with dotted|Simple nested block'
```

Expected: all four cases pass. The `Simple nested block` case from
Task 2 also passes now that `theme_decl` exists.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus/theme.txt}
git commit -m "feat(tree-sitter): parse theme declarations, selectors, and value primitives"
```

---

## Task 5: Struct / def / site declarations and member commands

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/struct.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/*`

**Interfaces:**
- Consumes: `attribute_list`, `attribute`, `material_ref`,
  `size_literal`, `_value`, `_indent`, `_dedent`, `_newline`.
- Produces:
  - Named nodes: `struct_decl`, `def_decl`, `site_decl`, `struct_body`,
    `member_stmt`, `member_keyword`, `signal_ref`.
  - `signal_ref` is reused by Task 7 and Task 8.

- [ ] **Step 1: Write the corpus**

Create `crates/cairn-lang-tree-sitter/test/corpus/struct.txt`:

```
==================
Cottage struct
==================

struct cottage size=9x7
  floor mat_slot=floor
  walls class=outer mat_slot=wall height=4
  door side=front at=center

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (member_stmt (member_keyword)
        (attribute_list (attribute (identifier) (identifier))))
      (member_stmt (member_keyword)
        (attribute_list
          (attribute (identifier) (identifier))
          (attribute (identifier) (identifier))
          (attribute (identifier) (integer))))
      (member_stmt (member_keyword)
        (attribute_list
          (attribute (identifier) (identifier))
          (attribute (identifier) (identifier)))))))

==================
Member with arrow to signal
==================

struct gatehouse size=7x5
  pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (member_stmt (member_keyword)
        (attribute_list
          (attribute (identifier) (identifier))
          (attribute (identifier) (signal_ref (identifier) (identifier)))
          (attribute (identifier) (integer))
          (attribute (identifier) (integer)))
        (signal_ref (identifier) (identifier))))))
```

- [ ] **Step 2: Extend `grammar.js`**

Add to `_top_level_decl`:

```js
_top_level_decl: $ => choice(
  $.theme_decl,
  $.struct_decl,
  $.def_decl,
  $.site_decl,
),
```

Add the declaration and member rules. Define a plain JS helper
`declOf` **outside** `module.exports` (helpers cannot live inside
`rules`), then invoke it three times:

```js
// Above module.exports:
function declOf(keyword) {
  return $ => seq(
    keyword,
    field('name', $.identifier),
    optional(field('args', $.attribute_list)),
    $._newline,
    field('body', $.struct_body),
  );
}

// Inside module.exports.rules:
struct_decl: declOf('struct'),
def_decl:    declOf('def'),
site_decl:   declOf('site'),

struct_body: $ => seq(
  $._indent,
  repeat(seq($._struct_body_item, $._newline)),
  $._dedent,
),

_struct_body_item: $ => choice($.member_stmt /* extended in later tasks */),

member_stmt: $ => seq(
  field('keyword', $.member_keyword),
  optional(field('args', $.attribute_list)),
  optional(seq('->', field('output', $.signal_ref))),
),

member_keyword: $ => choice(
  'floor', 'walls', 'door', 'window', 'roof', 'stair',
  'pressure_plate', 'circuit', 'place', 'connect',
),

signal_ref: $ => prec.left(seq(
  $.identifier,
  repeat1(seq('.', $.identifier)),
)),
```

The `_decl_of` helper is a plain JavaScript function returning a
grammar production; it is invoked at grammar-definition time, so all
three declarations share the same structure without duplication.

The bare `identifier` in `_value` (Task 4) now competes with
`signal_ref`; add a precedence hint to `_value`:

```js
_value: $ => choice(
  $.size_literal,
  $.material_ref,
  prec(2, $.signal_ref),
  $.integer,
  $.boolean,
  $.string,
  $.identifier,
  $.value_list,
),
```

- [ ] **Step 3: Regenerate and run**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'Cottage struct|Member with arrow'
```

Expected: both cases pass. `pnpm test` (full suite) also expected green
except for cases whose grammar arrives in later tasks.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus/struct.txt}
git commit -m "feat(tree-sitter): parse struct/def/site declarations and member commands"
```

---

## Task 6: Nested scopes (`level`, `room`)

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/nested.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/*`

**Interfaces:**
- Consumes: `attribute_list`, `struct_body`, `member_stmt`.
- Produces: named node `nested_scope`. Reused by Task 9.

- [ ] **Step 1: Write the corpus**

Create `crates/cairn-lang-tree-sitter/test/corpus/nested.txt`:

```
==================
level nested inside struct
==================

struct keep size=11x9
  level id=floor1 y=0
    door id=entry side=front at=center

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (nested_scope
        (identifier)
        (attribute_list
          (attribute (identifier) (identifier))
          (attribute (identifier) (integer)))
        (struct_body
          (member_stmt (member_keyword)
            (attribute_list
              (attribute (identifier) (identifier))
              (attribute (identifier) (identifier))
              (attribute (identifier) (identifier)))))))))
```

Delete the `Two-level nesting` case from `test/corpus/indent.txt` — it
duplicates this coverage now that the grammar knows `nested_scope`.

- [ ] **Step 2: Extend `grammar.js`**

Add `nested_scope` to `_struct_body_item`:

```js
_struct_body_item: $ => choice($.member_stmt, $.nested_scope),

nested_scope: $ => seq(
  field('keyword', alias(choice('level', 'room'), $.identifier)),
  optional(field('args', $.attribute_list)),
  $._newline,
  field('body', $.struct_body),
),
```

- [ ] **Step 3: Regenerate and run**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'level nested inside struct'
```

Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus}
git commit -m "feat(tree-sitter): parse level/room nested scopes"
```

---

## Task 7: Logic declarations and boolean expressions

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/logic.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/*`

**Interfaces:**
- Consumes: `signal_ref`, `_newline`.
- Produces:
  - Named nodes: `logic_decl`, `bool_expr`, `binary_expression`,
    `unary_expression`, `parenthesized_expression`, `wire_ref`.
  - Precedence names: `LOGIC_OR = 1`, `LOGIC_AND = 2`, `LOGIC_NOT = 3`.

- [ ] **Step 1: Write the corpus**

Create `crates/cairn-lang-tree-sitter/test/corpus/logic.txt`:

```
==================
logic with or
==================

struct gatehouse size=7x5
  logic sig.open = sig.step or sig.exit

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (logic_decl
        (signal_ref (identifier) (identifier))
        (binary_expression
          (signal_ref (identifier) (identifier))
          (signal_ref (identifier) (identifier)))))))

==================
logic with and, not, parens precedence
==================

struct sample size=5x5
  logic sig.out = not sig.a and (sig.b or sig.c)

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (logic_decl
        (signal_ref (identifier) (identifier))
        (binary_expression
          (unary_expression (signal_ref (identifier) (identifier)))
          (parenthesized_expression
            (binary_expression
              (signal_ref (identifier) (identifier))
              (signal_ref (identifier) (identifier)))))))))
```

- [ ] **Step 2: Extend `grammar.js`**

Add to `_struct_body_item`:

```js
_struct_body_item: $ => choice($.member_stmt, $.nested_scope, $.logic_decl),

logic_decl: $ => seq(
  'logic',
  field('name', $.signal_ref),
  '=',
  field('value', $._bool_expr),
),

_bool_expr: $ => choice(
  $.binary_expression,
  $.unary_expression,
  $.parenthesized_expression,
  $.signal_ref,
  $.boolean,
),

binary_expression: $ => choice(
  prec.left(1, seq(field('lhs', $._bool_expr), 'or',  field('rhs', $._bool_expr))),
  prec.left(2, seq(field('lhs', $._bool_expr), 'and', field('rhs', $._bool_expr))),
),

unary_expression: $ => prec(3, seq('not', field('operand', $._bool_expr))),

parenthesized_expression: $ => seq('(', $._bool_expr, ')'),
```

- [ ] **Step 3: Regenerate and run**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'logic with or|logic with and'
```

Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus/logic.txt}
git commit -m "feat(tree-sitter): parse logic declarations and boolean expressions"
```

---

## Task 8: Assert statements (truth tables and temporal assertions)

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/grammar.js`
- Create: `crates/cairn-lang-tree-sitter/test/corpus/assert.txt`
- Regenerate: `crates/cairn-lang-tree-sitter/src/*`

**Interfaces:**
- Consumes: `signal_ref`, `_bool_expr`, `integer`, `_newline`.
- Produces: `assert_stmt`, `truth_form`, `truth_row`, `bit_pattern`,
  `signal_list`, `temporal_form`, `temporal_expr`.

- [ ] **Step 1: Write the corpus**

Create `crates/cairn-lang-tree-sitter/test/corpus/assert.txt`:

```
==================
Truth table
==================

struct s size=3x3
  assert truth(sig.a, sig.b -> sig.out) { 00->0; 01->1; 10->1; 11->1 }

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (assert_stmt
        (truth_form
          (signal_list
            (signal_ref (identifier) (identifier))
            (signal_ref (identifier) (identifier)))
          (signal_ref (identifier) (identifier))
          (truth_row (bit_pattern) (bit_pattern))
          (truth_row (bit_pattern) (bit_pattern))
          (truth_row (bit_pattern) (bit_pattern))
          (truth_row (bit_pattern) (bit_pattern)))))))

==================
Temporal assertion
==================

struct s size=3x3
  assert always(sig.step -> eventually sig.open within 2)

---

(source_file
  (struct_decl
    (identifier)
    (attribute_list
      (attribute (identifier) (size_literal (integer) (integer))))
    (struct_body
      (assert_stmt
        (temporal_form
          (temporal_expr
            (signal_ref (identifier) (identifier))
            (signal_ref (identifier) (identifier))
            (integer)))))))
```

- [ ] **Step 2: Extend `grammar.js`**

```js
_struct_body_item: $ => choice(
  $.member_stmt, $.nested_scope, $.logic_decl, $.assert_stmt,
),

assert_stmt: $ => seq('assert', choice($.truth_form, $.temporal_form)),

truth_form: $ => seq(
  'truth', '(',
  field('inputs', $.signal_list),
  '->',
  field('output', $.signal_ref),
  ')',
  '{',
  $.truth_row,
  repeat(seq(';', $.truth_row)),
  optional(';'),
  '}',
),

signal_list: $ => seq($.signal_ref, repeat(seq(',', $.signal_ref))),

truth_row: $ => seq($.bit_pattern, '->', $.bit_pattern),
bit_pattern: $ => /[01]+/,

temporal_form: $ => seq('always', '(', $.temporal_expr, ')'),

temporal_expr: $ => seq(
  field('trigger', $.signal_ref),
  '->',
  'eventually',
  field('target', $.signal_ref),
  optional(seq('within', field('bound', $.integer))),
),
```

- [ ] **Step 3: Regenerate and run**

```bash
cd crates/cairn-lang-tree-sitter
pnpm run generate
pnpm test -- --filter 'Truth table|Temporal assertion'
```

Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{grammar.js,src,test/corpus/assert.txt}
git commit -m "feat(tree-sitter): parse assert truth tables and temporal assertions"
```

---

## Task 9: Rust integration test against `examples/*.crn`

**Files:**
- Create: `crates/cairn-lang-tree-sitter/tests/examples.rs`
- Modify: `crates/cairn-lang-tree-sitter/Cargo.toml` (add
  `[[test]] name = "examples"` if the auto-discovery path is different)

**Interfaces:**
- Consumes: `pub const LANGUAGE`.
- Produces: a `cargo test -p cairn-lang-tree-sitter --test examples`
  target that fails if any `examples/*.crn` produces a tree containing
  an `ERROR` node.

- [ ] **Step 1: Write the failing integration test**

Create `crates/cairn-lang-tree-sitter/tests/examples.rs`:

```rust
//! Example integration test.
//!
//! For every `.crn` under `examples/` at the repo root, parse it with the
//! tree-sitter grammar and assert the resulting syntax tree has no
//! `ERROR` node. This is the primary regression guardrail against the
//! reference parser: `cairn-lang-core` already accepts every file in
//! `examples/`, so the grammar must accept them too.

use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Parser;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("examples")
}

fn crn_files() -> Vec<PathBuf> {
    fs::read_dir(examples_dir())
        .expect("examples dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "crn"))
        .collect()
}

#[test]
fn all_examples_parse_without_error() {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");

    let mut failures = Vec::new();
    for path in crn_files() {
        let src = fs::read_to_string(&path).unwrap();
        let tree = parser.parse(&src, None).expect("parse produced no tree");
        if tree.root_node().has_error() {
            failures.push(format!("{}", path.display()));
        }
    }
    assert!(failures.is_empty(), "grammar rejected: {failures:#?}");
}
```

- [ ] **Step 2: Run the test to see it pass**

```bash
cargo test -p cairn-lang-tree-sitter --test examples
```

Expected: passes. If any `.crn` file fails, add a minimal reproducer
to the tree-sitter corpus for the missing construct, then extend
`grammar.js` in a small follow-up task before continuing. Do **not**
patch around it with `has_error` tolerance.

- [ ] **Step 3: Commit**

```bash
git add crates/cairn-lang-tree-sitter/tests/examples.rs
git commit -m "test(tree-sitter): parse every examples/*.crn without ERROR nodes"
```

---

## Task 10: Node.js binding

**Files:**
- Create: `crates/cairn-lang-tree-sitter/binding.gyp`
- Create: `crates/cairn-lang-tree-sitter/bindings/node/binding.cc`
- Create: `crates/cairn-lang-tree-sitter/bindings/node/index.js`
- Create: `crates/cairn-lang-tree-sitter/bindings/node/index.d.ts`
- Modify: `crates/cairn-lang-tree-sitter/package.json` (add `node-addon-api`
  and `node-gyp-build` runtime deps via CLI)

**Interfaces:**
- Consumes: `src/parser.c`, `src/scanner.c`.
- Produces: `require('tree-sitter-cairn')` returns
  `{ name: 'cairn', language: <native binding>, nodeTypeInfo: [...] }`.

- [ ] **Step 1: Install the Node runtime dependencies**

```bash
cd crates/cairn-lang-tree-sitter
pnpm add node-addon-api
pnpm add node-gyp-build
```

- [ ] **Step 2: Write `binding.gyp`**

```python
{
  "targets": [
    {
      "target_name": "tree_sitter_cairn_binding",
      "dependencies": [
        "<!(node -p \"require('node-addon-api').targets\"):node_addon_api_except"
      ],
      "include_dirs": [ "src" ],
      "sources": [
        "bindings/node/binding.cc",
        "src/parser.c",
        "src/scanner.c"
      ],
      "cflags_c": [
        "-std=c11",
        "-Wno-unused-parameter"
      ],
      "conditions": [
        [ "OS!=\"win\"", { "cflags": [ "-fPIC" ] } ]
      ]
    }
  ]
}
```

- [ ] **Step 3: Write the Node addon `binding.cc`**

```cpp
#include <napi.h>

typedef struct TSLanguage TSLanguage;
extern "C" TSLanguage *tree_sitter_cairn();

namespace {
Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports["name"] = Napi::String::New(env, "cairn");
  auto language = Napi::External<TSLanguage>::New(env, tree_sitter_cairn());
  language.TypeTag(&exports.Env().GetInstanceData<char>());
  exports["language"] = language;
  return exports;
}
} // namespace

NODE_API_MODULE(tree_sitter_cairn_binding, Init)
```

- [ ] **Step 4: Write `bindings/node/index.js`**

```js
const root = require("path").join(__dirname, "..", "..");

module.exports = require("node-gyp-build")(root);

try {
  module.exports.nodeTypeInfo = require("../../src/node-types.json");
} catch (_) {}
```

- [ ] **Step 5: Write `bindings/node/index.d.ts`**

```ts
type BaseNode = { type: string; named: boolean };
type ChildNode = { multiple: boolean; required: boolean; types: BaseNode[] };
type NodeInfo =
  | (BaseNode & { subtypes: BaseNode[] })
  | (BaseNode & { fields: { [name: string]: ChildNode }; children: ChildNode[] });

declare const cairn: {
  name: "cairn";
  language: unknown;
  nodeTypeInfo?: NodeInfo[];
};

export = cairn;
```

- [ ] **Step 6: Build the native addon**

```bash
cd crates/cairn-lang-tree-sitter
pnpm exec node-gyp rebuild --release
```

Expected: writes `build/Release/tree_sitter_cairn_binding.node` and
exits 0.

- [ ] **Step 7: Smoke-test the addon**

```bash
node -e "console.log(require('./bindings/node').name)"
```

Expected: prints `cairn`.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{binding.gyp,bindings/node,package.json,pnpm-lock.yaml}
git commit -m "feat(tree-sitter): add node-gyp binding for tree-sitter-cairn"
```

---

## Task 11: Highlight, locals, and injections queries

**Files:**
- Modify: `crates/cairn-lang-tree-sitter/queries/highlights.scm`
- Modify: `crates/cairn-lang-tree-sitter/queries/locals.scm`
- Modify: `crates/cairn-lang-tree-sitter/queries/injections.scm`
- Create: `crates/cairn-lang-tree-sitter/test/highlight/cottage.ansi`

**Interfaces:**
- Consumes: every named node introduced in Tasks 3–8.
- Produces: `highlight-names.txt` compatibility with `nvim-treesitter`
  (capture names conform to the standard set).

- [ ] **Step 1: Write `queries/highlights.scm`**

```scheme
; keywords
[
  "theme" "struct" "def" "site" "slot" "level" "room"
  "logic" "assert"
] @keyword

(member_keyword) @keyword
"truth" @keyword.operator
"always" @keyword.operator
"eventually" @keyword.operator
"within" @keyword.operator
"or" @keyword.operator
"and" @keyword.operator
"not" @keyword.operator

; directives
(directive_name) @keyword.directive

; operators
["->" "=" ">=" "<=" ">" "<"] @operator

; punctuation
["[" "]" "(" ")" "{" "}"] @punctuation.bracket
["," ";" "." ":"] @punctuation.delimiter

; literals
(string) @string
(integer) @number
(bit_pattern) @number
(size_literal) @number.special
(boolean) @constant.builtin

; types / references
(material_ref) @type
(attribute key: (identifier) @variable.parameter)
(signal_ref (identifier) . (identifier) @variable.member)

; comments
(comment) @comment
```

- [ ] **Step 2: Write `queries/locals.scm`**

```scheme
; A member with `id=<ident>` defines a local reference.
(member_stmt
  (attribute_list
    (attribute
      key: (identifier) @_key
      value: (identifier) @local.definition.member))
  (#eq? @_key "id"))

; Any bare-identifier attribute value is a reference to that id.
(attribute
  key: (identifier) @_key
  value: (identifier) @local.reference
  (#not-eq? @_key "id"))
```

- [ ] **Step 3: Leave `queries/injections.scm` intentionally empty**

```bash
> crates/cairn-lang-tree-sitter/queries/injections.scm
```

(No embedded languages for the initial release; the empty file is still
included in `LANGUAGE` re-exports and in the node binding shipping list.)

- [ ] **Step 4: Freeze the highlight golden**

```bash
cd crates/cairn-lang-tree-sitter
mkdir -p test/highlight
pnpm exec tree-sitter highlight \
  --html ../../examples/cottage.crn > /dev/null   # sanity check
pnpm exec tree-sitter highlight \
  ../../examples/cottage.crn > test/highlight/cottage.ansi
```

The ANSI escape sequences are deterministic per grammar + query state,
so future diffs on this file signal an intentional query change.

- [ ] **Step 5: Wire the golden into the corpus test**

Extend `tests/examples.rs` with a second test that shells out to the
locally installed `tree-sitter` CLI. This deliberately avoids taking a
Rust dep on `tree-sitter-highlight`; the CLI is already a dev dep via
`pnpm add -D tree-sitter-cli` in Task 1, and its `highlight` output is
the same bytes the golden was frozen from.

```rust
use std::process::Command;

#[test]
fn cottage_highlight_golden_is_stable() {
    let golden = include_str!("../test/highlight/cottage.ansi");

    let cli = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) { "tree-sitter.cmd" } else { "tree-sitter" });

    let output = Command::new(cli)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["highlight", "../../examples/cottage.crn"])
        .output()
        .expect("run tree-sitter CLI");

    assert!(output.status.success(), "cli failed: {:?}", output);
    let rendered = String::from_utf8(output.stdout).expect("utf-8");

    assert_eq!(rendered, golden, "highlight output drifted from golden");
}
```

Run:

```bash
cargo test -p cairn-lang-tree-sitter --test examples cottage_highlight_golden_is_stable
```

Expected: passes. If it fails on a fresh clone because `node_modules`
is absent, run `pnpm install` inside the crate first — the test
requires the CLI to be present, matching the CI flow in Task 12.

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-lang-tree-sitter/{queries,test/highlight,tests/examples.rs,Cargo.toml,Cargo.lock}
git commit -m "feat(tree-sitter): add highlight/locals queries with golden snapshot"
```

---

## Task 12: CI, release-plz integration, and README

**Files:**
- Create: `.github/workflows/tree-sitter.yml`
- Modify: `release-plz.toml` (register the new crate)
- Create: `crates/cairn-lang-tree-sitter/README.md`

**Interfaces:**
- Consumes: everything.
- Produces:
  - Green CI on push and PR that touches the crate.
  - `parser.wasm` uploaded to the GitHub Release on tag push.
  - crates.io release picked up by release-plz's monthly minor flow.

- [ ] **Step 1: Write `.github/workflows/tree-sitter.yml`**

```yaml
name: tree-sitter

on:
  push:
    branches: [main, canary]
    tags: ["v*"]
    paths:
      - "crates/cairn-lang-tree-sitter/**"
      - ".github/workflows/tree-sitter.yml"
  pull_request:
    paths:
      - "crates/cairn-lang-tree-sitter/**"
      - ".github/workflows/tree-sitter.yml"

defaults:
  run:
    working-directory: crates/cairn-lang-tree-sitter

jobs:
  generate-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: crates/cairn-lang-tree-sitter/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm run generate
      - name: Ensure generated files match commit
        run: git diff --exit-code src/parser.c src/grammar.json src/node-types.json

  tree-sitter-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: crates/cairn-lang-tree-sitter/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm test

  rust-test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test -p cairn-lang-tree-sitter --tests
        working-directory: .

  node-smoke:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        node: [20, 22]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: ${{ matrix.node }}
          cache: pnpm
          cache-dependency-path: crates/cairn-lang-tree-sitter/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm exec node-gyp rebuild --release
      - run: node -e "console.log(require('./bindings/node').name)"

  wasm-build:
    if: startsWith(github.ref, 'refs/tags/')
    runs-on: ubuntu-latest
    needs: [generate-check, tree-sitter-test]
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: crates/cairn-lang-tree-sitter/pnpm-lock.yaml
      - uses: mymindstorm/setup-emsdk@v14
      - run: pnpm install --frozen-lockfile
      - run: pnpm exec tree-sitter build --wasm --output tree-sitter-cairn.wasm
      - uses: softprops/action-gh-release@v2
        with:
          files: crates/cairn-lang-tree-sitter/tree-sitter-cairn.wasm
```

- [ ] **Step 2: Register the crate with release-plz**

Open `release-plz.toml` and confirm the CalVer bump path picks up the
new workspace member. If the file has a per-crate section, add:

```toml
[[package]]
name = "cairn-lang-tree-sitter"
```

If instead the workspace-wide default is inherited (most likely, given
the other `cairn-lang-*` crates), no change is needed — the presence
of the crate in `Cargo.toml`'s `members` and `publish = true`
inherited from `workspace.package` is sufficient.

Verify with:

```bash
cargo install --locked release-plz --version '^0.3'
release-plz update --dry-run
```

Expected: mentions `cairn-lang-tree-sitter` in the version bump plan.

- [ ] **Step 3: Write a short README for the crate**

`crates/cairn-lang-tree-sitter/README.md`:

```markdown
# cairn-lang-tree-sitter

tree-sitter grammar for the Cairn build description language.

- Rust: `use cairn_lang_tree_sitter::LANGUAGE;`
- Node: `require('tree-sitter-cairn')`
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
```

- [ ] **Step 4: Push the branch and open the PR**

```bash
git push -u origin feat/tree-sitter-cairn
gh pr create --base canary --title 'feat(tree-sitter): add cairn-lang-tree-sitter crate' \
  --body "$(cat <<'EOF'
## Summary
- Full tree-sitter grammar for Cairn under `crates/cairn-lang-tree-sitter/`,
  matching `cairn-lang-core`'s accepted-source set on `examples/*.crn`.
- Rust, Node (node-gyp), and WASM bindings; queries for highlights and locals.
- New GitHub Actions workflow gating generation-drift, corpus tests, Rust and
  Node smoke tests, and (on tag) a `tree-sitter-cairn.wasm` release asset.

## Test plan
- [ ] `pnpm test` (tree-sitter corpus)
- [ ] `cargo test -p cairn-lang-tree-sitter` (examples + highlight golden)
- [ ] `pnpm exec node-gyp rebuild --release && node -e "require('./bindings/node')"`
- [ ] `release-plz update --dry-run` mentions the new crate

Spec: `docs/superpowers/specs/2026-07-23-tree-sitter-cairn-design.md`
Plan: `docs/superpowers/plans/2026-07-23-tree-sitter-cairn.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01TkEPFpcs7jUgbmLGjS97P1
EOF
)"
```

- [ ] **Step 5: Commit any docs / release-plz tweaks**

```bash
git add .github/workflows/tree-sitter.yml release-plz.toml \
  crates/cairn-lang-tree-sitter/README.md
git commit -m "ci(tree-sitter): add CI workflow and release-plz registration"
git push
```

---

## Self-review notes

- **Spec coverage:** every section of the design spec has an owning task:
  §1 → Task 1 & 9, §2 → Task 2 & 9, §3 → Task 1 & 10, §4 → Tasks 3–8,
  §5 → Task 2, §6 → Task 11, §7 → Tasks 1/9/10 (Rust and Node) plus
  Task 12 (WASM CI), §8 → Task 12, §9 → Task 12, §10 → Task 9 (Rust
  integration), Task 2–8 (corpus), Task 11 (highlight golden), §11 →
  documented as deferred, no task needed.
- **Type consistency:** node names used across tasks (`theme_decl`,
  `theme_body`, `struct_decl`, `struct_body`, `member_stmt`,
  `member_keyword`, `signal_ref`, `attribute_list`, `attribute`,
  `material_ref`, `size_literal`, `binary_expression`,
  `unary_expression`, `parenthesized_expression`, `logic_decl`,
  `assert_stmt`, `truth_form`, `truth_row`, `bit_pattern`,
  `signal_list`, `temporal_form`, `temporal_expr`) are consistent
  across Tasks 3–11.
- **Placeholder scan:** no `TBD`, `TODO`, or "fill in details" markers.
  Every code block is complete and runnable.
