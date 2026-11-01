//! Where a line ends: `\n`, `\r\n` and a lone `\r` each break a line, unlike `str::lines`.
//! Everything that turns a byte offset into a `line:column` takes the rule from here.

use std::num::NonZeroU32;

use crate::error::Position;

/// Byte length of the line terminator beginning at `offset`, or `None` when none begins there.
#[must_use]
pub fn terminator_len(source: &str, offset: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.get(offset)? {
        b'\r' if bytes.get(offset + 1) == Some(&b'\n') => Some(2),
        b'\r' | b'\n' => Some(1),
        _ => None,
    }
}

/// Byte offset at which each line of `source` begins, including the empty line after a trailing
/// terminator that [`split`] drops, since a cursor can sit on it.
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
#[must_use]
pub fn start_of(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let clamped = offset.min(source.len());
    // The `\n` of a `\r\n` belongs to the line the `\r` ends.
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

/// Byte offset where the line before `next_start`, a nonzero entry of [`starts`], ends: the first
/// byte of its terminator, which `next_start - 1` misses for a `\r\n`.
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

/// Iterate the lines of `source` without terminators, as [`str::lines`] does plus the lone `\r`.
#[must_use]
pub fn split(source: &str) -> Split<'_> {
    Split { rest: source }
}

/// Iterator returned by [`split`].
#[derive(Debug, Clone)]
pub struct Split<'a> {
    /// The tail not yet yielded.
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
        // A trailing terminator closes the last line rather than opening an empty one.
        let body = match trailing_terminator_len(self.rest) {
            Some(len) => &self.rest[..self.rest.len() - len],
            None => self.rest,
        };
        // The break before the line stays in `rest`: it closes the line before.
        let line_start = start_of(body, body.len());
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
fn trailing_terminator_len(source: &str) -> Option<usize> {
    matches!(source.as_bytes().last()?, b'\r' | b'\n')
        .then(|| source.len() - end_before(source, source.len()))
}

/// Precomputed line-start byte offsets for a source string.
#[derive(Debug, Clone)]
pub struct LineStarts {
    starts: Vec<usize>,
}

impl LineStarts {
    /// Index the line starts of `source`.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            starts: starts(source),
        }
    }

    /// The byte offset each line begins at, as [`crate::lines::starts`] computes them.
    #[must_use]
    pub fn line_starts(&self) -> &[usize] {
        &self.starts
    }

    /// Resolve a byte offset, on a character boundary, into a 1-based `line:column` [`Position`],
    /// clamping one past the end.
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

    /// Resolve a 1-based `line:column` back to a byte offset, clamping a line or column past the
    /// end to the end of the source or the line.
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

    /// Byte offset of the end of the line `byte_offset` sits on, before its terminator.
    #[must_use]
    pub fn line_end(&self, source: &str, byte_offset: usize) -> usize {
        let clamped = byte_offset.min(source.len());
        self.starts
            .get(self.line_of(clamped) + 1)
            .map_or(source.len(), |&next| end_before(source, next))
    }

    /// The 0-based index of the line `byte_offset` sits on; past the end is the last line, and the
    /// `\n` of a `\r\n` is on the line the pair ends.
    #[must_use]
    pub fn line_of(&self, byte_offset: usize) -> usize {
        // Never underflows: `starts[0]` is 0, which is `<=` every offset.
        self.starts.partition_point(|&s| s <= byte_offset) - 1
    }
}

/// Compute a 1-based `line:column` for a byte offset into `source`; hold a [`LineStarts`] for many.
#[must_use]
pub fn position_at(source: &str, byte_offset: usize) -> Position {
    LineStarts::new(source).position(source, byte_offset)
}

#[cfg(test)]
mod tests;
