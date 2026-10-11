//! Where a line ends.
//!
//! Cairn accepts `\n`, `\r\n`, and a lone `\r` as one logical line break, so
//! a file written on Windows lexes the same as one written on Linux. That
//! rule is not the one Rust's standard library implements: `str::lines`
//! splits on `\n` and `\r\n` but carries a lone `\r` into the middle of a
//! line, and a hand-rolled `for byte in source.bytes()` loop that pushes on
//! `b'\n'` misses it entirely.
//!
//! Every layer that turns a byte offset into a `line:column` — the lexer as
//! it walks, [`LineStarts`] from a span, `LineIndex` in the
//! language server, and the completion scanner — has to agree, because a
//! diagnostic's position is compared against an editor's cursor. They agree
//! by taking the rule from here rather than each re-deciding it.
//!
//! The one implementation this cannot reach is the tree-sitter grammar: its
//! `Point.row` comes from the tree-sitter runtime's own lexer, which
//! advances the row on `\n` only, and an external scanner cannot override
//! it.

use std::num::NonZeroU32;

use crate::error::Position;

/// Byte length of the line terminator beginning at `offset` — 2 for a
/// `\r\n` pair, 1 for a lone `\r` or a `\n` — or `None` when no terminator
/// begins there, including when `offset` is past the end of `source`.
///
/// Byte indexing is sound for this question: `\r` and `\n` are ASCII, and
/// no byte of a multi-byte UTF-8 sequence is ASCII.
#[must_use]
pub fn terminator_len(source: &str, offset: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.get(offset)? {
        b'\r' if bytes.get(offset + 1) == Some(&b'\n') => Some(2),
        b'\r' | b'\n' => Some(1),
        _ => None,
    }
}

/// Byte offset at which each line of `source` begins.
///
/// Entry 0 is always 0, so an empty source is one empty line. A source that
/// ends in a terminator gets a final entry at `source.len()`: the text after
/// the last break is a line, even when it is empty. Both conventions match
/// what a cursor at the end of such a file reports in an editor.
///
/// This differs from [`split`], which follows `str::lines` in *not* yielding
/// that trailing empty line, so `split(s).count() + 1 == starts(s).len()`
/// whenever `s` ends in a terminator. Positions are resolved against this
/// one: a cursor can sit on the empty line after the last break, and an
/// offset there has to name a line.
#[must_use]
pub fn starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut offset = 0;
    while offset < source.len() {
        match terminator_len(source, offset) {
            Some(len) => {
                offset += len;
                starts.push(offset);
            }
            None => offset += 1,
        }
    }
    starts
}

/// Byte offset at which the line containing `offset` begins.
///
/// Agrees with [`starts`] plus a binary search, and allocates nothing. Both
/// walk the source once, so prefer the index when many offsets are in
/// question and this when one is.
#[must_use]
pub fn start_of(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let clamped = offset.min(source.len());
    // An offset sitting on the `\n` of a `\r\n` is *inside* one break, not
    // after it. The line that break ends begins one byte further on, so the
    // line holding this offset is the one before the `\r`.
    let search_end =
        if clamped > 0 && bytes[clamped - 1] == b'\r' && bytes.get(clamped) == Some(&b'\n') {
            clamped - 1
        } else {
            clamped
        };
    bytes[..search_end]
        .iter()
        .rposition(|b| matches!(b, b'\r' | b'\n'))
        .map_or(0, |i| i + 1)
}

/// Byte offset at which the line preceding `next_start` ends: the first
/// byte of the terminator that `next_start` follows.
///
/// `next_start` must be a line start other than 0 — one of [`starts`]'
/// entries — which is exactly the condition under which a terminator
/// precedes it. Callers that hold an index of line starts want this rather
/// than `next_start - 1`: that is the terminator's *last* byte, and for a
/// `\r\n` pair the `\r` belongs to the break too, so a range built from it
/// would cover a carriage return.
///
/// Stepping back by hand is where the rule gets re-decided. The version
/// this replaced asked whether the byte before the end was a `\r`, which is
/// also true of the *previous* line's terminator when this line is empty —
/// answering with an offset that is not on this line at all, and, for a
/// source beginning with `\n`, underflowing before it could.
#[must_use]
pub fn end_before(source: &str, next_start: usize) -> usize {
    debug_assert!(
        next_start >= 1 && next_start <= source.len(),
        "next_start must be a line start after a terminator",
    );
    let bytes = source.as_bytes();
    if next_start >= 2 && bytes[next_start - 2..next_start] == *b"\r\n" {
        next_start - 2
    } else {
        next_start - 1
    }
}

/// Iterate the lines of `source` with their terminators removed.
///
/// Same contract as [`str::lines`] — no line for an empty source, and a
/// trailing terminator does not produce a trailing empty line — extended
/// with the lone `\r` that `str::lines` does not treat as a break. See
/// [`starts`] for why the two disagree about that trailing line.
#[must_use]
pub fn split(source: &str) -> Split<'_> {
    Split { rest: source }
}

/// Iterator returned by [`split`].
#[derive(Debug, Clone)]
pub struct Split<'a> {
    /// The not-yet-yielded tail, terminator of the previous line already
    /// consumed. Empty means exhausted, which is why the final line of a
    /// source ending in a break is not yielded.
    rest: &'a str,
}

impl<'a> Iterator for Split<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.rest.is_empty() {
            return None;
        }
        for offset in 0..self.rest.len() {
            if let Some(len) = terminator_len(self.rest, offset) {
                let line = &self.rest[..offset];
                self.rest = &self.rest[offset + len..];
                return Some(line);
            }
        }
        Some(std::mem::take(&mut self.rest))
    }
}

impl<'a> DoubleEndedIterator for Split<'a> {
    fn next_back(&mut self) -> Option<&'a str> {
        if self.rest.is_empty() {
            return None;
        }
        // A terminator at the very end closes the last line rather than
        // opening an empty one — the half of `str::lines`' contract that a
        // reverse walk has to reproduce, since a forward walk gets it from
        // running out of input.
        let body = match trailing_terminator_len(self.rest) {
            Some(len) => &self.rest[..self.rest.len() - len],
            None => self.rest,
        };
        // Everything after the last break in `body` is the line. Everything
        // up to and including that break stays in `rest`, because the break
        // is what closes the line before it — dropping it here would
        // swallow an empty line between two breaks. When `body` holds no
        // break at all, `start_of` answers 0 and both halves fall out: the
        // whole of `body` is the line and nothing remains.
        let line_start = start_of(body, body.len());
        // `line_start` is at most `body.len()`, and equals it only if
        // `body` ends in a break — which `trailing_terminator_len` has
        // already removed. So the tail always shrinks. That rests on the
        // two saying the same thing about what a terminator is; should they
        // ever stop, this walk stops consuming input, and a hang is a far
        // worse way to find that out than a panic.
        debug_assert!(
            line_start < self.rest.len(),
            "the reverse walk must consume input: {:?} yielded nothing",
            self.rest,
        );
        self.rest = &self.rest[..line_start];
        Some(&body[line_start..])
    }
}

impl std::iter::FusedIterator for Split<'_> {}

/// Length of the terminator `source` ends with, if it ends with one.
///
/// Defers the pair's shape to [`end_before`] rather than restating it: the
/// last byte says whether a terminator ends here, and the shared rule says
/// how far back it began.
fn trailing_terminator_len(source: &str) -> Option<usize> {
    matches!(source.as_bytes().last()?, b'\r' | b'\n')
        .then(|| source.len() - end_before(source, source.len()))
}

/// Precomputed line-start byte offsets for a source string.
///
/// Construct once with [`LineStarts::new`], then look up many byte offsets
/// via [`LineStarts::position`]. Reduces an N-diagnostic conversion from
/// `O(N * file_len)` (re-walking the source per call) to
/// `O(file_len + N * log L)` where `L` is the line count.
#[derive(Debug, Clone)]
pub struct LineStarts {
    /// Byte offset of the first character of each line (line 1 starts at
    /// offset 0; subsequent entries are the byte after each line break).
    starts: Vec<usize>,
}

impl LineStarts {
    /// Build the index by walking the source exactly once.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            starts: starts(source),
        }
    }

    /// The byte offset each line begins at, as [`crate::lines::starts`]
    /// computed them.
    ///
    /// For a caller that wants the *text* of a line rather than a position
    /// in it, and already holds this index. Handing back the slice keeps
    /// the line-break rule where `crate::lines` puts it: the alternative
    /// is a second walk of the source deciding again where a line ends,
    /// which is what that module exists to prevent.
    #[must_use]
    pub fn line_starts(&self) -> &[usize] {
        &self.starts
    }

    /// Resolve a byte offset into a 1-based `line:column` [`Position`].
    ///
    /// `byte_offset` must be a character boundary — every offset in this
    /// crate comes from a [`Span`](crate::error::Span), which is built from
    /// one. Offsets past the end of `source` clamp to the final position; an
    /// offset inside a character does not, and panics on the slice.
    ///
    /// Line and column saturate at `u32::MAX` rather than wrapping. Each
    /// needs its own four billion — of lines in the file, of characters on
    /// the line — and a source that reaches either has worse problems than
    /// a truncated position.
    #[must_use]
    pub fn position(&self, source: &str, byte_offset: usize) -> Position {
        let clamped = byte_offset.min(source.len());
        let line_index = self.line_of(clamped);
        let line_start = self.starts[line_index];
        let column_chars = source[line_start..clamped].chars().count() + 1;
        let line = NonZeroU32::new(u32::try_from(line_index + 1).unwrap_or(u32::MAX))
            .unwrap_or(NonZeroU32::MIN);
        let col = NonZeroU32::new(u32::try_from(column_chars).unwrap_or(u32::MAX))
            .unwrap_or(NonZeroU32::MIN);
        Position { line, col }
    }

    /// Resolve a 1-based `line:column` back to a byte offset — the inverse
    /// of [`LineStarts::position`].
    ///
    /// Needed by anything holding a [`Position`] and wanting a
    /// [`Span`](crate::error::Span): the parser reports where it stopped,
    /// not the range it stopped over, so a renderer has to put the two back
    /// together.
    ///
    /// Out-of-range inputs clamp rather than panic, which is what a
    /// position from a *different* source than the one indexed here
    /// amounts to. A line past the end resolves to the end of the source;
    /// a column past the end of its line resolves to that line's end,
    /// before the terminator, so the result never lands inside a line
    /// break or past one.
    #[must_use]
    pub fn offset_of(&self, source: &str, position: Position) -> usize {
        let line_index = position.line.get() as usize - 1;
        let Some(&line_start) = self.starts.get(line_index) else {
            return source.len();
        };
        let line_end = self.line_end(source, line_start);
        let mut offset = line_start;
        for _ in 1..position.col.get() {
            let Some(c) = source[offset..line_end].chars().next() else {
                break;
            };
            offset += c.len_utf8();
        }
        offset
    }

    /// Byte offset of the end of the line `byte_offset` sits on, before
    /// that line's terminator.
    ///
    /// The terminator is excluded so a span built from this cannot cross
    /// into the next line — an editor rendering one that did would
    /// underline the following row's first column.
    #[must_use]
    pub fn line_end(&self, source: &str, byte_offset: usize) -> usize {
        let clamped = byte_offset.min(source.len());
        self.starts
            .get(self.line_of(clamped) + 1)
            .map_or(source.len(), |&next| end_before(source, next))
    }

    /// The 0-based index of the line `byte_offset` sits on.
    ///
    /// An offset past the end of the indexed source lands on its last line,
    /// the one [`LineStarts::position`] clamps such an offset to. An offset
    /// on the `\n` of a `\r\n` pair is on the line that pair ends, since
    /// the break is one terminator and the next line begins after it.
    #[must_use]
    pub fn line_of(&self, byte_offset: usize) -> usize {
        // `partition_point` counts the line starts at or before the offset,
        // and the line holding it is the last of those. The count is never
        // 0: `starts[0]` is 0, which is `<= byte_offset` for every offset.
        self.starts.partition_point(|&s| s <= byte_offset) - 1
    }
}

/// Compute a 1-based `line:column` for a byte offset into `source`.
///
/// The one-shot convenience over [`LineStarts`], not a second answer to the
/// same question — computing the line here independently is how the two
/// came to disagree about a lone `\r`. It builds and discards an index per
/// call, so prefer holding one when converting many offsets from the same
/// source; the O(`source.len()`) cost per call is unchanged.
#[must_use]
pub fn position_at(source: &str, byte_offset: usize) -> Position {
    LineStarts::new(source).position(source, byte_offset)
}

#[cfg(test)]
mod tests;
