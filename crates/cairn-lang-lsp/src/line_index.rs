//! Byte offsets ↔ LSP positions, which count 0-based lines and UTF-16 code-unit columns.
//!
//! Lines end where [`cairn_lang_core::lines`] ends them, at `\r\n`, `\r` or `\n`, as editors do.

use cairn_lang_core::Span;
use cairn_lang_core::lines::LineStarts;

/// One document revision's line starts, answering in LSP coordinates.
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: LineStarts,
}

impl LineIndex {
    /// Indexes the line starts of `source`.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            starts: LineStarts::new(source),
        }
    }

    /// How many lines the text has: `"abc\n"` has 2, and `""` has 1.
    pub(crate) fn line_count(&self) -> usize {
        self.starts.line_starts().len()
    }

    /// The protocol position of `byte_offset`, clamped to the end of `source`.
    #[must_use]
    pub fn position(&self, source: &str, byte_offset: usize) -> lsp_types::Position {
        let clamped = byte_offset.min(source.len());
        let line = self.starts.line_of(clamped);
        let line_start = self.starts.line_starts()[line];
        let character = source[line_start..clamped].encode_utf16().count();
        lsp_types::Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            character: u32::try_from(character).unwrap_or(u32::MAX),
        }
    }

    /// Both ends of `span` as a protocol range.
    #[must_use]
    pub fn range(&self, source: &str, span: &Span) -> lsp_types::Range {
        lsp_types::Range {
            start: self.position(source, span.start),
            end: self.position(source, span.end),
        }
    }

    /// The byte offset of a protocol position, its column clamped to a char boundary on the line,
    /// or `None` for a line the document does not have.
    #[must_use]
    pub fn offset_at(&self, source: &str, position: lsp_types::Position) -> Option<usize> {
        let line = position.line as usize;
        let &line_start = self.starts.line_starts().get(line)?;
        let line_end = self.starts.line_end(source, line_start);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            index.position(source, 4),
            lsp_types::Position {
                line: 1,
                character: 0,
            },
        );
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
    fn the_empty_line_after_a_trailing_terminator_is_a_line() {
        // Typing at the end of a newline-terminated file happens on this line.
        for source in ["abc\n", "abc\r\n", "abc\r"] {
            let index = LineIndex::new(source);
            assert_eq!(
                index.offset_at(
                    source,
                    lsp_types::Position {
                        line: 1,
                        character: 0,
                    }
                ),
                Some(source.len()),
                "for {source:?}",
            );
            assert_eq!(
                index.offset_at(
                    source,
                    lsp_types::Position {
                        line: 2,
                        character: 0,
                    }
                ),
                None,
                "for {source:?}",
            );
        }
        let index = LineIndex::new("");
        assert_eq!(
            index.offset_at(
                "",
                lsp_types::Position {
                    line: 0,
                    character: 0,
                }
            ),
            Some(0),
        );
        assert_eq!(
            index.offset_at(
                "",
                lsp_types::Position {
                    line: 1,
                    character: 0,
                }
            ),
            None,
        );
    }

    #[test]
    fn offset_at_round_trips_with_position() {
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
        for line in [2, 3, 99] {
            assert_eq!(
                index.offset_at(source, lsp_types::Position { line, character: 0 }),
                None,
                "line {line} is past the two this source has",
            );
        }
        // Column 2 falls between the two UTF-16 units of 😀.
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
                // ASCII, so only the 0- versus 1-based origin differs.
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
        for source in ["ab\r", "a\rb\r\nc\nd", "\r\r", "\r\n\r", "a\r\r\nb"] {
            let index = LineIndex::new(source);
            let boundaries = source
                .char_indices()
                .map(|(i, _)| i)
                .chain([source.len()])
                // The `\n` of a `\r\n` is no position the protocol can name.
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
}
