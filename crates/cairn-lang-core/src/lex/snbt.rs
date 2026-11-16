//! An NBT compound: the subset of Minecraft's SNBT that every Java version Cairn targets reads
//! alike (`spec/syntax` "Literals and separators"). The lexer checks it here and keeps it as
//! written; nothing in core reads what it holds.

use std::collections::HashSet;
use std::ops::Range;

/// Deepest nesting of compounds, lists and arrays an NBT compound holds, itself included.
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
    Number(Number),
    String,
    List,
    Compound,
    Array(ArrayKind),
}

impl Tag {
    /// The type as a noun phrase, with its article.
    fn noun(self) -> &'static str {
        match self {
            Self::Number(number) => number.noun(),
            Self::String => "a string",
            Self::List => "a list",
            Self::Compound => "a compound",
            Self::Array(kind) => kind.noun(),
        }
    }
}

/// The type of an NBT number, which its suffix and its `.` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Number {
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
}

impl Number {
    /// The type as a noun phrase, with its article.
    fn noun(self) -> &'static str {
        match self {
            Self::Byte => "a byte",
            Self::Short => "a short",
            Self::Int => "an int",
            Self::Long => "a long",
            Self::Float => "a float",
            Self::Double => "a double",
        }
    }

    /// Whether the type holds whole numbers only.
    fn is_whole(self) -> bool {
        matches!(self, Self::Byte | Self::Short | Self::Int | Self::Long)
    }

    /// The type's range, as a refusal names it.
    fn range(self) -> &'static str {
        match self {
            Self::Byte => "-128 to 127",
            Self::Short => "-32768 to 32767",
            Self::Int => "-2147483648 to 2147483647",
            Self::Long => "-9223372036854775808 to 9223372036854775807",
            Self::Float => "about -3.4e38 to 3.4e38",
            Self::Double => "about -1.8e308 to 1.8e308",
        }
    }

    /// Whether `digits`, a sign, digits and any fraction, is inside the type's range.
    fn holds(self, digits: &str) -> bool {
        match self {
            Self::Byte => digits.parse::<i8>().is_ok(),
            Self::Short => digits.parse::<i16>().is_ok(),
            Self::Int => digits.parse::<i32>().is_ok(),
            Self::Long => digits.parse::<i64>().is_ok(),
            Self::Float => digits.parse::<f32>().is_ok_and(f32::is_finite),
            Self::Double => digits.parse::<f64>().is_ok_and(f64::is_finite),
        }
    }

    /// The wider types a number past this type's range may take instead, each with its suffix.
    fn wider(self) -> &'static [(Self, &'static str)] {
        match self {
            Self::Byte => &[(Self::Short, "s"), (Self::Int, ""), (Self::Long, "L")],
            Self::Short => &[(Self::Int, ""), (Self::Long, "L")],
            Self::Int => &[(Self::Long, "L")],
            Self::Float => &[(Self::Double, "d")],
            Self::Long | Self::Double => &[],
        }
    }
}

/// The type an array's `[B;`, `[I;` or `[L;` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayKind {
    Byte,
    Int,
    Long,
}

impl ArrayKind {
    /// The kind a type letter names.
    fn of_letter(letter: u8) -> Option<Self> {
        match letter {
            b'B' => Some(Self::Byte),
            b'I' => Some(Self::Int),
            b'L' => Some(Self::Long),
            _ => None,
        }
    }

    /// The type of each value the array holds.
    fn element(self) -> Number {
        match self {
            Self::Byte => Number::Byte,
            Self::Int => Number::Int,
            Self::Long => Number::Long,
        }
    }

    /// The array as a noun phrase, with its article.
    fn noun(self) -> &'static str {
        match self {
            Self::Byte => "a byte array",
            Self::Int => "an int array",
            Self::Long => "a long array",
        }
    }

    /// The values the array holds, as a plural noun.
    fn plural(self) -> &'static str {
        match self {
            Self::Byte => "bytes",
            Self::Int => "ints",
            Self::Long => "longs",
        }
    }

    /// The suffix each value the array holds is written with.
    fn suffix(self) -> &'static str {
        match self {
            Self::Byte => "b",
            Self::Int => "",
            Self::Long => "L",
        }
    }
}

/// Where the parts of a number in an NBT compound fall in the source.
struct NumberParts {
    /// Where its sign, or its first digit when it has none, is.
    start: usize,
    /// Its digits before any `.`.
    whole: Range<usize>,
    /// Where its digits end, those of its fraction included.
    digits_end: usize,
    /// Whether it has a `.` and digits after it.
    fraction: bool,
    /// Its type suffix, lowercased.
    suffix: Option<u8>,
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
        let mut keys = HashSet::new();
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
                keys.insert(key);
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
            _ if self.at_number() => self.number().map(Tag::Number),
            Some(b) if is_unquoted(b) => self.word().map(Tag::Number),
            _ => Err(self.expected("a value")),
        }
    }

    /// Whether a number starts here: a sign, a digit, or a `.` before a digit.
    fn at_number(&self) -> bool {
        match self.peek() {
            Some(b'+' | b'-' | b'0'..=b'9') => true,
            Some(b'.') => self.peek_at(1).is_some_and(|b| b.is_ascii_digit()),
            _ => false,
        }
    }

    /// An unquoted word in value position, which only `true` and `false` may be.
    fn word(&mut self) -> Result<Number, Refusal> {
        let start = self.pos;
        self.skip_unquoted();
        match &self.src[start..self.pos] {
            "true" | "false" => Ok(Number::Byte),
            word => Err(Refusal {
                at: start,
                reason: format!(
                    "`{word}` is not quoted: an NBT compound reads a string only where it is \
                     quoted, and no unquoted word but `true` and `false`: write `\"{word}\"`"
                ),
            }),
        }
    }

    /// A number, typed by its suffix and by whether it has a fraction.
    fn number(&mut self) -> Result<Number, Refusal> {
        let parts = self.number_parts()?;
        self.number_type(&parts)
    }

    /// The parts of the number here, refused when they do not make one.
    fn number_parts(&mut self) -> Result<NumberParts, Refusal> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let whole_start = self.pos;
        self.skip_digits();
        let whole = whole_start..self.pos;
        if whole.is_empty() {
            if self.peek() != Some(b'.') || !self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
                return Err(self.expected("a digit"));
            }
            self.pos += 1;
            self.skip_digits();
            let fraction = &self.src[whole_start..self.pos];
            let suffix_start = self.pos;
            self.suffix();
            let (sign, suffix) = (
                &self.src[start..whole_start],
                &self.src[suffix_start..self.pos],
            );
            return Err(Refusal {
                at: start,
                reason: format!(
                    "`{sign}{fraction}{suffix}` has no digit before its `.`: a number in an NBT \
                     compound with a fraction has digits on both sides of the `.`, as \
                     `{sign}0{fraction}{suffix}`"
                ),
            });
        }
        let fraction = self.peek() == Some(b'.');
        if fraction {
            self.pos += 1;
            let fraction_start = self.pos;
            self.skip_digits();
            if self.pos == fraction_start {
                self.suffix();
                let written = &self.src[start..fraction_start];
                let suffix = &self.src[fraction_start..self.pos];
                return Err(Refusal {
                    at: start,
                    reason: format!(
                        "`{written}{suffix}` has no digit after its `.`: a number in an NBT \
                         compound with a fraction has digits on both sides of the `.`, as \
                         `{written}0{suffix}`"
                    ),
                });
            }
        }
        let digits_end = self.pos;
        let suffix = self.suffix();
        if let Some(next) = self.peek().filter(|&b| is_unquoted(b)) {
            let number_end = self.pos;
            self.skip_unquoted();
            return Err(Refusal {
                at: start,
                reason: format!(
                    "the number `{}` runs into `{}`: a number in an NBT compound is digits, a `.` \
                     and digits for a fraction, and a type suffix, the one form every Java \
                     version Cairn targets reads alike: write it in that form, or as `\"{}\"` if \
                     it is text",
                    &self.src[start..number_end],
                    char::from(next),
                    &self.src[start..self.pos],
                ),
            });
        }
        Ok(NumberParts {
            start,
            whole,
            digits_end,
            fraction,
            suffix,
        })
    }

    /// The type suffix here, lowercased and read past, or `None` when there is none.
    fn suffix(&mut self) -> Option<u8> {
        let suffix = self
            .peek()
            .map(|b| b.to_ascii_lowercase())
            .filter(|b| matches!(b, b'b' | b's' | b'l' | b'f' | b'd'));
        if suffix.is_some() {
            self.pos += 1;
        }
        suffix
    }

    /// The type `parts` writes, which ends where the scanner is, refused when it cannot hold them.
    fn number_type(&self, parts: &NumberParts) -> Result<Number, Refusal> {
        let NumberParts {
            start,
            ref whole,
            digits_end,
            fraction,
            suffix,
        } = *parts;
        let text = &self.src[start..self.pos];
        let sign = &self.src[start..whole.start];
        let written_suffix = &self.src[digits_end..self.pos];
        let number = match (fraction, suffix) {
            (_, Some(b'f')) => Number::Float,
            (_, Some(b'd')) | (true, None) => Number::Double,
            (false, None) => Number::Int,
            (false, Some(b'b')) => Number::Byte,
            (false, Some(b's')) => Number::Short,
            (false, Some(_)) => Number::Long,
            (true, Some(_)) => {
                return Err(Refusal {
                    at: start,
                    reason: format!(
                        "`{text}` has a fraction and a whole-number suffix, and a byte, a short \
                         or a long has no `.`: write `{sign}{}{written_suffix}` for a whole \
                         number, or `{}` for a double",
                        &self.src[whole.clone()],
                        &self.src[start..digits_end],
                    ),
                });
            }
        };
        let whole = &self.src[whole.clone()];
        if number.is_whole() && whole.len() > 1 && whole.starts_with('0') {
            let trimmed = whole.trim_start_matches('0');
            let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
            return Err(Refusal {
                at: start,
                reason: format!(
                    "`{text}` has a leading zero, which a whole number in an NBT compound does \
                     not: write `{sign}{trimmed}{written_suffix}`"
                ),
            });
        }
        let digits = &self.src[start..digits_end];
        if !number.holds(digits) {
            let wider = number
                .wider()
                .iter()
                .find(|(wider, _)| wider.holds(digits))
                .map_or_else(String::new, |(wider, suffix)| {
                    format!(", or `{digits}{suffix}` for {}", wider.noun())
                });
            return Err(Refusal {
                at: start,
                reason: format!(
                    "`{text}` is past the range of {}, {}: write a value inside it{wider}",
                    number.noun(),
                    number.range(),
                ),
            });
        }
        Ok(number)
    }

    fn list_or_array(&mut self) -> Result<Tag, Refusal> {
        let open = self.pos;
        self.enter()?;
        let array = self.array_kind(open)?;
        self.skip_spaces();
        let mut first: Option<Tag> = None;
        if self.peek() == Some(b']') {
            self.pos += 1;
        } else {
            loop {
                let at = self.pos;
                let tag = match array {
                    Some(kind) => Tag::Number(self.array_value(kind)?),
                    None => self.value()?,
                };
                match first {
                    None => first = Some(tag),
                    Some(first) if first != tag => {
                        return Err(Refusal {
                            at,
                            reason: format!(
                                "an NBT list holds values of one type: its first is {first}, and \
                                 this one is {}: write each value in it as {first}",
                                tag.noun(),
                                first = first.noun(),
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
        Ok(array.map_or(Tag::List, Tag::Array))
    }

    /// Past the `[` at `open`, step over the `B;`, `I;` or `L;` of an array, if one opens there.
    fn array_kind(&mut self, open: usize) -> Result<Option<ArrayKind>, Refusal> {
        let letter_at = self.pos + self.spaces_at(self.pos);
        let Some(kind) = self
            .bytes
            .get(letter_at)
            .copied()
            .and_then(ArrayKind::of_letter)
        else {
            return Ok(None);
        };
        let semi_at = letter_at + 1 + self.spaces_at(letter_at + 1);
        if self.bytes.get(semi_at) != Some(&b';') {
            return Ok(None);
        }
        if semi_at != open + 2 {
            let letter = char::from(self.bytes[letter_at]);
            return Err(Refusal {
                at: open,
                reason: format!(
                    "`{}` opens an NBT array with a space among its `[`, `{letter}` and `;`, \
                     which an array keeps together: write `[{letter};`",
                    &self.src[open..=semi_at],
                ),
            });
        }
        self.pos = semi_at + 1;
        Ok(Some(kind))
    }

    /// One value of an array of `kind`: a number of the type it holds.
    fn array_value(&mut self, kind: ArrayKind) -> Result<Number, Refusal> {
        let at = self.pos;
        let plural = kind.plural();
        let suffix = kind.suffix();
        if self.at_number() {
            let number = self.number()?;
            if number != kind.element() {
                return Err(Refusal {
                    at,
                    reason: format!(
                        "an array of {plural} holds only {plural}, and this is {}: write each as \
                         `1{suffix}`",
                        number.noun(),
                    ),
                });
            }
            return Ok(number);
        }
        if !self.peek().is_some_and(is_unquoted) {
            return Err(self.expected(kind.element().noun()));
        }
        self.skip_unquoted();
        let word = &self.src[at..self.pos];
        let fixed = match word {
            "true" => format!("`1{suffix}`"),
            "false" => format!("`0{suffix}`"),
            _ => format!("each as `1{suffix}`"),
        };
        Err(Refusal {
            at,
            reason: format!(
                "an array of {plural} holds each value as a number, and `{word}` is not one: \
                 write {fixed}"
            ),
        })
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
                            "unterminated string in an NBT compound: close it with `{quote}` on \
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
                                    "a string in an NBT compound escapes only `\\` and its own \
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
                            "one comma stands between two {items} in an NBT compound, and this is \
                             a second: delete it"
                        ),
                    }),
                    Some(b) if b == close => Err(Refusal {
                        at: comma,
                        reason: format!(
                            "a comma in an NBT compound stands between two {items}, and none \
                             follows this one: delete it"
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
                    "an NBT compound nests compounds, lists and arrays at most {MAX_NBT_DEPTH} \
                     deep, itself included: flatten the levels past the {MAX_NBT_DEPTH}th"
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
                "expected {what} in an NBT compound, got end of line: a compound closes on the \
                 line it opens on"
            ),
            Some(found) => format!("expected {what} in an NBT compound, got `{found}`"),
        };
        Refusal {
            at: self.pos,
            reason,
        }
    }

    /// How many spaces stand from byte `at` on.
    fn spaces_at(&self, at: usize) -> usize {
        self.bytes
            .get(at..)
            .map_or(0, |rest| rest.iter().take_while(|&&b| b == b' ').count())
    }

    fn skip_spaces(&mut self) {
        self.pos += self.spaces_at(self.pos);
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
