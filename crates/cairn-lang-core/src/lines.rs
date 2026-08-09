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
//! it walks, [`crate::check::LineStarts`] from a span, `LineIndex` in the
//! language server, and the completion scanner — has to agree, because a
//! diagnostic's position is compared against an editor's cursor. They agree
//! by taking the rule from here rather than each re-deciding it.
//!
//! The one implementation this cannot reach is the tree-sitter grammar: its
//! `Point.row` comes from the tree-sitter runtime's own lexer, which
//! advances the row on `\n` only, and an external scanner cannot override
//! it.

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
/// The cheap answer when only one offset is in question; [`starts`] plus a
/// binary search is the cheaper one for many, and the two agree.
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

/// Iterate the lines of `source` with their terminators removed.
///
/// Same contract as [`str::lines`] — no line for an empty source, and a
/// trailing terminator does not produce a trailing empty line — extended
/// with the lone `\r` that `str::lines` does not treat as a break.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Sources whose lines Rust already agrees about. `split` has to match
    /// `str::lines` on all of them, or replacing one with the other changes
    /// behaviour nobody asked to change.
    const AGREED: &[&str] = &[
        "",
        "a",
        "a\n",
        "a\nb",
        "a\nb\n",
        "\n",
        "a\n\nb\n",
        "a\r\nb\r\n",
        "a\r\n\r\nb",
        "\r\n",
    ];

    #[test]
    fn terminator_len_names_each_of_the_three_breaks() {
        assert_eq!(terminator_len("a\r\nb", 1), Some(2));
        assert_eq!(terminator_len("a\rb", 1), Some(1));
        assert_eq!(terminator_len("a\nb", 1), Some(1));
        assert_eq!(terminator_len("a\r\nb", 0), None);
        // Past the end is not a terminator, so callers can ask about any
        // offset without a bounds check of their own.
        assert_eq!(terminator_len("a", 1), None);
        assert_eq!(terminator_len("a", 99), None);
        // The `\n` of a `\r\n` is the pair's second byte, not a break of
        // its own — asking about it would double-count the line.
        assert_eq!(terminator_len("a\r\nb", 2), Some(1));
    }

    #[test]
    fn starts_a_new_line_after_every_break() {
        assert_eq!(starts(""), vec![0]);
        assert_eq!(starts("ab"), vec![0]);
        assert_eq!(starts("ab\ncd"), vec![0, 3]);
        assert_eq!(starts("ab\r\ncd"), vec![0, 4]);
        assert_eq!(starts("ab\rcd"), vec![0, 3]);
        // A trailing break opens a final empty line.
        assert_eq!(starts("ab\r"), vec![0, 3]);
        // Consecutive breaks of different kinds each open one line.
        assert_eq!(starts("\r\r\n\n"), vec![0, 1, 3, 4]);
    }

    #[test]
    fn split_matches_str_lines_wherever_str_lines_is_right() {
        for source in AGREED {
            assert_eq!(
                split(source).collect::<Vec<_>>(),
                source.lines().collect::<Vec<_>>(),
                "{source:?}",
            );
        }
    }

    #[test]
    fn split_also_breaks_at_a_lone_carriage_return() {
        assert_eq!(split("a\rb\r").collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(split("a\r\rb").collect::<Vec<_>>(), vec!["a", "", "b"]);
        // The case that motivates the module: `str::lines` yields one line
        // here, so a scanner built on it sees `floor walls` as one command.
        assert_eq!(
            split("floor\rwalls").collect::<Vec<_>>(),
            vec!["floor", "walls"]
        );
    }

    #[test]
    fn start_of_agrees_with_the_index_at_every_offset() {
        for source in AGREED
            .iter()
            .copied()
            .chain(["a\rb\r", "a\r\rb", "\r\r\n", "x\r\ny"])
        {
            let index = starts(source);
            for offset in 0..=source.len() {
                let from_index = index[index.partition_point(|&s| s <= offset) - 1];
                assert_eq!(
                    start_of(source, offset),
                    from_index,
                    "{source:?} at {offset}"
                );
            }
        }
    }

    #[test]
    fn split_and_starts_describe_the_same_lines() {
        for source in ["a\rb\r\nc\nd", "\r\n\r\r\n", "x", ""] {
            let from_starts: Vec<&str> = starts(source)
                .iter()
                .map(|&start| {
                    let end = source[start..]
                        .char_indices()
                        .find(|(i, _)| terminator_len(source, start + i).is_some())
                        .map_or(source.len(), |(i, _)| start + i);
                    &source[start..end]
                })
                .collect();
            // `starts` keeps the trailing empty line that `split` drops, so
            // the comparison is against the prefix.
            let split_lines: Vec<&str> = split(source).collect();
            assert_eq!(
                from_starts[..split_lines.len()],
                split_lines[..],
                "{source:?}"
            );
        }
    }
}
