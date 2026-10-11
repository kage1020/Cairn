//! Where a line ends.
//!
//! Cairn takes `\n`, `\r\n` and a lone `\r` each as one line break, so a file lexes the same
//! whatever platform wrote it. `str::lines` does not: it leaves a lone `\r` inside a line.
//!
//! Every layer that turns a byte offset into a `line:column` (the lexer, [`LineStarts`], the
//! language server's `LineIndex` and its completion scanner) takes the rule from here, because a
//! diagnostic's position is compared against an editor's cursor. The tree-sitter grammar cannot:
//! its runtime advances the row on `\n` only.

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
/// Entry 0 is always 0, so an empty source is one empty line, and a source ending in a terminator
/// gets a final entry at `source.len()`. Both match what an editor's cursor reports.
///
/// [`split`] follows `str::lines` instead and drops that trailing empty line. Positions resolve
/// against this one, because a cursor can sit on that line.
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
/// Agrees with [`starts`] plus a binary search, without allocating; prefer the index for many
/// offsets.
#[must_use]
pub fn start_of(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let clamped = offset.min(source.len());
    // An offset on the `\n` of a `\r\n` is inside the break, so on the line before the `\r`.
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

/// Byte offset at which the line preceding `next_start` ends: the first byte of the terminator
/// that `next_start` follows.
///
/// `next_start` must be a nonzero entry of [`starts`]. Use this rather than `next_start - 1`,
/// which leaves the `\r` of a `\r\n` pair inside the line.
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
/// Same contract as [`str::lines`] (no line for an empty source, no empty line after a trailing
/// terminator), plus the lone `\r` break. See [`starts`] for why the two differ on that trailing
/// line.
#[must_use]
pub fn split(source: &str) -> Split<'_> {
    Split { rest: source }
}

/// Iterator returned by [`split`].
#[derive(Debug, Clone)]
pub struct Split<'a> {
    /// The tail not yet yielded. Empty means exhausted, which is why a trailing break yields no
    /// empty line.
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
        // A terminator at the very end closes the last line rather than opening an empty one; a
        // forward walk gets that from running out of input.
        let body = match trailing_terminator_len(self.rest) {
            Some(len) => &self.rest[..self.rest.len() - len],
            None => self.rest,
        };
        // The break before the line stays in `rest`, because it closes the line before it;
        // dropping it would swallow an empty line between two breaks.
        let line_start = start_of(body, body.len());
        // The tail shrinks only while `trailing_terminator_len` and `start_of` agree on what a
        // terminator is. If they ever disagree this walk would hang, so it panics instead.
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
/// Defers to [`end_before`] for how far back the terminator begins, rather than restating the
/// rule.
fn trailing_terminator_len(source: &str) -> Option<usize> {
    matches!(source.as_bytes().last()?, b'\r' | b'\n')
        .then(|| source.len() - end_before(source, source.len()))
}

/// Precomputed line-start byte offsets for a source string.
///
/// Build once, then resolve many offsets: `O(file_len + N * log L)` for `N` offsets over `L`
/// lines, rather than a walk of the source per offset.
#[derive(Debug, Clone)]
pub struct LineStarts {
    /// As [`starts`] computes them.
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

    /// The byte offset each line begins at, as [`crate::lines::starts`] computes them.
    ///
    /// For a caller that wants a line's text, so it does not walk the source again deciding where
    /// lines end.
    #[must_use]
    pub fn line_starts(&self) -> &[usize] {
        &self.starts
    }

    /// Resolve a byte offset into a 1-based `line:column` [`Position`].
    ///
    /// `byte_offset` must be a character boundary, as every [`Span`](crate::error::Span) bound
    /// is. An offset past the end of `source` clamps to the final position; one inside a
    /// character panics on the slice. Line and column saturate at `u32::MAX`.
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

    /// Resolve a 1-based `line:column` back to a byte offset, the inverse of
    /// [`LineStarts::position`].
    ///
    /// Out-of-range input, such as a position from a different source, clamps: a line past the
    /// end gives the end of the source, and a column past the end of its line gives that line's
    /// end before its terminator.
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
    /// Excludes the terminator, so a span ending here does not underline the next row.
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
        // Never underflows: `starts[0]` is 0, which is `<=` every offset.
        self.starts.partition_point(|&s| s <= byte_offset) - 1
    }
}

/// Compute a 1-based `line:column` for a byte offset into `source`.
///
/// A one-shot [`LineStarts`], so the two cannot disagree on where a line ends. Builds an index per
/// call; hold a [`LineStarts`] to convert many offsets.
#[must_use]
pub fn position_at(source: &str, byte_offset: usize) -> Position {
    LineStarts::new(source).position(source, byte_offset)
}

#[cfg(test)]
mod tests;
