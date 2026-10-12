#include "tree_sitter/parser.h"
#include "tree_sitter/array.h"

enum TokenType {
  INDENT,
  DEDENT,
  NEWLINE,
  FILE_START,
  SIZE_X,
  LINE_START,
  FILE_END,
  BIT_PATTERN,
  BIT,
  ERROR_SENTINEL,
};

// The deepest `indent_stack` that `serialize()` fits in its buffer beside the other fields. An
// INDENT past it is refused, since a short `serialize()` would read back as no state at all.
#define MAX_INDENT_DEPTH                                            \
  ((TREE_SITTER_SERIALIZATION_BUFFER_SIZE - sizeof(uint16_t) * 2 -  \
    sizeof(uint32_t) - sizeof(uint8_t)) /                           \
   sizeof(uint16_t))

typedef struct {
  Array(uint16_t) indent_stack;
  // Dedents still owed by the current line, whose leading spaces are already behind the lexer.
  uint16_t pending_dedents;
  // `get_column()` where the current line began: not 0 after a lone `\r`, since tree-sitter counts
  // columns from `\n` alone.
  uint32_t line_start_column;
  // Caps a `repeat1($._newline)` at EOF to one zero-width NEWLINE; an EOF-path DEDENT resets it,
  // so each enclosing body gets its own.
  bool eof_newline_used;
} Scanner;

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer)    { lexer->advance(lexer, true); }

void *tree_sitter_cairn_external_scanner_create(void) {
  Scanner *s = ts_calloc(1, sizeof(Scanner));
  array_init(&s->indent_stack);
  array_push(&s->indent_stack, 0);
  s->pending_dedents = 0;
  s->line_start_column = 0;
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
  if (size + sizeof(s->pending_dedents) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
  memcpy(buffer + size, &s->pending_dedents, sizeof(s->pending_dedents));
  size += sizeof(s->pending_dedents);
  if (size + sizeof(s->line_start_column) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
  memcpy(buffer + size, &s->line_start_column, sizeof(s->line_start_column));
  size += sizeof(s->line_start_column);
  if (size + sizeof(uint8_t) > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) return 0;
  uint8_t eof_flag = s->eof_newline_used ? 1 : 0;
  memcpy(buffer + size, &eof_flag, sizeof(eof_flag));
  size += sizeof(eof_flag);
  return size;
}

// The state `create()` starts in, for an empty or truncated buffer.
static void reset_to_sentinel(Scanner *s) {
  array_clear(&s->indent_stack);
  array_push(&s->indent_stack, 0);
  s->pending_dedents = 0;
  s->line_start_column = 0;
  s->eof_newline_used = false;
}

void tree_sitter_cairn_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length == 0) {
    reset_to_sentinel(s);
    return;
  }
  array_clear(&s->indent_stack);
  s->pending_dedents = 0;
  s->line_start_column = 0;
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
  // A corrupt `len == 0` would leave the stack empty, and every other function reads `array_back`.
  if (s->indent_stack.size == 0) {
    array_push(&s->indent_stack, 0);
  }
  if (offset + sizeof(s->pending_dedents) <= length) {
    memcpy(&s->pending_dedents, buffer + offset, sizeof(s->pending_dedents));
    offset += sizeof(s->pending_dedents);
  }
  if (offset + sizeof(s->line_start_column) <= length) {
    memcpy(&s->line_start_column, buffer + offset, sizeof(s->line_start_column));
    offset += sizeof(s->line_start_column);
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

// Closes one open level. Callers reach here only above the bottom `0` sentinel.
static bool emit_dedent(Scanner *s, TSLexer *lexer) {
  (void)array_pop(&s->indent_stack);
  if (s->pending_dedents > 0) s->pending_dedents--;
  lexer->result_symbol = DEDENT;
  return true;
}

bool tree_sitter_cairn_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // Error recovery offers every external token at once, `_error_sentinel` among them; answering
  // would describe a line the parse has abandoned.
  if (valid_symbols[ERROR_SENTINEL]) return false;

  // The `x` of `9x7`: `word` would lex `x7` as an identifier. Taken before any space is skipped,
  // and only with a digit behind it, as core's `scan_number` reads a size; returning false after
  // the `advance()` rewinds it.
  if (valid_symbols[SIZE_X] && lexer->lookahead == 'x') {
    advance(lexer);
    if (lexer->lookahead < '0' || lexer->lookahead > '9') return false;
    lexer->result_symbol = SIZE_X;
    return true;
  }

  // A truth row's inputs, external so `mark_end` can stop before a `-` that is the arrow's. Spaces
  // are skipped and tabs are not, as core refuses a tab.
  if (valid_symbols[BIT_PATTERN]) {
    while (lexer->lookahead == ' ') skip(lexer);
    bool scanned = false;
    for (;;) {
      if (lexer->lookahead == '0' || lexer->lookahead == '1') {
        advance(lexer);
        scanned = true;
        lexer->mark_end(lexer);
        continue;
      }
      if (lexer->lookahead == '-') {
        advance(lexer);
        if (lexer->lookahead == '>') break;
        scanned = true;
        lexer->mark_end(lexer);
        continue;
      }
      break;
    }
    if (scanned) {
      lexer->result_symbol = BIT_PATTERN;
      return true;
    }
    return false;
  }

  // A truth row's output: one `0` or `1` with no digit of any kind behind it, since core's
  // `scan_number` reads `12` as one integer.
  if (valid_symbols[BIT]) {
    while (lexer->lookahead == ' ') skip(lexer);
    if (lexer->lookahead != '0' && lexer->lookahead != '1') return false;
    advance(lexer);
    if (lexer->lookahead >= '0' && lexer->lookahead <= '9') return false;
    lexer->result_symbol = BIT;
    return true;
  }

  // The blank and comment lines a file opens with, which no line break precedes. An indented first
  // content line is refused, as core refuses it.
  if (valid_symbols[FILE_START]) {
    for (;;) {
      uint32_t spaces = 0;
      while (lexer->lookahead == ' ') {
        spaces++;
        skip(lexer);
      }
      if (lexer->lookahead == '#') {
        while (!at_line_break(lexer)) skip(lexer);
      }
      if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
        if (lexer->lookahead == '\r') { skip(lexer); if (lexer->lookahead == '\n') skip(lexer); }
        else { skip(lexer); }
        continue;
      }
      // A file of nothing but spaces holds no line to indent.
      if (lexer->eof(lexer)) break;
      if (spaces > 0) return false;
      break;
    }
    s->line_start_column = lexer->get_column(lexer);
    lexer->result_symbol = FILE_START;
    return true;
  }

  // Drained first: these are zero-width, and no later branch can recover the count.
  if (s->pending_dedents > 0 && valid_symbols[DEDENT]) {
    if (s->indent_stack.size > 1) return emit_dedent(s, lexer);
    s->pending_dedents = 0;
  }

  // At a line start the run below is indentation, elsewhere a separator; asked before `skip()`
  // moves the column.
  bool at_line_start = lexer->get_column(lexer) == s->line_start_column;

  // Read here so a line ending in spaces still reaches the EOF and NEWLINE branches.
  uint32_t spaces = 0;
  while (lexer->lookahead == ' ') {
    skip(lexer);
    spaces++;
  }

  // At EOF: the last line's missing NEWLINE, as core's `scan_line_body` emits one, then the
  // DEDENTs, then FILE_END.
  if (lexer->eof(lexer)) {
    if (valid_symbols[NEWLINE] && !s->eof_newline_used) {
      s->eof_newline_used = true;
      lexer->result_symbol = NEWLINE;
      return true;
    }
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      s->eof_newline_used = false;
      return emit_dedent(s, lexer);
    }
    if (valid_symbols[FILE_END]) {
      lexer->result_symbol = FILE_END;
      return true;
    }
    return false;
  }

  // Consumes the break and nothing else: the layout section below refuses a bad indent on the
  // next line, so the error lands on that row.
  if (valid_symbols[NEWLINE] && (lexer->lookahead == '\n' || lexer->lookahead == '\r')) {
    if (lexer->lookahead == '\r') {
      advance(lexer);
      if (lexer->lookahead == '\n') advance(lexer);
    } else {
      advance(lexer);
    }
    s->line_start_column = lexer->get_column(lexer);
    lexer->result_symbol = NEWLINE;
    return true;
  }

  // Mid-line, a construct follows the INDENT or DEDENT that measured its line. Sound only while
  // `grammar.js` writes `_line_start` directly in front of a construct and nowhere else.
  if (valid_symbols[LINE_START] && !at_line_start) {
    lexer->result_symbol = LINE_START;
    return true;
  }

  if (!at_line_start) return false;
  // LINE_START too: a declaration after a body is asked for where neither indent token is valid.
  if (!(valid_symbols[INDENT] || valid_symbols[DEDENT] || valid_symbols[LINE_START])) {
    return false;
  }

  // Crosses blank and comment-only lines to the next line's level. A comment crossed here becomes
  // no node, so `crossed_comment` hands it back wherever a NEWLINE could carry it. `spaces` is
  // counted, not read off `get_column()`, which a lone `\r` does not restart.
  bool crossed_comment = false;
  if (valid_symbols[INDENT] || valid_symbols[DEDENT]) {
    for (;;) {
      if (lexer->lookahead == '#') {
        crossed_comment = true;
        while (!at_line_break(lexer)) skip(lexer);
      }
      if (lexer->lookahead == '\n' || lexer->lookahead == '\r') {
        if (lexer->lookahead == '\r') { skip(lexer); if (lexer->lookahead == '\n') skip(lexer); }
        else { skip(lexer); }
        spaces = 0;
        while (lexer->lookahead == ' ') { skip(lexer); spaces++; }
        continue;
      }
      break;
    }
  }

  // EOF behind crossed layout. A comment crossed on the way is lost from the tree, since declining
  // would refuse files core accepts; `test/corpus/comments.txt` pins those trees.
  if (lexer->eof(lexer)) {
    if (valid_symbols[NEWLINE] && !s->eof_newline_used) {
      s->eof_newline_used = true;
      lexer->result_symbol = NEWLINE;
      return true;
    }
    if (valid_symbols[DEDENT] && s->indent_stack.size > 1) {
      s->eof_newline_used = false;
      return emit_dedent(s, lexer);
    }
    if (valid_symbols[FILE_END]) {
      lexer->result_symbol = FILE_END;
      return true;
    }
    return false;
  }

  if (crossed_comment && valid_symbols[NEWLINE]) return false;

  // A comment-only line carries no indentation verdict.
  if (lexer->lookahead == '#') return false;

  // A tab is refused, not read as level 0, which would close bodies the file never closed.
  if (lexer->lookahead == '\t') return false;

  if (spaces & 1u) return false; // odd indent, let LR surface an ERROR
  uint16_t current = *array_back(&s->indent_stack);
  // Compared before narrowing: 131072 spaces would wrap to level 0.
  if (spaces / 2 > (uint32_t)current + 1) return false;
  uint16_t level = (uint16_t)(spaces / 2);

  if (level > current) {
    if (!valid_symbols[INDENT]) return false;
    if (s->indent_stack.size >= MAX_INDENT_DEPTH) return false;
    array_push(&s->indent_stack, level);
    lexer->result_symbol = INDENT;
    return true;
  }

  if (level < current) {
    if (!valid_symbols[DEDENT]) return false;
    // Levels rise one at a time, so the stack holds every level in between.
    s->pending_dedents = (uint16_t)(current - level);
    return emit_dedent(s, lexer);
  }

  if (valid_symbols[LINE_START]) {
    lexer->result_symbol = LINE_START;
    return true;
  }

  return false;
}
