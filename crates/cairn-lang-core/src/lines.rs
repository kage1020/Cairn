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
        let Some(start) = last_terminator_start(body) else {
            self.rest = "";
            return Some(body);
        };
        let len = terminator_len(body, start).expect("a terminator starts here");
        // The terminator stays in `rest`: it is what closes the line before,
        // and dropping it here would silently swallow an empty line between
        // two breaks.
        self.rest = &self.rest[..start + len];
        Some(&body[start + len..])
    }
}

impl std::iter::FusedIterator for Split<'_> {}

/// Length of the terminator `source` ends with, if it ends with one.
fn trailing_terminator_len(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.last()? {
        b'\n' if bytes.len() >= 2 && bytes[bytes.len() - 2] == b'\r' => Some(2),
        b'\r' | b'\n' => Some(1),
        _ => None,
    }
}

/// Offset at which the last terminator in `source` begins.
fn last_terminator_start(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let last = bytes.iter().rposition(|b| matches!(b, b'\r' | b'\n'))?;
    // A `\n` found this way may be the second half of a pair, whose first
    // byte is where the break actually starts.
    Some(
        if last > 0 && bytes[last] == b'\n' && bytes[last - 1] == b'\r' {
            last - 1
        } else {
            last
        },
    )
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

    /// Sources with a lone `\r`, an empty line, or a break at either edge —
    /// the shapes where stepping across a terminator can leave its own line.
    const AWKWARD: &[&str] = &[
        "\n",
        "\nx",
        "\n\r\n",
        "\r\r",
        "\r\r\r",
        "a\r\r\n",
        "a\rb\r\nc\nd",
        "\r\n\r",
        "x\r\ny",
    ];

    #[test]
    fn end_before_stops_at_the_terminator_it_follows() {
        assert_eq!(end_before("a\nb", 2), 1);
        assert_eq!(end_before("a\r\nb", 3), 1);
        assert_eq!(end_before("a\rb", 2), 1);
        // A source opening with a break: the first line is empty and ends
        // where it starts. Reading the byte before it would leave the
        // source entirely.
        assert_eq!(end_before("\nx", 1), 0);
        assert_eq!(end_before("\r\nx", 2), 0);
        // An empty line between two breaks gives up its own terminator and
        // not the one that opened it.
        assert_eq!(end_before("\r\r", 2), 1);
        assert_eq!(end_before("a\r\r\n", 4), 2);
    }

    #[test]
    fn end_before_never_leaves_the_line_it_ends() {
        for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
            let index = starts(source);
            for pair in index.windows(2) {
                let [line_start, next_start] = *pair else {
                    unreachable!("windows(2)")
                };
                let end = end_before(source, next_start);
                assert!(
                    (line_start..next_start).contains(&end),
                    "{source:?}: line {line_start}..{next_start} ends at {end}",
                );
                assert!(
                    !source[line_start..end].contains(['\r', '\n']),
                    "{source:?}: line {line_start}..{end} covers its terminator",
                );
                assert!(
                    terminator_len(source, end).is_some(),
                    "{source:?}: {end} should be where the terminator begins",
                );
            }
        }
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
    fn split_matches_str_lines_backwards_too() {
        for source in AGREED {
            assert_eq!(
                split(source).rev().collect::<Vec<_>>(),
                source.lines().rev().collect::<Vec<_>>(),
                "{source:?}",
            );
        }
    }

    /// The two ends have to meet exactly once, or a line is yielded twice
    /// or not at all. Alternating is the only way to catch that.
    #[test]
    fn split_yields_each_line_once_when_walked_from_both_ends() {
        for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
            let expected: Vec<&str> = split(source).collect();
            let mut iterator = split(source);
            let (mut front, mut back) = (Vec::new(), Vec::new());
            let mut take_front = true;
            loop {
                let next = if take_front {
                    iterator.next()
                } else {
                    iterator.next_back()
                };
                let Some(line) = next else { break };
                if take_front { &mut front } else { &mut back }.push(line);
                take_front = !take_front;
            }
            back.reverse();
            front.extend(back);
            assert_eq!(front, expected, "{source:?}");
        }
    }

    #[test]
    fn split_also_breaks_at_a_lone_carriage_return_backwards() {
        assert_eq!(split("a\rb\r").rev().collect::<Vec<_>>(), vec!["b", "a"]);
        assert_eq!(
            split("a\r\rb").rev().collect::<Vec<_>>(),
            vec!["b", "", "a"]
        );
    }

    #[test]
    fn start_of_agrees_with_the_index_at_every_offset() {
        for source in AGREED.iter().copied().chain(AWKWARD.iter().copied()) {
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
