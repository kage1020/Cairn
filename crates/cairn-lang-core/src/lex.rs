//! Lexer for the Cairn surface syntax, with synthetic `Indent`, `Dedent` and `Newline` tokens.
//!
//! The lines inside a truth table's braces carry no indentation, so they take no `Indent` or
//! `Dedent`; each still ends in a `Newline`, which separates the rows.
//!
//! A bad indentation ends the scan. A failure inside a line becomes a [`TokenKind::Unlexed`] in
//! the parser's own pass, since a directive that takes raw text to end of line judges it itself.

use crate::error::{IntContext, LexError, Position, Span};

const INDENT_STACK_NONEMPTY: &str = "indent_stack invariant: bottom 0 sentinel is never popped";

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
    /// A run of decimal digits, verbatim, since a truth-table row's leading zeros are data.
    Int {
        /// Raw source lexeme.
        lexeme: String,
    },
    /// Boolean literal `true` / `false`.
    Bool(bool),
    /// Double-quoted string literal, verbatim and without the quotes.
    Str(String),
    /// Size literal `WxH`, e.g. `9x7`.
    Size(u32, u32),
    /// `@` sigil.
    At,
    /// `->` arrow.
    Arrow,
    /// `-` on its own, which only a truth row reads, as a don't-care bit.
    Minus,
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
    /// A stretch of a line that is no token; only [`crate::parse()`]'s own pass sees one.
    Unlexed,
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
            Self::Minus => f.write_str("`-`"),
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
            Self::Unlexed => f.write_str("text that is no token"),
        }
    }
}

/// Tokenise the entire source string.
///
/// # Errors
/// Returns the first [`LexError`] encountered.
pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let Lexed {
        tokens,
        deferred,
        fatal,
    } = lex_deferring(source);
    // Every deferred failure precedes the fatal one.
    match (deferred.into_iter().next(), fatal) {
        (Some(first), _) => Err(first.error),
        (None, Some(fatal)) => Err(fatal),
        (None, None) => Ok(tokens),
    }
}

/// A failure inside a line, kept as the [`TokenKind::Unlexed`] at index `token`.
pub(crate) struct Deferred {
    /// Index of the `Unlexed` token in [`Lexed::tokens`].
    pub(crate) token: usize,
    /// What [`lex`] reports for it.
    pub(crate) error: LexError,
}

/// The result of a scan that defers failures inside a line.
pub(crate) struct Lexed {
    /// Every token scanned, up to where a `fatal` failure stopped the scan.
    pub(crate) tokens: Vec<Token>,
    /// The failures inside lines, in source order.
    pub(crate) deferred: Vec<Deferred>,
    /// The indentation failure that ended the scan, after every entry of `deferred`.
    pub(crate) fatal: Option<LexError>,
}

/// Tokenise the source, keeping each failure inside a line as a [`TokenKind::Unlexed`].
pub(crate) fn lex_deferring(source: &str) -> Lexed {
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
    deferred: Vec<Deferred>,
    /// Index in `out` of the current line's first token.
    line_first_token: usize,
    /// Whether a truth table's `{` is open, from an `assert` line's `{` to the next `}`.
    in_truth_table: bool,
}

/// U+FEFF, the byte-order mark, as it appears in UTF-8.
const BOM: &str = "\u{feff}";

impl<'src> Lexer<'src> {
    fn new(src: &'src str) -> Self {
        // Skipped by `pos`, so spans stay offsets into `src`; `col` counts it, as every
        // span-derived position does.
        let has_bom = src.starts_with(BOM);
        Self {
            src,
            bytes: src.as_bytes(),
            pos: if has_bom { BOM.len() } else { 0 },
            line: 1,
            col: if has_bom { 2 } else { 1 },
            indent_stack: vec![0],
            out: Vec::new(),
            deferred: Vec::new(),
            line_first_token: 0,
            in_truth_table: false,
        }
    }

    fn run(mut self) -> Lexed {
        while self.pos < self.bytes.len() {
            if let Err(fatal) = self.scan_line_start() {
                return Lexed {
                    tokens: self.out,
                    deferred: self.deferred,
                    fatal: Some(fatal),
                };
            }
            self.scan_line_body();
        }
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.push_synthetic(TokenKind::Dedent);
        }
        Lexed {
            tokens: self.out,
            deferred: self.deferred,
            fatal: None,
        }
    }

    /// Emit `Indent` / `Dedent` for the next line that is not blank or a comment, unless it is
    /// inside a truth table.
    fn scan_line_start(&mut self) -> Result<(), LexError> {
        loop {
            let line_start = self.pos;
            let start_position = self.position();
            let spaces = self.count_leading_spaces()?;

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
                return Ok(());
            }
            if self.in_truth_table {
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

    /// Scan the rest of a logical line through its `Newline`, deferring a token that fails.
    fn scan_line_body(&mut self) {
        self.line_first_token = self.out.len();
        loop {
            self.skip_spaces();
            let Some(b) = self.peek() else {
                if !self.last_is_newline() {
                    self.push_synthetic(TokenKind::Newline);
                }
                return;
            };
            if b == b'\n' || b == b'\r' {
                // Before the break, so an error at end of line names the line that is wrong.
                let start = self.pos;
                let position = self.position();
                self.consume_line_break();
                self.push_at(TokenKind::Newline, start..self.pos, position);
                return;
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
            let start = self.pos;
            let position = self.position();
            if let Err(error) = self.scan_token(b) {
                self.deferred.push(Deferred {
                    token: self.out.len(),
                    error,
                });
                self.push_at(TokenKind::Unlexed, start..self.pos, position);
            }
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
            b'-' => {
                self.advance();
                self.push_at(TokenKind::Minus, start..self.pos, position);
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
                // Only `assert truth` takes a `{`: one in a directive's raw value is text, and
                // must not suspend the layout of the lines below it.
                if self.line_opens_with("assert") {
                    self.in_truth_table = true;
                }
                self.push_at(TokenKind::LBrace, start..self.pos, position);
            }
            b'}' => {
                self.advance();
                self.in_truth_table = false;
                self.push_at(TokenKind::RBrace, start..self.pos, position);
            }
            b'"' => self.scan_string(start, position)?,
            c if c.is_ascii_digit() => self.scan_number(start, position)?,
            c if is_ident_start(c) => self.scan_ident(start, position),
            _ => {
                // A whole character, so `pos` never lands inside a UTF-8 sequence.
                let ch = self.peek_char().unwrap_or('\0');
                self.advance_char();
                return Err(LexError::UnexpectedChar { position, ch });
            }
        }
        Ok(())
    }

    fn scan_string(&mut self, start: usize, position: Position) -> Result<(), LexError> {
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
        if self.peek() == Some(b'x') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
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
        self.push_at(TokenKind::Int { lexeme }, start..self.pos, position);
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

    /// Advance by one byte and one column. The byte at `pos` must be ASCII.
    fn advance(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    fn advance_char(&mut self) {
        if let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            self.col += 1;
        }
    }

    /// Consume one line break and start the next line; whether there was one.
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

    /// Whether the current line's first token is the identifier `word`.
    fn line_opens_with(&self, word: &str) -> bool {
        matches!(
            self.out.get(self.line_first_token).map(|t| &t.kind),
            Some(TokenKind::Ident(first)) if first == word
        )
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
