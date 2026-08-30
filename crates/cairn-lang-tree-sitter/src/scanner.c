#include "tree_sitter/parser.h"
#include "tree_sitter/array.h"

enum TokenType {
  INDENT,
  DEDENT,
  NEWLINE,
  FILE_START,
  SIZE_X,
  LINE_START,
  ERROR_SENTINEL,
};

// Upper bound on `indent_stack`'s length that `serialize()` below can
// always fit in `TREE_SITTER_SERIALIZATION_BUFFER_SIZE`: the 2-byte
// length prefix, plus one `uint16_t` per stack entry, plus the 2-byte
// `pending_dedents` counter, the 4-byte `line_start_column`, and the
// 1-byte `eof_newline_used` flag.
// Enforced on push (see the INDENT branch in
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
#define MAX_INDENT_DEPTH                                            \
  ((TREE_SITTER_SERIALIZATION_BUFFER_SIZE - sizeof(uint16_t) * 2 -  \
    sizeof(uint32_t) - sizeof(uint8_t)) /                           \
   sizeof(uint16_t))

typedef struct {
  Array(uint16_t) indent_stack;
  // Levels still to close on the line currently being read. A line that
  // returns from several levels at once owes one `_dedent` per level, and
  // the count has to be carried because only the first of them can be
  // derived from the source: `scan()` counts a line's leading spaces as
  // it reads past them, and they are already behind the lexer once the
  // first `_dedent` has been produced. Set where the line's indentation
  // is read and drained one token at a time.
  uint16_t pending_dedents;
  // What `get_column()` reports at the start of the line being read —
  // except while the blank-line loop in `scan()` is crossing lines, which
  // leaves it on the line that call started from and says why there.
  //
  // Zero for every line that follows an `\n`, and *not* zero for one that
  // follows a lone `\r`: tree-sitter counts columns from the last `\n`
  // alone, while cairn-lang-core::lex::Lexer::consume_line_break ends a
  // line on `\r` too. Recorded for the file's opening layout by the
  // FILE_START branch and thereafter wherever a line break is consumed,
  // it is what lets `scan()` ask whether it is standing where a line
  // begins — which is the whole question, since a run of spaces is that
  // line's indentation there and separator whitespace anywhere else.
  // Comparing `get_column()` against 0 instead would answer that question
  // correctly for the first line of a `\r`-terminated file, which starts
  // at column 0 like any other, and wrongly for every line after it.
  uint32_t line_start_column;
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

// Resets `s` to the same single-sentinel-level state `create()` starts
// with. Shared by every early-return path in `deserialize()` below: a
// too-short `length` (empty buffer, or one that claims more entries than
// it actually holds — truncated/corrupt input, however that occurred)
// gets the same treatment as "no prior state", rather than reading past
// `buffer + length`.
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
  // `len == 0` is itself a corrupt/impossible state — `serialize()` always
  // writes at least the base `0` level — but is representable within an
  // otherwise-valid-length buffer, so it isn't caught by the bounds checks
  // above. Restore the invariant every other function in this file relies
  // on (`array_back(&s->indent_stack)` is always safe to call) rather than
  // leaving an empty stack.
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

// Close one open level, keeping the stack and the owed-dedent count in
// step. The bottom `0` sentinel is never popped: callers only reach here
// while a level above it is still open.
static bool emit_dedent(Scanner *s, TSLexer *lexer) {
  array_pop(&s->indent_stack);
  if (s->pending_dedents > 0) s->pending_dedents--;
  lexer->result_symbol = DEDENT;
  return true;
}

// Whether the line following the line break the lexer has just consumed
// is indented in a way `cairn-lang-core::lex::Lexer::scan_line_start`
// refuses: an odd number of spaces (`LexError::OddIndent`), or a jump of
// more than one level, which that function reports as `OddIndent` too.
//
// Both are checked here, one line early, so the refusal lands on the
// break in front of the offending line rather than on the line itself.
// LINE_START refuses both where they occur as well — it is withheld for
// an odd count and for a level the stack cannot reach — so what this buys
// is no longer whether a file is refused, only where the error is
// reported and how much of the file the recovery around it keeps. Kept
// rather than removed because that is a decision about error shape with
// its own blast radius, not a consequence of the token that made it
// redundant.
//
// Called with the token already marked (see the NEWLINE branch in
// `scan()`), so the characters read here are lookahead: they are not part
// of whatever token the caller goes on to produce.
//
// Blank and comment-only lines carry no indentation, so they answer
// `false` and leave the verdict to the line break that follows them,
// which puts the refusal on the break nearest the offending line. That
// matches `scan_line_start`, which counts a blank line's leading spaces
// like any other line's but then discards the line before comparing the
// count to anything.
static bool next_line_indent_is_illegal(Scanner *s, TSLexer *lexer) {
  uint32_t spaces = 0;
  while (lexer->lookahead == ' ') {
    advance(lexer);
    spaces++;
  }
  if (at_line_break(lexer) || lexer->lookahead == '#') return false;
  if (spaces & 1u) return true;
  return spaces / 2 > (uint32_t)*array_back(&s->indent_stack) + 1;
}

bool tree_sitter_cairn_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // `_error_sentinel` belongs to no rule, so the parser marks it valid
  // only once it has given up on the current parse and is offering every
  // external token at once, looking for somewhere to resynchronise. Every
  // branch below reads the file's layout and half of them move the indent
  // stack, so answering there would describe a line the parse has already
  // abandoned. The generated table makes this reachable rather than
  // theoretical: `ts_external_scanner_states[1]` marks all five tokens
  // valid, including `FILE_START`, whose branch overwrites
  // `line_start_column` with wherever it happens to be standing.
  if (valid_symbols[ERROR_SENTINEL]) return false;

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
  // It is checked before any extras (spaces) are skipped, and before the
  // run of spaces the layout section below reads, which is exactly what
  // enforces immediate adjacency: `9 x 7` must not parse as one size
  // literal. The right of the separator is held by `token.immediate` in
  // `grammar.js`; the left is held by nothing but this branch's position,
  // which `size_space_before_x` in `tests/parser_parity.rs` keeps here.
  if (valid_symbols[SIZE_X] && lexer->lookahead == 'x') {
    advance(lexer);
    lexer->result_symbol = SIZE_X;
    return true;
  }

  // The file's opening layout, consumed once at offset 0 by the
  // `_file_start` token `source_file` opens with.
  //
  // What is left to it, now that LINE_START stands in front of every
  // construct, is the trivia: the blank and comment-only lines a file may
  // open with, which no line break precedes and which LINE_START declines
  // rather than crosses. Skipping them here is what puts the first
  // content line on the same footing as every other. Refusing to produce
  // the token then rejects a file whose first content line is indented —
  // the reference lexer refuses it too, by handing `parse_item` an
  // `Indent` where it wants an identifier.
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
      // EOF has no indentation to be wrong: a file of nothing but
      // spaces holds no line for them to indent, and the reference lexer
      // reaches its end without ever counting them.
      if (lexer->eof(lexer)) break;
      // A content line.
      if (spaces > 0) return false;
      break;
    }
    s->line_start_column = lexer->get_column(lexer);
    lexer->result_symbol = FILE_START;
    return true;
  }

  // Levels still owed by the line already being read. Drained before
  // anything else looks at the input: these tokens are zero-width, and
  // the lexer is parked just past the leading spaces the level was read
  // from, so no other branch below can recover the count.
  if (s->pending_dedents > 0 && valid_symbols[DEDENT]) {
    if (s->indent_stack.size > 1) return emit_dedent(s, lexer);
    s->pending_dedents = 0;
  }

  // Whether the lexer is parked where a line begins, recorded before
  // anything moves it.
  //
  // A run of spaces reaching this scanner means one of two things, and
  // this is what tells them apart: at the start of a line the run is the
  // line's indentation, whose length the INDENT and DEDENT branches below
  // need; anywhere else it is separator whitespace the `/ +/` extra owns.
  // Asking after the run has been read is what does not work — `skip()`
  // moves the column past it, so `get_column()` no longer reports where
  // the line began.
  bool at_line_start = lexer->get_column(lexer) == s->line_start_column;

  // The run itself, read once here and measured by counting rather than
  // by subtracting columns afterwards. Reading it up front is what puts
  // the EOF and NEWLINE branches within reach of a line that ends in a
  // space: the scanner is consulted before extras are skipped, so without
  // this read such a line arrives with the space in `lookahead` and no
  // branch below able to consume one.
  uint32_t spaces = 0;
  while (lexer->lookahead == ' ') {
    skip(lexer);
    spaces++;
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
      s->eof_newline_used = false;
      return emit_dedent(s, lexer);
    }
    return false;
  }

  // NEWLINE handling: consume \r\n / \n / \r and emit NEWLINE.
  //
  // The token is marked as soon as the break is consumed so the odd-indent
  // check that follows reads the next line without widening it. Withholding
  // the NEWLINE is what makes an odd-indented line an error: nothing else
  // in the grammar can consume a line break, so the parser has no way past
  // it. Refusing here rather than at the offending line start is the only
  // lever available — a line that sits at the level it already sits at asks
  // the scanner for no token at all, so at that point there is nothing left
  // to withhold.
  //
  // Whatever run of spaces stood in front of the break is already behind
  // the lexer, so a line that ends in one arrives here like any other. A
  // line holding nothing but spaces arrives here too, and is a break like
  // any other: the grammar wants one NEWLINE per physical line break.
  if (valid_symbols[NEWLINE] && (lexer->lookahead == '\n' || lexer->lookahead == '\r')) {
    if (lexer->lookahead == '\r') {
      advance(lexer);
      if (lexer->lookahead == '\n') advance(lexer);
    } else {
      advance(lexer);
    }
    uint32_t next_line_column = lexer->get_column(lexer);
    lexer->mark_end(lexer);
    if (next_line_indent_is_illegal(s, lexer)) return false;
    s->line_start_column = next_line_column;
    lexer->result_symbol = NEWLINE;
    return true;
  }

  // A construct is starting on a line whose leading run is already behind
  // the lexer, consumed by the INDENT or DEDENT that measured it. The
  // line has had its verdict; this token is owed no further reading.
  //
  // Nothing here checks that, and nothing can cheaply: the claim rests on
  // where `_line_start` is written in `grammar.js`, which is directly in
  // front of a construct and nowhere else. A construct starts either at
  // its own level's column, where `at_line_start` holds, or just past the
  // indent token that opened its line, where it does not. Put
  // `_line_start` anywhere else in a rule and this branch would grant it
  // mid-line, without measuring anything.
  if (valid_symbols[LINE_START] && !at_line_start) {
    lexer->result_symbol = LINE_START;
    return true;
  }

  // Not where a line begins, so any run read above was separator
  // whitespace the extras rule owns, and whatever follows it is a token
  // for tree-sitter's own lexer. Returning false rewinds over it.
  if (!at_line_start) return false;
  // LINE_START belongs in this gate as much as the indent tokens do: a
  // declaration following a body is asked for at a position where neither
  // INDENT nor DEDENT is valid, and without it that line could not start.
  if (!(valid_symbols[INDENT] || valid_symbols[DEDENT] || valid_symbols[LINE_START])) {
    return false;
  }

  // Skip blank and comment-only lines without shifting indent state, so
  // the level measured below is the next line that carries one.
  //
  // Gated on INDENT and DEDENT because looking past a blank line for the
  // next level is a question only those two ask.
  //
  // Crossing costs something, and `crossed_comment` is what pays it back.
  // A line crossed here is consumed as whitespace, and a comment line
  // consumed as whitespace is a comment that never reaches the `comment`
  // extra and so never becomes a node — invisible in an editor, and
  // invisible to every test in this crate that compares verdicts. So
  // where the grammar could take a NEWLINE instead, this call declines
  // rather than commits: the comment is lexed as an extra, the break
  // after it is a NEWLINE, and the next call measures the line behind it.
  // Where no NEWLINE is on offer — between a declaration header and the
  // body it opens — declining would refuse a file the reference parser
  // accepts, so the crossing stands and the comment is spent.
  //
  // Each crossed line restarts `spaces`, which is also why the count is
  // kept rather than read back off `get_column()` at the end: the column
  // restarts at `\n` and at nothing else, so every lone `\r` the loop
  // crosses would leave a column-derived count running on from the line
  // before.
  //
  // `line_start_column` is deliberately left describing the line this
  // call started on. It is read again — `at_line_start` above reads it at
  // the top of every call — but the next call stands past the indentation
  // of whatever line this loop lands on, where the stale base and the
  // true one answer alike; and the NEWLINE branch records the real base
  // for the line after that. Confirmed by measurement rather than by
  // reading: adding the write changes no tree across a sweep of nested
  // and dedenting bodies in all three line endings.
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
    return false;
  }

  // Hand back a comment the loop crossed, wherever a NEWLINE can carry
  // the lines it was hidden among. See the loop's own note above.
  if (crossed_comment && valid_symbols[NEWLINE]) return false;

  // A comment-only line carries no indentation verdict, and the token
  // that ends it is the newline behind it rather than this one. Reachable
  // only where the loop above did not run, since the loop crosses such a
  // line rather than landing on it.
  if (lexer->lookahead == '#') return false;

  // `spaces` now holds the leading run of whatever real line the loop
  // landed on.
  //
  // A tab is refused rather than counted as zero. The file is refused
  // either way — the `/ +/` extra matches spaces alone, so a tab reaches
  // tree-sitter's own lexer as a character no token starts with — but
  // answering here reads the line as a return to level 0 and emits the
  // DEDENTs to match, closing bodies the file never closed and taking the
  // lines written inside them down with it. No verdict moves, so it is
  // pinned on placement in `tests/parser_parity.rs` instead.
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
    // Levels only ever go up by one (see the INDENT branch above), so the
    // stack holds every level between `level` and `current` and the line
    // closes exactly that many.
    s->pending_dedents = (uint16_t)(current - level);
    return emit_dedent(s, lexer);
  }

  // The level did not change, and LINE_START is the token that says so.
  // Reaching here is what makes a line legal; every path above that
  // returns false without producing an indent token withholds it, and a
  // construct that cannot start is how an illegally indented line is
  // refused rather than quietly reshaped.
  if (valid_symbols[LINE_START]) {
    lexer->result_symbol = LINE_START;
    return true;
  }

  return false;
}
