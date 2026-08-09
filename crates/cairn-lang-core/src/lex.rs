//! Lexer for the Cairn surface syntax.
//!
//! Produces a stream of [`Token`]s with byte spans and 1-based line/column
//! positions, plus synthetic `Indent` / `Dedent` / `Newline` tokens so the
//! parser can treat the language as indent-structured without re-walking
//! whitespace.
//!
//! Line endings: `\n`, `\r\n`, and lone `\r` are all accepted as a single
//! logical newline so files written on Windows (`core.autocrlf=true`) lex the
//! same as files written on Linux. The rule itself lives in
//! [`crate::lines`], shared with every layer that resolves a byte offset
//! into a `line:column` after the fact — a `Newline` here and a diagnostic
//! there have to name the same row.
//!
//! Indent/Dedent asymmetry: only one `Indent` token may be emitted per indent
//! step (the lexer rejects multi-level jumps as `IndentJump`), but a single
//! dedented line emits *one `Dedent` per level closed*. Parsers can therefore
//! rely on `Dedent` counts to know how many scopes ended on that line.
//!
//! A byte-order mark at the very start of the file is skipped, since that
//! is what a default Windows editor writes and it is not part of the text.
//! One anywhere else is an ordinary stray character.
//!
//! Comments (`#` to end-of-line), blank lines, and trailing whitespace are
//! discarded silently; everything else either becomes a token or fails with a
//! [`LexError`].

use crate::error::{IntContext, LexError, Position, Span};

/// `indent_stack` is seeded with `0` in `Lexer::new` and we only pop while
/// the top is strictly greater than the current level, so the bottom-of-stack
/// zero sentinel is never popped. The `expect` calls below document that
/// invariant rather than hiding it behind an `unwrap_or(&0)` default.
const INDENT_STACK_NONEMPTY: &str = "indent_stack invariant: bottom 0 sentinel is never popped";

/// Leading spaces per indentation level.
///
/// Named because the number appears in three roles that a bare `2` does
/// not distinguish: the divisor turning a space count into a level, the
/// parity a width must satisfy, and the multiplier turning a level back
/// into the width a diagnostic asks for.
const SPACES_PER_LEVEL: u32 = 2;

/// One lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What the token represents.
    pub kind: TokenKind,
    /// Byte range into the source string.
    pub span: Span,
    /// 1-based line/column of the first byte of the token.
    pub position: Position,
}

/// Kind of a lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TokenKind {
    /// Bare identifier: `[A-Za-z_][A-Za-z0-9_]*`.
    Ident(String),
    /// Integer literal, also used for bit patterns in truth-table rows.
    /// The raw source lexeme is preserved so callers can distinguish e.g. `00` from `0`.
    Int {
        /// Parsed integer value.
        value: i64,
        /// Raw source lexeme (preserves leading zeros).
        lexeme: String,
    },
    /// Boolean literal `true` / `false`.
    Bool(bool),
    /// Double-quoted string literal. Escape sequences are preserved verbatim
    /// at this layer; interpretation is left to a later layer.
    Str(String),
    /// Size literal `WxH`, e.g. `9x7`.
    Size(u32, u32),
    /// `@` sigil.
    At,
    /// `->` arrow.
    Arrow,
    /// `=` (key/value or `logic =`).
    Eq,
    /// `>=`.
    GreaterEq,
    /// `<=`.
    LessEq,
    /// `>`.
    Greater,
    /// `<`.
    Less,
    /// `.`.
    Dot,
    /// `,`.
    Comma,
    /// `;`.
    Semi,
    /// `:` (block-header marker).
    Colon,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// End of a logical line.
    Newline,
    /// Indent introduction (one INDENT per nesting level entered).
    Indent,
    /// Indent termination (one DEDENT per nesting level exited).
    Dedent,
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ident(name) => write!(f, "identifier `{name}`"),
            Self::Int { lexeme, .. } => write!(f, "integer `{lexeme}`"),
            Self::Bool(b) => write!(f, "`{b}`"),
            Self::Str(_) => f.write_str("string literal"),
            Self::Size(w, h) => write!(f, "size `{w}x{h}`"),
            Self::At => f.write_str("`@`"),
            Self::Arrow => f.write_str("`->`"),
            Self::Eq => f.write_str("`=`"),
            Self::GreaterEq => f.write_str("`>=`"),
            Self::LessEq => f.write_str("`<=`"),
            Self::Greater => f.write_str("`>`"),
            Self::Less => f.write_str("`<`"),
            Self::Dot => f.write_str("`.`"),
            Self::Comma => f.write_str("`,`"),
            Self::Semi => f.write_str("`;`"),
            Self::Colon => f.write_str("`:`"),
            Self::LBracket => f.write_str("`[`"),
            Self::RBracket => f.write_str("`]`"),
            Self::LParen => f.write_str("`(`"),
            Self::RParen => f.write_str("`)`"),
            Self::LBrace => f.write_str("`{`"),
            Self::RBrace => f.write_str("`}`"),
            Self::Newline => f.write_str("end of line"),
            Self::Indent => f.write_str("indent"),
            Self::Dedent => f.write_str("dedent"),
        }
    }
}

/// Tokenise the entire source string.
///
/// # Errors
/// Returns the first [`LexError`] encountered.
pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(source).run()
}

struct Lexer<'src> {
    src: &'src str,
    bytes: &'src [u8],
    pos: usize,
    line: u32,
    col: u32,
    indent_stack: Vec<u32>,
    out: Vec<Token>,
}

/// U+FEFF, the byte-order mark, as it appears in UTF-8.
const BOM: &str = "\u{feff}";

impl<'src> Lexer<'src> {
    fn new(src: &'src str) -> Self {
        // A leading BOM is metadata about the encoding, not text. Skipped
        // by advancing `pos` rather than by trimming `src`, so every span
        // stays an offset into the string the caller handed us — an
        // editor highlighting a diagnostic indexes the same bytes.
        //
        // `col` counts it, though it occupies no column an author can
        // see. Not counting it would be the friendlier number in
        // isolation, but it is not the only one reported: every
        // span-derived position — `LineStarts::position` here,
        // `LineIndex` in the LSP — resolves a column by counting
        // characters from the line's start, and both count the mark.
        // A lexer that skipped it would put its own errors one column
        // left of every other layer's, on the first line of exactly the
        // files a Windows editor produces.
        let has_bom = src.starts_with(BOM);
        Self {
            src,
            bytes: src.as_bytes(),
            pos: if has_bom { BOM.len() } else { 0 },
            line: 1,
            col: if has_bom { 2 } else { 1 },
            indent_stack: vec![0],
            out: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        while self.pos < self.bytes.len() {
            self.scan_line_start()?;
            self.scan_line_body()?;
        }
        // Close any open indentation when the file ends.
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.push_synthetic(TokenKind::Dedent);
        }
        Ok(self.out)
    }

    /// Inspect leading whitespace of a (potential) logical line and emit
    /// `Indent` / `Dedent` tokens. Skips blank and comment-only lines so they
    /// do not perturb indentation.
    fn scan_line_start(&mut self) -> Result<(), LexError> {
        loop {
            let line_start = self.pos;
            let start_position = self.position();
            let spaces = self.count_leading_spaces()?;

            // Skip blank or comment-only lines without changing indent state.
            if self.peek().is_none_or(is_line_break_or_comment) {
                if self.peek() == Some(b'#') {
                    while let Some(b) = self.peek() {
                        if b == b'\n' || b == b'\r' {
                            break;
                        }
                        self.advance();
                    }
                }
                if self.consume_line_break() {
                    continue;
                }
                // EOF after blanks/comments.
                return Ok(());
            }

            if spaces % SPACES_PER_LEVEL != 0 {
                return Err(LexError::OddIndent {
                    position: start_position,
                    got: spaces,
                });
            }
            let level = spaces / SPACES_PER_LEVEL;
            let current = *self.indent_stack.last().expect(INDENT_STACK_NONEMPTY);
            if level > current {
                // Only allow one level of indent increase at a time.
                //
                // Its own variant rather than `OddIndent`: 4 spaces is a
                // multiple of 2, so "indentation must be a multiple of 2"
                // describes a rule the line already satisfies. `expected`
                // carries the one width that opens exactly one level from
                // here, which is what the author has to write.
                if level != current + 1 {
                    return Err(LexError::IndentJump {
                        position: start_position,
                        got: spaces,
                        expected: (current + 1) * SPACES_PER_LEVEL,
                    });
                }
                self.indent_stack.push(level);
                self.push_at(TokenKind::Indent, line_start..self.pos, start_position);
            } else {
                while *self.indent_stack.last().expect(INDENT_STACK_NONEMPTY) > level {
                    self.indent_stack.pop();
                    self.push_at(TokenKind::Dedent, line_start..self.pos, start_position);
                }
                if *self.indent_stack.last().expect(INDENT_STACK_NONEMPTY) != level {
                    return Err(LexError::UnmatchedDedent {
                        position: start_position,
                    });
                }
            }
            return Ok(());
        }
    }

    fn count_leading_spaces(&mut self) -> Result<u32, LexError> {
        let mut count: u32 = 0;
        while let Some(b) = self.peek() {
            match b {
                b' ' => {
                    self.advance();
                    count += 1;
                }
                b'\t' => {
                    return Err(LexError::TabIndent {
                        position: self.position(),
                    });
                }
                _ => break,
            }
        }
        Ok(count)
    }

    /// Scan the body of a logical line up to and including a single `Newline`
    /// (or EOF).
    fn scan_line_body(&mut self) -> Result<(), LexError> {
        loop {
            self.skip_spaces();
            let Some(b) = self.peek() else {
                if !self.last_is_newline() {
                    self.push_synthetic(TokenKind::Newline);
                }
                return Ok(());
            };
            if b == b'\n' || b == b'\r' {
                // Recorded before the break is consumed. A `Newline` is
                // where a line *ends*, and the parser reports "expected X,
                // got end of line" at the position of the token it stopped
                // at — so a position taken after the break names the first
                // column of the next line, which for the last line of a
                // file is a line the file does not have.
                let start = self.pos;
                let position = self.position();
                self.consume_line_break();
                self.push_at(TokenKind::Newline, start..self.pos, position);
                return Ok(());
            }
            if b == b'#' {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.advance();
                }
                continue;
            }
            self.scan_token(b)?;
        }
    }

    fn scan_token(&mut self, b: u8) -> Result<(), LexError> {
        let start = self.pos;
        let position = self.position();
        match b {
            b'@' => {
                self.advance();
                self.push_at(TokenKind::At, start..self.pos, position);
            }
            b'-' if self.peek_at(1) == Some(b'>') => {
                self.advance();
                self.advance();
                self.push_at(TokenKind::Arrow, start..self.pos, position);
            }
            b'=' => {
                self.advance();
                self.push_at(TokenKind::Eq, start..self.pos, position);
            }
            b'>' if self.peek_at(1) == Some(b'=') => {
                self.advance();
                self.advance();
                self.push_at(TokenKind::GreaterEq, start..self.pos, position);
            }
            b'<' if self.peek_at(1) == Some(b'=') => {
                self.advance();
                self.advance();
                self.push_at(TokenKind::LessEq, start..self.pos, position);
            }
            b'>' => {
                self.advance();
                self.push_at(TokenKind::Greater, start..self.pos, position);
            }
            b'<' => {
                self.advance();
                self.push_at(TokenKind::Less, start..self.pos, position);
            }
            b'.' => {
                self.advance();
                self.push_at(TokenKind::Dot, start..self.pos, position);
            }
            b',' => {
                self.advance();
                self.push_at(TokenKind::Comma, start..self.pos, position);
            }
            b';' => {
                self.advance();
                self.push_at(TokenKind::Semi, start..self.pos, position);
            }
            b':' => {
                self.advance();
                self.push_at(TokenKind::Colon, start..self.pos, position);
            }
            b'[' => {
                self.advance();
                self.push_at(TokenKind::LBracket, start..self.pos, position);
            }
            b']' => {
                self.advance();
                self.push_at(TokenKind::RBracket, start..self.pos, position);
            }
            b'(' => {
                self.advance();
                self.push_at(TokenKind::LParen, start..self.pos, position);
            }
            b')' => {
                self.advance();
                self.push_at(TokenKind::RParen, start..self.pos, position);
            }
            b'{' => {
                self.advance();
                self.push_at(TokenKind::LBrace, start..self.pos, position);
            }
            b'}' => {
                self.advance();
                self.push_at(TokenKind::RBrace, start..self.pos, position);
            }
            b'"' => self.scan_string(start, position)?,
            c if c.is_ascii_digit() => self.scan_number(start, position)?,
            c if is_ident_start(c) => self.scan_ident(start, position),
            _ => {
                // Read the *character* (not the byte) so multi-byte non-ASCII
                // codepoints are reported faithfully and `pos` advances past
                // the whole codepoint (avoids landing mid-sequence).
                let ch = self.peek_char().unwrap_or('\0');
                self.advance_char();
                return Err(LexError::UnexpectedChar { position, ch });
            }
        }
        Ok(())
    }

    fn scan_string(&mut self, start: usize, position: Position) -> Result<(), LexError> {
        // Consume opening quote.
        self.advance();
        let content_start = self.pos;
        loop {
            let Some(ch) = self.peek_char() else {
                return Err(LexError::UnterminatedString { position });
            };
            if ch == '\n' || ch == '\r' {
                return Err(LexError::UnterminatedString { position });
            }
            if ch == '"' {
                let content_end = self.pos;
                self.advance();
                let lexeme = self.src[content_start..content_end].to_owned();
                self.push_at(TokenKind::Str(lexeme), start..self.pos, position);
                return Ok(());
            }
            self.advance_char();
        }
    }

    fn scan_number(&mut self, start: usize, position: Position) -> Result<(), LexError> {
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        let lexeme_end = self.pos;
        // Size literal: `<digits>x<digits>` produced as a single Size token.
        if self.peek() == Some(b'x') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.advance(); // 'x'
            let h_start = self.pos;
            while let Some(b) = self.peek() {
                if b.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            let w_str = &self.src[start..lexeme_end];
            let h_str = &self.src[h_start..self.pos];
            // A `Size` holds two extents. If the run continues into a
            // third (`2x2x9`) or into a word (`2x2y`), the author wrote
            // something this token cannot carry, and splitting it into a
            // `Size` plus an identifier reports the wrong thing in the
            // wrong place: the identifier lands among the parser's
            // positional values, where `check::positional` eventually
            // reports it as a stray argument — a true statement about a
            // token the author never wrote, pointing past the literal
            // that produced it. In a declaration header there is no
            // positional list at all, so the parser fails at the end of
            // the line instead.
            //
            // Refusing here also reaches the consumers that never run
            // `check`: the tree-sitter grammar, an editor's scan path, a
            // formatter. `check::positional` is the only guard today, and
            // it sits downstream of all of them.
            if let Some(found) = self.peek().filter(|b| is_ident_continue(*b)) {
                return Err(LexError::TrailingSizeSegment {
                    position,
                    literal: self.src[start..self.pos].to_owned(),
                    found: char::from(found),
                });
            }
            let w = w_str.parse::<u32>().map_err(|err| LexError::InvalidInt {
                position,
                context: IntContext::SizeWidth,
                lexeme: w_str.to_owned(),
                kind: *err.kind(),
            })?;
            let h = h_str.parse::<u32>().map_err(|err| LexError::InvalidInt {
                position,
                context: IntContext::SizeHeight,
                lexeme: h_str.to_owned(),
                kind: *err.kind(),
            })?;
            self.push_at(TokenKind::Size(w, h), start..self.pos, position);
            return Ok(());
        }
        let lexeme = self.src[start..lexeme_end].to_owned();
        let value = lexeme.parse::<i64>().map_err(|err| LexError::InvalidInt {
            position,
            context: IntContext::IntLiteral,
            lexeme: lexeme.clone(),
            kind: *err.kind(),
        })?;
        self.push_at(TokenKind::Int { value, lexeme }, start..self.pos, position);
        Ok(())
    }

    fn scan_ident(&mut self, start: usize, position: Position) {
        while let Some(b) = self.peek() {
            if is_ident_continue(b) {
                self.advance();
            } else {
                break;
            }
        }
        let lexeme = &self.src[start..self.pos];
        let kind = match lexeme {
            "true" => TokenKind::Bool(true),
            "false" => TokenKind::Bool(false),
            other => TokenKind::Ident(other.to_owned()),
        };
        self.push_at(kind, start..self.pos, position);
    }

    fn skip_spaces(&mut self) {
        while let Some(b' ') = self.peek() {
            self.advance();
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// Advance by one ASCII byte and one column. Caller asserts that the byte
    /// at `pos` is ASCII (every existing call site reads via `peek()` first).
    fn advance(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    /// Advance by one full Unicode code point — used inside string literals and
    /// the unexpected-character recovery path, where non-ASCII may appear.
    fn advance_char(&mut self) {
        if let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            self.col += 1;
        }
    }

    /// Consume one line break (`\n`, `\r\n`, or lone `\r`) if present and bump
    /// the line counter. Returns `true` if any input was consumed.
    ///
    /// Which byte sequences count is [`crate::lines::terminator_len`]'s to
    /// say, so that this lexer and every layer that resolves a byte offset
    /// into a `line:column` after the fact split the source the same way.
    fn consume_line_break(&mut self) -> bool {
        let Some(len) = crate::lines::terminator_len(self.src, self.pos) else {
            return false;
        };
        self.pos += len;
        self.line += 1;
        self.col = 1;
        true
    }

    fn position(&self) -> Position {
        // `self.line` and `self.col` are seeded with 1 in `Lexer::new` and only
        // ever advance (line via newline, col via byte/char). They never hit
        // zero, so the `NonZeroU32::new` calls below are total.
        Position {
            line: std::num::NonZeroU32::new(self.line).expect("lex line is 1-based"),
            col: std::num::NonZeroU32::new(self.col).expect("lex col is 1-based"),
        }
    }

    fn push_at(&mut self, kind: TokenKind, span: Span, position: Position) {
        self.out.push(Token {
            kind,
            span,
            position,
        });
    }

    fn push_synthetic(&mut self, kind: TokenKind) {
        let span = self.pos..self.pos;
        let position = self.position();
        self.out.push(Token {
            kind,
            span,
            position,
        });
    }

    fn last_is_newline(&self) -> bool {
        matches!(self.out.last().map(|t| &t.kind), Some(TokenKind::Newline))
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_line_break_or_comment(b: u8) -> bool {
    b == b'\n' || b == b'\r' || b == b'#'
}
