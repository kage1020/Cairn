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

  // At EOF: synthesize the missing NEWLINE first (matches
  // cairn-lang-core::lex::scan_line_body, which emits a Newline token for a
  // final content line with no trailing line break before closing any
  // remaining indents), then emit trailing DEDENTs.
  if (lexer->eof(lexer)) {
    if (valid_symbols[NEWLINE]) {
      lexer->result_symbol = NEWLINE;
      return true;
    }
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
    if (valid_symbols[NEWLINE]) {
      lexer->result_symbol = NEWLINE;
      return true;
    }
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
