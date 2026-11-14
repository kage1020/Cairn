//! An `nbt={...}` compound: the subset of Minecraft's SNBT that every Java version Cairn targets
//! reads alike (`spec/syntax` "Literals and separators"). The lexer checks it here and keeps it as
//! written; nothing in core reads what it holds.

/// Deepest nesting of compounds, lists and arrays an `nbt={...}` value holds, itself included.
pub const MAX_NBT_DEPTH: usize = 64;

/// Why a compound was refused, and the byte offset into the source it was refused at.
pub(super) struct Refusal {
    pub(super) at: usize,
    pub(super) reason: String,
}

/// Scan the compound whose `{` is at byte `start` of `src`, returning the offset past its `}`.
pub(super) fn scan(src: &str, start: usize) -> Result<usize, Refusal> {
    let mut scanner = Scanner {
        src,
        bytes: src.as_bytes(),
        pos: start,
        depth: 0,
    };
    scanner.compound()?;
    Ok(scanner.pos)
}

/// The type of an NBT value, which a list holds one of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    String,
    List,
    Compound,
    ByteArray,
    IntArray,
    LongArray,
}

impl Tag {
    /// Whether the type holds whole numbers only.
    fn is_whole(self) -> bool {
        matches!(self, Self::Byte | Self::Short | Self::Int | Self::Long)
    }

    /// The type as a noun phrase, with its article.
    fn noun(self) -> &'static str {
        match self {
            Self::Byte => "a byte",
            Self::Short => "a short",
            Self::Int => "an int",
            Self::Long => "a long",
            Self::Float => "a float",
            Self::Double => "a double",
            Self::String => "a string",
            Self::List => "a list",
            Self::Compound => "a compound",
            Self::ByteArray => "a byte array",
            Self::IntArray => "an int array",
            Self::LongArray => "a long array",
        }
    }
}

struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Scanner<'_> {
    fn compound(&mut self) -> Result<Tag, Refusal> {
        self.enter()?;
        let mut keys: Vec<String> = Vec::new();
        self.skip_spaces();
        if self.peek() == Some(b'}') {
            self.pos += 1;
        } else {
            loop {
                let key_at = self.pos;
                let key = self.key()?;
                if keys.contains(&key) {
                    return Err(Refusal {
                        at: key_at,
                        reason: format!(
                            "an NBT compound names each key once, and `{key}` is named twice: \
                             delete one of them"
                        ),
                    });
                }
                keys.push(key);
                self.skip_spaces();
                if self.peek() != Some(b':') {
                    return Err(self.expected("`:` after the key"));
                }
                self.pos += 1;
                self.skip_spaces();
                self.value()?;
                self.skip_spaces();
                if !self.separator(b'}', "entries")? {
                    break;
                }
            }
        }
        self.depth -= 1;
        Ok(Tag::Compound)
    }

    fn key(&mut self) -> Result<String, Refusal> {
        match self.peek() {
            Some(quote @ (b'"' | b'\'')) => self.quoted(quote),
            Some(b) if is_unquoted(b) => {
                let start = self.pos;
                self.skip_unquoted();
                Ok(self.src[start..self.pos].to_owned())
            }
            _ => Err(self.expected("a key")),
        }
    }

    fn value(&mut self) -> Result<Tag, Refusal> {
        match self.peek() {
            Some(b'{') => self.compound(),
            Some(b'[') => self.list_or_array(),
            Some(quote @ (b'"' | b'\'')) => self.quoted(quote).map(|_| Tag::String),
            Some(b'+' | b'-' | b'0'..=b'9') => self.number(),
            Some(b) if is_unquoted(b) => self.word(),
            _ => Err(self.expected("a value")),
        }
    }

    /// An unquoted word in value position, which only `true` and `false` may be.
    fn word(&mut self) -> Result<Tag, Refusal> {
        let start = self.pos;
        self.skip_unquoted();
        match &self.src[start..self.pos] {
            "true" | "false" => Ok(Tag::Byte),
            word => Err(Refusal {
                at: start,
                reason: format!(
                    "`{word}` is not quoted: a string in `nbt={{...}}` is, as `\"{word}\"`, since \
                     SNBT reads a word that is not a number as text"
                ),
            }),
        }
    }

    fn number(&mut self) -> Result<Tag, Refusal> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let whole_start = self.pos;
        self.skip_digits();
        if self.pos == whole_start {
            return Err(self.expected("a digit"));
        }
        let whole_end = self.pos;
        let fraction = self.peek() == Some(b'.');
        if fraction {
            self.pos += 1;
            let fraction_start = self.pos;
            self.skip_digits();
            if self.pos == fraction_start {
                return Err(Refusal {
                    at: start,
                    reason: format!(
                        "`{}` has no digit after its `.`: a number in `nbt={{...}}` with a \
                         fraction has digits on both sides of the `.`, as `1.0`",
                        &self.src[start..self.pos]
                    ),
                });
            }
        }
        let digits_end = self.pos;
        let suffix = self
            .peek()
            .map(|b| b.to_ascii_lowercase())
            .filter(|b| matches!(b, b'b' | b's' | b'l' | b'f' | b'd'));
        if suffix.is_some() {
            self.pos += 1;
        }
        if let Some(next) = self.peek().filter(|&b| is_unquoted(b)) {
            return Err(Refusal {
                at: start,
                reason: format!(
                    "the number `{}` runs into `{}`: a number in `nbt={{...}}` is digits, a `.` \
                     and digits for a fraction, and a type suffix, with no exponent, and SNBT \
                     reads the whole run as text",
                    &self.src[start..self.pos],
                    char::from(next),
                ),
            });
        }
        let text = &self.src[start..self.pos];
        let tag = match (fraction, suffix) {
            (_, Some(b'f')) => Tag::Float,
            (_, Some(b'd')) | (true, None) => Tag::Double,
            (false, None) => Tag::Int,
            (false, Some(b'b')) => Tag::Byte,
            (false, Some(b's')) => Tag::Short,
            (false, Some(_)) => Tag::Long,
            (true, Some(_)) => {
                return Err(Refusal {
                    at: start,
                    reason: format!(
                        "`{text}` has a fraction and a whole-number suffix: a byte, a short or a \
                         long has no `.`"
                    ),
                });
            }
        };
        let whole = &self.src[whole_start..whole_end];
        if !fraction && whole.len() > 1 && whole.starts_with('0') && tag.is_whole() {
            let trimmed = whole.trim_start_matches('0');
            let fixed = format!(
                "{}{}{}",
                &self.src[start..whole_start],
                if trimmed.is_empty() { "0" } else { trimmed },
                &self.src[digits_end..self.pos],
            );
            return Err(Refusal {
                at: start,
                reason: format!(
                    "`{text}` has a leading zero, and SNBT reads a whole number with one as text: \
                     write `{fixed}`"
                ),
            });
        }
        let digits = &self.src[start..digits_end];
        let in_range = match tag {
            Tag::Byte => digits.parse::<i8>().is_ok(),
            Tag::Short => digits.parse::<i16>().is_ok(),
            Tag::Int => digits.parse::<i32>().is_ok(),
            Tag::Long => digits.parse::<i64>().is_ok(),
            Tag::Float => digits.parse::<f32>().is_ok_and(f32::is_finite),
            _ => digits.parse::<f64>().is_ok_and(f64::is_finite),
        };
        if !in_range {
            return Err(Refusal {
                at: start,
                reason: format!("`{text}` is past the range of {}", range_of(tag)),
            });
        }
        Ok(tag)
    }

    fn list_or_array(&mut self) -> Result<Tag, Refusal> {
        self.enter()?;
        let array = match (self.peek(), self.peek_at(1)) {
            (Some(kind @ (b'B' | b'I' | b'L')), Some(b';')) => {
                self.pos += 2;
                Some(match kind {
                    b'B' => (Tag::ByteArray, Tag::Byte),
                    b'I' => (Tag::IntArray, Tag::Int),
                    _ => (Tag::LongArray, Tag::Long),
                })
            }
            _ => None,
        };
        self.skip_spaces();
        let mut first: Option<Tag> = None;
        if self.peek() == Some(b']') {
            self.pos += 1;
        } else {
            loop {
                let at = self.pos;
                let tag = match array {
                    Some((_, element)) => {
                        let tag = if matches!(self.peek(), Some(b'+' | b'-' | b'0'..=b'9')) {
                            self.number()?
                        } else {
                            return Err(self.expected(element.noun()));
                        };
                        if tag != element {
                            return Err(Refusal {
                                at,
                                reason: format!(
                                    "an array of {kind} holds only {kind}, and this is {}: write \
                                     each as {example}",
                                    tag.noun(),
                                    kind = plural(element),
                                    example = example(element),
                                ),
                            });
                        }
                        tag
                    }
                    None => self.value()?,
                };
                match first {
                    None => first = Some(tag),
                    Some(first) if first != tag => {
                        return Err(Refusal {
                            at,
                            reason: format!(
                                "an NBT list holds values of one type: its first is {}, and this \
                                 one is {}",
                                first.noun(),
                                tag.noun(),
                            ),
                        });
                    }
                    Some(_) => {}
                }
                self.skip_spaces();
                if !self.separator(b']', "values")? {
                    break;
                }
            }
        }
        self.depth -= 1;
        Ok(array.map_or(Tag::List, |(tag, _)| tag))
    }

    /// Read a quoted string from its opening `quote`, returning its text without the escapes.
    fn quoted(&mut self, quote: u8) -> Result<String, Refusal> {
        let open = self.pos;
        let quote = char::from(quote);
        self.pos += 1;
        let mut text = String::new();
        loop {
            match self.src[self.pos..].chars().next() {
                None | Some('\n' | '\r') => {
                    return Err(Refusal {
                        at: open,
                        reason: format!(
                            "unterminated string in `nbt={{...}}`: close it with `{quote}` on \
                             this line"
                        ),
                    });
                }
                Some('\\') => {
                    let escape = self.pos;
                    self.pos += 1;
                    match self.src[self.pos..].chars().next() {
                        Some(c) if c == '\\' || c == quote => {
                            text.push(c);
                            self.pos += 1;
                        }
                        _ => {
                            return Err(Refusal {
                                at: escape,
                                reason: format!(
                                    "a string in `nbt={{...}}` escapes only `\\` and its own \
                                     quote, as `\\\\` and `\\{quote}`"
                                ),
                            });
                        }
                    }
                }
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Ok(text);
                }
                Some(c) => {
                    text.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    /// After an entry or a value: whether a `,` stands before another, or `close` ended the run.
    fn separator(&mut self, close: u8, items: &str) -> Result<bool, Refusal> {
        match self.peek() {
            Some(b',') => {
                let comma = self.pos;
                self.pos += 1;
                self.skip_spaces();
                match self.peek() {
                    Some(b',') => Err(Refusal {
                        at: self.pos,
                        reason: format!(
                            "one comma stands between two {items} in `nbt={{...}}`, and this is \
                             a second: delete it"
                        ),
                    }),
                    Some(b) if b == close => Err(Refusal {
                        at: comma,
                        reason: format!(
                            "a comma in `nbt={{...}}` stands between two {items}, and none follows \
                             this one: delete it"
                        ),
                    }),
                    _ => Ok(true),
                }
            }
            Some(b) if b == close => {
                self.pos += 1;
                Ok(false)
            }
            _ => Err(self.expected(&format!("`,` or `{}`", char::from(close)))),
        }
    }

    /// Open one level of nesting at the current `{` or `[`, and step past it.
    fn enter(&mut self) -> Result<(), Refusal> {
        if self.depth >= MAX_NBT_DEPTH {
            return Err(Refusal {
                at: self.pos,
                reason: format!(
                    "an `nbt={{...}}` value nests at most {MAX_NBT_DEPTH} compounds, lists and arrays \
                     deep"
                ),
            });
        }
        self.depth += 1;
        self.pos += 1;
        Ok(())
    }

    fn expected(&self, what: &str) -> Refusal {
        let reason = match self.src[self.pos..].chars().next() {
            None | Some('\n' | '\r') => format!(
                "expected {what} in `nbt={{...}}`, got end of line: a compound closes on the line \
                 it opens on"
            ),
            Some(found) => format!("expected {what} in `nbt={{...}}`, got `{found}`"),
        };
        Refusal {
            at: self.pos,
            reason,
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
    }

    fn skip_digits(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    fn skip_unquoted(&mut self) {
        while self.peek().is_some_and(is_unquoted) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }
}

/// Whether `b` may stand in an unquoted key or word.
fn is_unquoted(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+')
}

fn range_of(tag: Tag) -> &'static str {
    match tag {
        Tag::Byte => "a byte, -128 to 127",
        Tag::Short => "a short, -32768 to 32767",
        Tag::Int => "an int, -2147483648 to 2147483647",
        Tag::Long => "a long, -9223372036854775808 to 9223372036854775807",
        Tag::Float => "a float",
        _ => "a double",
    }
}

fn plural(element: Tag) -> &'static str {
    match element {
        Tag::Byte => "bytes",
        Tag::Int => "ints",
        _ => "longs",
    }
}

fn example(element: Tag) -> &'static str {
    match element {
        Tag::Byte => "`1b`",
        Tag::Int => "`1`",
        _ => "`1L`",
    }
}
