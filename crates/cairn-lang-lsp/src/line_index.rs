//! Byte-offset to LSP position conversion.
//!
//! `cairn-lang-core` reports every finding as a byte [`Span`] into the source
//! text, and its own [`cairn_lang_core::LineStarts`] converts those into
//! 1-based line / Unicode-scalar-value columns for the CLI. The LSP wire
//! protocol instead mandates 0-based lines and **UTF-16 code unit** columns
//! (the default `positionEncoding`), so this module owns that conversion —
//! keeping UTF-16 knowledge out of `cairn-lang-core`, which stays
//! editor-agnostic.

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
    /// each subsequent entry is the byte after a `\n`.
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build the index by walking the source exactly once.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
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

    /// Byte offset of the end of the line containing `byte_offset` — the
    /// position of its `\n` (or of the `\r` in a `\r\n` pair), or
    /// `source.len()` for the final line.
    #[must_use]
    pub fn line_end(&self, source: &str, byte_offset: usize) -> usize {
        let clamped = byte_offset.min(source.len());
        let line_idx = self.line_starts.partition_point(|&s| s <= clamped) - 1;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .map_or(source.len(), |&next_start| next_start - 1);
        // A `\r\n` terminator contributes its `\r` to the line slice; the
        // logical line ends before it so ranges do not cover the carriage
        // return.
        if end > 0 && source.as_bytes().get(end - 1) == Some(&b'\r') {
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
        // AC10 (first half): offsets beyond the source clamp to the last
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
        // 4 bytes / 1 scalar / 2 UTF-16 units. AC10 (second half): the
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
}
