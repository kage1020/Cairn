//! Lexer for the Cairn surface syntax.
//!
//! Produces a stream of [`Token`]s with byte spans and 1-based line/column
//! positions, plus synthetic `Indent` / `Dedent` / `Newline` tokens so the
//! parser can treat the language as indent-structured without re-walking
//! whitespace.
//!
//! Line endings: `\n`, `\r\n`, and lone `\r` are all accepted as a single
//! logical newline so files written on Windows (`core.autocrlf=true`) lex the
//! same as files written on Linux.
//!
//! Indent/Dedent asymmetry: only one `Indent` token may be emitted per indent
//! step (the lexer rejects multi-level jumps as `OddIndent`), but a single
//! dedented line emits *one `Dedent` per level closed*. Parsers can therefore
//! rely on `Dedent` counts to know how many scopes ended on that line.
//!
//! Comments (`#` to end-of-line), blank lines, and trailing whitespace are
//! discarded silently; everything else either becomes a token or fails with a
//! [`LexError`].

use crate::error::{LexError, Position, Span};

/// `indent_stack` is seeded with `0` in `Lexer::new` and we only pop while
/// the top is strictly greater than the current level, so the bottom-of-stack
/// zero sentinel is never popped. The `expect` calls below document that
/// invariant rather than hiding it behind an `unwrap_or(&0)` default.
const INDENT_STACK_NONEMPTY: &str = "indent_stack invariant: bottom 0 sentinel is never popped";

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
    /// Double-quoted string literal (escapes are not interpreted in M1).
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

impl<'src> Lexer<'src> {
    fn new(src: &'src str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
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

            if spaces % 2 != 0 {
                return Err(LexError::OddIndent {
                    position: start_position,
                    got: spaces,
                });
            }
            let level = spaces / 2;
            let current = *self.indent_stack.last().expect(INDENT_STACK_NONEMPTY);
            if level > current {
                // Only allow one level of indent increase at a time.
                if level != current + 1 {
                    return Err(LexError::OddIndent {
                        position: start_position,
                        got: spaces,
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
                self.consume_line_break();
                self.push_synthetic(TokenKind::Newline);
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
            let w = w_str.parse::<u32>().map_err(|_| LexError::InvalidInt {
                position,
                lexeme: w_str.to_owned(),
            })?;
            let h = h_str.parse::<u32>().map_err(|_| LexError::InvalidInt {
                position,
                lexeme: h_str.to_owned(),
            })?;
            self.push_at(TokenKind::Size(w, h), start..self.pos, position);
            return Ok(());
        }
        let lexeme = self.src[start..lexeme_end].to_owned();
        let value = lexeme.parse::<i64>().map_err(|_| LexError::InvalidInt {
            position,
            lexeme: lexeme.clone(),
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
    fn consume_line_break(&mut self) -> bool {
        match self.peek() {
            Some(b'\r') => {
                self.pos += 1;
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
                self.line += 1;
                self.col = 1;
                true
            }
            Some(b'\n') => {
                self.pos += 1;
                self.line += 1;
                self.col = 1;
                true
            }
            _ => false,
        }
    }

    fn position(&self) -> Position {
        Position {
            line: self.line,
            col: self.col,
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
