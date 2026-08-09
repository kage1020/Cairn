//! Byte-offset to LSP position conversion.
//!
//! `cairn-lang-core` reports every finding as a byte [`Span`] into the source
//! text, and its own [`cairn_lang_core::check::LineStarts`] converts those
//! into 1-based line / Unicode-scalar-value columns for the CLI. The LSP wire
//! protocol instead mandates 0-based lines and **UTF-16 code unit** columns
//! (the default `positionEncoding`), so this module owns that conversion —
//! keeping UTF-16 knowledge out of `cairn-lang-core`, which stays
//! editor-agnostic.
//!
//! What it does *not* own is where a line ends. That comes from
//! [`cairn_lang_core::lines`], because an editor compares the line number in
//! a diagnostic against the line number under its own cursor: a document
//! containing a lone `\r` used to shift every diagnostic after it onto the
//! wrong line, since VS Code and Monaco both break on `\r\n|\r|\n` and this
//! index broke on `\n` alone.

use cairn_lang_core::{Position as CorePosition, Span};

/// Precomputed line-start byte offsets for one document revision.
///
/// Build once per document text with [`LineIndex::new`], then convert any
/// number of byte offsets / spans. Mirrors `cairn_lang_core::LineStarts`
/// (whose internal offsets are private) but targets the LSP coordinate
/// system directly.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the first byte of each line: entry 0 is always 0, and
    /// each subsequent entry is the byte after a line break.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build the index by walking the source exactly once.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            line_starts: cairn_lang_core::lines::starts(source),
        }
    }

    /// Convert a byte offset into a 0-based line / UTF-16 code-unit column
    /// [`lsp_types::Position`]. Offsets past the end of `source` clamp to
    /// the final position.
    #[must_use]
    pub fn position(&self, source: &str, byte_offset: usize) -> lsp_types::Position {
        let clamped = byte_offset.min(source.len());
        // partition_point returns the count of line starts at or before the
        // offset; the containing line is the last of those, i.e. count - 1.
        let line_idx = self.line_starts.partition_point(|&s| s <= clamped) - 1;
        let line_start = self.line_starts[line_idx];
        let character = source[line_start..clamped].encode_utf16().count();
        lsp_types::Position {
            line: u32::try_from(line_idx).unwrap_or(u32::MAX),
            character: u32::try_from(character).unwrap_or(u32::MAX),
        }
    }

    /// Convert both ends of a core byte [`Span`] into an
    /// [`lsp_types::Range`].
    #[must_use]
    pub fn range(&self, source: &str, span: &Span) -> lsp_types::Range {
        lsp_types::Range {
            start: self.position(source, span.start),
            end: self.position(source, span.end),
        }
    }

    /// Inverse conversion for parse/lex errors, which carry a 1-based
    /// line / Unicode-scalar-value [`CorePosition`] instead of a byte span:
    /// recover the byte offset of that position. Columns past the end of
    /// the line clamp to the line's last byte; lines past the end of the
    /// source clamp to `source.len()`.
    #[must_use]
    pub fn offset_of(&self, source: &str, pos: CorePosition) -> usize {
        let line_idx = pos.line.get() as usize - 1;
        let Some(&line_start) = self.line_starts.get(line_idx) else {
            return source.len();
        };
        let line_end = self.line_end(source, line_start);
        let line = &source[line_start..line_end];
        let scalar_col = pos.col.get() as usize - 1;
        line.char_indices()
            .map(|(i, _)| i)
            .nth(scalar_col)
            .map_or(line_end, |rel| line_start + rel)
    }

    /// Inverse of [`LineIndex::position`]: recover the byte offset of a
    /// protocol position (0-based line, UTF-16 code-unit column).
    ///
    /// Slightly-off coordinates clamp — a request can carry a position one
    /// keystroke ahead of the last synced revision, and clamping yields the
    /// nearest valid offset instead of a dead request: a column past the end
    /// of the line clamps to the line end (before its terminator), a column
    /// landing between the two UTF-16 units of an astral char clamps down to
    /// that char's start so the result is always a char boundary, and a line
    /// exactly one past the last clamps to `source.len()` (the revision race
    /// can remove at most the lines the change deleted, and answering at EOF
    /// is right for the common append case). A line further out is not a
    /// race but a client bug; that returns `None` so the caller can refuse
    /// the request instead of fabricating an EOF context.
    #[must_use]
    pub fn offset_at(&self, source: &str, position: lsp_types::Position) -> Option<usize> {
        let line = position.line as usize;
        let Some(&line_start) = self.line_starts.get(line) else {
            return (line == self.line_starts.len()).then_some(source.len());
        };
        let line_end = self.line_end(source, line_start);
        let target = position.character as usize;
        let mut units = 0;
        for (rel, ch) in source[line_start..line_end].char_indices() {
            if target < units + ch.len_utf16() {
                return Some(line_start + rel);
            }
            units += ch.len_utf16();
        }
        Some(line_end)
    }

    /// Byte offset of the end of the line containing `byte_offset` — the
    /// first byte of its terminator, or `source.len()` for the final line.
    #[must_use]
    pub fn line_end(&self, source: &str, byte_offset: usize) -> usize {
        let clamped = byte_offset.min(source.len());
        let line_idx = self.line_starts.partition_point(|&s| s <= clamped) - 1;
        let line_start = self.line_starts[line_idx];
        let Some(&next_start) = self.line_starts.get(line_idx + 1) else {
            return source.len();
        };
        // `next_start` is the byte after the terminator, so one back is the
        // terminator's *last* byte. That is the whole terminator unless it
        // is a `\r\n` pair, whose `\r` also has to be excluded so a range
        // never covers a carriage return.
        //
        // The `end > line_start` guard is what keeps the step back inside
        // this line: for a source of two lone `\r`s, the second line is
        // empty and the byte before it is the first line's `\r`, which is
        // not this line's to give up.
        let end = next_start - 1;
        let bytes = source.as_bytes();
        if end > line_start && bytes[end] == b'\n' && bytes[end - 1] == b'\r' {
            end - 1
        } else {
            end
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use cairn_lang_core::check::position_at;

    use super::*;

    fn core_pos(line: u32, col: u32) -> CorePosition {
        CorePosition {
            line: NonZeroU32::new(line).expect("line"),
            col: NonZeroU32::new(col).expect("col"),
        }
    }

    #[test]
    fn position_is_zero_based_on_ascii_lines() {
        let source = "abc\ndef\n";
        let index = LineIndex::new(source);
        assert_eq!(
            index.position(source, 0),
            lsp_types::Position {
                line: 0,
                character: 0,
            },
        );
        assert_eq!(
            index.position(source, 5),
            lsp_types::Position {
                line: 1,
                character: 1,
            },
        );
    }

    #[test]
    fn position_clamps_past_eof_to_final_position() {
        // Offsets beyond the source clamp to the last
        // position instead of panicking on the slice.
        let source = "abc\n";
        let index = LineIndex::new(source);
        assert_eq!(
            index.position(source, 99),
            lsp_types::Position {
                line: 1,
                character: 0,
            },
        );
    }

    #[test]
    fn position_counts_utf16_code_units_not_bytes_or_scalars() {
        // 'é' is 2 bytes / 1 scalar / 1 UTF-16 unit; '😀' (U+1F600) is
        // 4 bytes / 1 scalar / 2 UTF-16 units. The
        // astral char advances `character` by 2.
        let source = "é😀x\n";
        let index = LineIndex::new(source);
        let x_offset = source.find('x').expect("x in source");
        assert_eq!(
            index.position(source, x_offset),
            lsp_types::Position {
                line: 0,
                character: 3,
            },
        );
    }

    #[test]
    fn position_on_crlf_line_boundary() {
        let source = "ab\r\ncd\n";
        let index = LineIndex::new(source);
        // Offset of 'c': after "ab\r\n" = byte 4, line 1 col 0.
        assert_eq!(
            index.position(source, 4),
            lsp_types::Position {
                line: 1,
                character: 0,
            },
        );
        // The '\r' itself still belongs to line 0.
        assert_eq!(
            index.position(source, 2),
            lsp_types::Position {
                line: 0,
                character: 2,
            },
        );
    }

    #[test]
    fn range_converts_both_span_ends() {
        let source = "abc\ndef\n";
        let index = LineIndex::new(source);
        let range = index.range(source, &(4..7));
        assert_eq!(
            range.start,
            lsp_types::Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            range.end,
            lsp_types::Position {
                line: 1,
                character: 3
            }
        );
    }

    #[test]
    fn offset_of_round_trips_with_core_position_at() {
        // For every char boundary, converting the byte offset through
        // core's `position_at` and back through `offset_of` recovers the
        // original offset. Non-ASCII chars included so scalar-value column
        // arithmetic is exercised.
        let source = "α\nfoo\nβar\n";
        let index = LineIndex::new(source);
        for (offset, _) in source.char_indices() {
            let core = position_at(source, offset);
            assert_eq!(
                index.offset_of(source, core),
                offset,
                "round trip failed at byte {offset}",
            );
        }
    }

    #[test]
    fn offset_of_clamps_past_line_and_source_ends() {
        let source = "ab\ncd";
        let index = LineIndex::new(source);
        // Column past the end of line 1 clamps to its line end (byte 2).
        assert_eq!(index.offset_of(source, core_pos(1, 99)), 2);
        // Line past the end of the source clamps to source.len().
        assert_eq!(index.offset_of(source, core_pos(9, 1)), source.len());
    }

    #[test]
    fn offset_at_round_trips_with_position() {
        // For every char boundary (and EOF), converting the byte offset to
        // an LSP position and back recovers the original offset. Non-ASCII
        // chars included so UTF-16 column arithmetic is exercised in both
        // directions.
        let source = "é😀x\nfoo\nβar";
        let index = LineIndex::new(source);
        let boundaries = source.char_indices().map(|(i, _)| i).chain([source.len()]);
        for offset in boundaries {
            let position = index.position(source, offset);
            assert_eq!(
                index.offset_at(source, position),
                Some(offset),
                "round trip failed at byte {offset}",
            );
        }
    }

    #[test]
    fn offset_at_clamps_column_line_and_surrogate_interior() {
        let source = "a😀\r\ncd";
        let index = LineIndex::new(source);
        // Column past the end of line 0 clamps to the line end, before the
        // `\r\n` terminator.
        assert_eq!(
            index.offset_at(
                source,
                lsp_types::Position {
                    line: 0,
                    character: 99,
                },
            ),
            Some(5),
        );
        // One line past the end clamps to source.len() — a stale position
        // from a racing didChange deserves an answer.
        assert_eq!(
            index.offset_at(
                source,
                lsp_types::Position {
                    line: 2,
                    character: 0,
                },
            ),
            Some(source.len()),
        );
        // Further out is a client bug, refused rather than clamped.
        assert_eq!(
            index.offset_at(
                source,
                lsp_types::Position {
                    line: 3,
                    character: 0,
                },
            ),
            None,
        );
        // A column landing between the two UTF-16 units of 😀 clamps down
        // to the char's start so the result is always a char boundary.
        assert_eq!(
            index.offset_at(
                source,
                lsp_types::Position {
                    line: 0,
                    character: 2,
                },
            ),
            Some(1),
        );
    }

    #[test]
    fn line_end_handles_lf_crlf_and_eof() {
        let source = "ab\r\ncd\nef";
        let index = LineIndex::new(source);
        // Line 0 ends before its `\r`.
        assert_eq!(index.line_end(source, 0), 2);
        // Line 1 ends at its `\n`.
        assert_eq!(index.line_end(source, 4), 6);
        // Final line without a terminator ends at EOF.
        assert_eq!(index.line_end(source, 7), source.len());
    }

    #[test]
    fn line_end_of_blank_line_is_its_start() {
        let source = "a\n\nb\n";
        let index = LineIndex::new(source);
        assert_eq!(index.line_end(source, 2), 2);
    }

    // -- the lone `\r` ----------------------------------------------------

    #[test]
    fn a_lone_carriage_return_starts_a_line() {
        let source = "ab\rcd\r\nef\ngh";
        let index = LineIndex::new(source);
        for (offset, expected) in [(0, (0, 0)), (3, (1, 0)), (7, (2, 0)), (10, (3, 0))] {
            let position = index.position(source, offset);
            assert_eq!(
                (position.line, position.character),
                expected,
                "byte {offset}",
            );
        }
    }

    /// The reported break: an editor splits on `\r\n|\r|\n`, so an index
    /// that splits on `\n` alone reports a diagnostic one line — and, in a
    /// CR-only document, every line — above where the editor draws it.
    #[test]
    fn line_numbers_match_the_core_resolver_for_every_line_ending() {
        let base = "struct s size=2x2\n  floor mat_slot=f\nstruct t size=3x3\n";
        for rendering in [
            base.to_owned(),
            base.replace('\n', "\r\n"),
            base.replace('\n', "\r"),
        ] {
            let source = rendering.as_str();
            let index = LineIndex::new(source);
            let core = cairn_lang_core::check::LineStarts::new(source);
            for (offset, _) in source.char_indices().chain([(source.len(), ' ')]) {
                let mine = index.position(source, offset);
                let theirs = core.position(source, offset);
                // ASCII fixture, so a UTF-16 unit is a scalar is a byte and
                // the only difference left is the 0- versus 1-based origin.
                assert_eq!(
                    (mine.line + 1, mine.character + 1),
                    (theirs.line.get(), theirs.col.get()),
                    "{source:?} at byte {offset}",
                );
            }
        }
    }

    #[test]
    fn position_and_offset_at_round_trip_across_mixed_line_endings() {
        // A `\r`-terminated document was the broken case: `line_end`
        // stripped the terminator of a line the index did not know had
        // ended, so `offset_at` answered two bytes short of `position`.
        for source in ["ab\r", "a\rb\r\nc\nd", "\r\r", "\r\n\r", "a\r\r\nb"] {
            let index = LineIndex::new(source);
            let boundaries = source
                .char_indices()
                .map(|(i, _)| i)
                .chain([source.len()])
                // The `\n` of a `\r\n` is not a position the protocol can
                // name: the pair is one line break, and a range that ended
                // between its halves would split it. Such an offset clamps
                // to the line end rather than round-tripping, which is the
                // documented behaviour for any column past a line's text.
                .filter(|&i| i == 0 || !source.as_bytes()[i - 1..].starts_with(b"\r\n"));
            for offset in boundaries {
                let position = index.position(source, offset);
                assert_eq!(
                    index.offset_at(source, position),
                    Some(offset),
                    "{source:?} round trip failed at byte {offset}",
                );
            }
        }
    }

    /// Stepping back over a terminator must not step out of the line doing
    /// the stepping. Two lone `\r`s put a `\r` immediately before an empty
    /// line, and an unguarded "is the byte before the end a `\r`?" test
    /// answers with an offset belonging to the line above — which then
    /// slices in reverse and panics.
    #[test]
    fn line_end_never_precedes_its_own_line_start() {
        for source in ["\r\r", "a\r\r\n", "\r\n\r\n", "a\n\r\nb", "\r\r\r"] {
            let index = LineIndex::new(source);
            for (offset, _) in source.char_indices().chain([(source.len(), ' ')]) {
                let start =
                    index.line_starts[index.line_starts.partition_point(|&s| s <= offset) - 1];
                let end = index.line_end(source, offset);
                assert!(
                    (start..=source.len()).contains(&end),
                    "{source:?}: line at byte {offset} starts at {start} but ends at {end}",
                );
                assert!(
                    !source[start..end].contains(['\r', '\n']),
                    "{source:?}: line at byte {offset} covers its terminator",
                );
            }
        }
    }
}
