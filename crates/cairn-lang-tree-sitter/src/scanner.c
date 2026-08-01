#include "tree_sitter/parser.h"
#include "tree_sitter/array.h"

enum TokenType {
  INDENT,
  DEDENT,
  NEWLINE,
  SIZE_X,
};

// Upper bound on `indent_stack`'s length that `serialize()` below can
// always fit in `TREE_SITTER_SERIALIZATION_BUFFER_SIZE`: the 2-byte
// length prefix, plus one `uint16_t` per stack entry, plus the 1-byte
// `eof_newline_used` flag. Enforced on push (see the INDENT branch in
// `scan()`) rather than tolerated in `serialize()`: `serialize()` returning
// a short count on overflow is silently reinterpreted by tree-sitter as
// "no state" (see `deserialize()`'s `length == 0` path), corrupting the
// indent stack rather than surfacing an error. Cairn's 2-space-per-level
// indent convention means reaching this cap in a legitimate file would
// require on the order of a thousand nesting levels (2000+ leading spaces
// on one line) — evidence of a bug or pathological/adversarial input, not
// a real program, so an INDENT that would exceed the cap is refused
// outright (surfacing as an ERROR node) rather than risking silent
// corruption later at serialize() time.
#define MAX_INDENT_DEPTH \
  ((TREE_SITTER_SERIALIZATION_BUFFER_SIZE - sizeof(uint16_t) - sizeof(uint8_t)) / sizeof(uint16_t))

typedef struct {
  Array(uint16_t) indent_stack;
  // Set once an end-of-file NEWLINE has been synthesized since the last
  // EOF-path DEDENT. Grammar sites that need "one or more" newlines
  // (`repeat1($._newline)`, used to swallow blank/comment lines between
  // same-level items) would otherwise re-request NEWLINE forever at EOF:
  // the synthesized token is zero-width (no `advance()` call), so nothing
  // ever bounds the repeat. Only the DEDENTs emitted on the EOF path (see
  // the two `lexer->eof(lexer)` blocks in `scan()`) reset this flag — the
  // non-EOF DEDENT branch (ordinary mid-file dedent, driven by a real
  // decrease in leading-space count) leaves it untouched, since it isn't
  // preceded by a synthesized EOF newline in the first place. Each EOF
  // DEDENT closes out one enclosing body before an *outer* site can ask
  // for its own newline, so resetting there lets every nesting level still
  // get its own one-shot synthesized newline while capping any single
  // `repeat1` to exactly one synthetic match.
  bool eof_newline_used;
} Scanner;

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer)    { lexer->advance(lexer, true); }

void *tree_sitter_cairn_external_scanner_create(void) {
  Scanner *s = ts_calloc(1, sizeof(Scanner));
  array_init(&s->indent_stack);
  array_push(&s->indent_stack, 0);
  s->eof_newline_used = false;
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
  if (size + sizeof(uint8_t) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
  uint8_t eof_flag = s->eof_newline_used ? 1 : 0;
  memcpy(buffer + size, &eof_flag, sizeof(eof_flag));
  size += sizeof(eof_flag);
  return size;
}

// Resets `s` to the same single-sentinel-level state `create()` starts
// with. Shared by every early-return path in `deserialize()` below: a
// too-short `length` (empty buffer, or one that claims more entries than
// it actually holds — truncated/corrupt input, however that occurred)
// gets the same treatment as "no prior state", rather than reading past
// `buffer + length`.
static void reset_to_sentinel(Scanner *s) {
  array_clear(&s->indent_stack);
  array_push(&s->indent_stack, 0);
  s->eof_newline_used = false;
}

void tree_sitter_cairn_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length == 0) {
    reset_to_sentinel(s);
    return;
  }
  array_clear(&s->indent_stack);
  s->eof_newline_used = false;
  unsigned offset = 0;
  uint16_t len;
  if (offset + sizeof(len) > length) {
    reset_to_sentinel(s);
    return;
  }
  memcpy(&len, buffer + offset, sizeof(len));
  offset += sizeof(len);
  for (uint16_t i = 0; i < len; i++) {
    uint16_t v;
    if (offset + sizeof(v) > length) {
      reset_to_sentinel(s);
      return;
    }
    memcpy(&v, buffer + offset, sizeof(v));
    offset += sizeof(v);
    array_push(&s->indent_stack, v);
  }
  // `len == 0` is itself a corrupt/impossible state — `serialize()` always
  // writes at least the base `0` level — but is representable within an
  // otherwise-valid-length buffer, so it isn't caught by the bounds checks
  // above. Restore the invariant every other function in this file relies
  // on (`array_back(&s->indent_stack)` is always safe to call) rather than
  // leaving an empty stack.
  if (s->indent_stack.size == 0) {
    array_push(&s->indent_stack, 0);
  }
  if (offset + sizeof(uint8_t) <= length) {
    uint8_t eof_flag;
    memcpy(&eof_flag, buffer + offset, sizeof(eof_flag));
    offset += sizeof(eof_flag);
    s->eof_newline_used = eof_flag != 0;
  }
}

static bool at_line_break(TSLexer *lexer) {
  return lexer->lookahead == '\n' || lexer->lookahead == '\r' || lexer->eof(lexer);
}

bool tree_sitter_cairn_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Size-literal separator `x` in e.g. `9x7`. Handled by the external
  // scanner (rather than a plain grammar-level `token.immediate('x')`)
  // because tree-sitter's keyword-extraction machinery, triggered by
  // `word: $.identifier`, greedily scans any word-shaped run of characters
  // starting at a letter (here, `x7`) before checking it against the
  // keyword table; since `x7` is not a registered keyword, extraction falls
  // back to a generic `identifier` token covering `x7`, so a plain literal
  // 'x' token can never win the match. The external scanner runs before
  // that machinery and is only ever consulted where the grammar expects
  // this separator, so it can commit to the single `x` character directly.
  // It is checked before any extras (spaces) are skipped, which is exactly
  // what enforces immediate adjacency: `9 x 7` must not parse as one size
  // literal.
  if (valid_symbols[SIZE_X] && lexer->lookahead == 'x') {
    advance(lexer);
    lexer->result_symbol = SIZE_X;
    return true;
  }

  // At EOF: synthesize the missing NEWLINE first (matches
  // cairn-lang-core::lex::scan_line_body, which emits a Newline token for a
  // final content line with no trailing line break before closing any
  // remaining indents), then emit trailing DEDENTs.
  //
  // The synthesized NEWLINE is zero-width (no `advance()` call), so it is
  // gated by `eof_newline_used`: a `repeat1($._newline)` site (blank/
  // comment lines between same-level items) would otherwise be offered an
  // endless run of these and never terminate. DEDENT resets the flag, so
  // each enclosing body still gets its own one-shot synthesized newline.
  if (lexer->eof(lexer)) {
    if (valid_symbols[NEWLINE] && !s->eof_newline_used) {
      s->eof_newline_used = true;
      lexer->result_symbol = NEWLINE;
      return true;
    }
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      array_pop(&s->indent_stack);
      s->eof_newline_used = false;
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
    if (valid_symbols[NEWLINE] && !s->eof_newline_used) {
      s->eof_newline_used = true;
      lexer->result_symbol = NEWLINE;
      return true;
    }
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      array_pop(&s->indent_stack);
      s->eof_newline_used = false;
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
    // See `MAX_INDENT_DEPTH`'s comment: refuse rather than push past what
    // `serialize()` can represent.
    if (s->indent_stack.size >= MAX_INDENT_DEPTH) return false;
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
