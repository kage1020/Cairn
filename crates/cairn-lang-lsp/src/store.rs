//! The latest text of each open document, which a completion request names by URI alone. A
//! ranged edit is applied as one, though the server advertises full sync.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::line_index::LineIndex;

/// Why [`DocumentStore::apply`] stored nothing for a `didChange`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeRefused {
    /// The URI names no open document.
    DocumentNotOpen,
    /// An event's range names a line the text it applies to does not have.
    OutsideDocument {
        /// Index of the event in the notification's `contentChanges`.
        event_index: usize,
        /// The range it carried.
        range: lsp_types::Range,
        /// How many lines the text it applies to has.
        line_count: usize,
    },
    /// An event's range has an end that resolves to a byte before its start.
    ReversedRange {
        /// Index of the event in the notification's `contentChanges`.
        event_index: usize,
        /// The range it carried.
        range: lsp_types::Range,
    },
}

impl std::fmt::Display for ChangeRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DocumentNotOpen => f.write_str("the document is not open"),
            Self::OutsideDocument {
                event_index,
                range,
                line_count,
            } => {
                write!(
                    f,
                    "contentChanges[{event_index}] edits {}, but the text it applies to has ",
                    range_text(range),
                )?;
                match line_count {
                    1 => f.write_str("1 line (line 0)"),
                    n => write!(f, "{n} lines (0 to {})", n.saturating_sub(1)),
                }
            }
            Self::ReversedRange { event_index, range } => write!(
                f,
                "contentChanges[{event_index}] edits {}, whose end resolves before its start",
                range_text(range),
            ),
        }
    }
}

impl std::error::Error for ChangeRefused {}

/// `line:character..line:character`, the way a refusal prints a range.
fn range_text(range: &lsp_types::Range) -> String {
    let lsp_types::Range { start, end } = range;
    format!(
        "{}:{}..{}:{}",
        start.line, start.character, end.line, end.character,
    )
}

/// The text a `didChange`'s events build from `text`, each applied to what the one before left.
///
/// # Errors
///
/// The refusal of the first event that cannot be applied.
pub(crate) fn apply_content_changes(
    text: &str,
    changes: impl IntoIterator<Item = lsp_types::TextDocumentContentChangeEvent>,
) -> Result<String, ChangeRefused> {
    let mut text = Cow::Borrowed(text);
    for (event_index, change) in changes.into_iter().enumerate() {
        let Some(range) = change.range else {
            text = Cow::Owned(change.text);
            continue;
        };
        let lines = LineIndex::new(&text);
        let (Some(start), Some(end)) = (
            lines.offset_at(&text, range.start),
            lines.offset_at(&text, range.end),
        ) else {
            return Err(ChangeRefused::OutsideDocument {
                event_index,
                range,
                line_count: lines.line_count(),
            });
        };
        if end < start {
            return Err(ChangeRefused::ReversedRange { event_index, range });
        }
        text.to_mut().replace_range(start..end, &change.text);
    }
    Ok(text.into_owned())
}

/// Latest full text of every open document, keyed by URI.
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: HashMap<lsp_types::Uri, String>,
}

impl DocumentStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the full text delivered by `didOpen`.
    pub fn open(&mut self, uri: lsp_types::Uri, text: String) {
        self.docs.insert(uri, text);
    }

    /// Applies a `didChange`'s `contentChanges` to an open document and returns the new text.
    ///
    /// # Errors
    ///
    /// A [`ChangeRefused`], leaving the stored text as it was.
    pub fn apply(
        &mut self,
        uri: &lsp_types::Uri,
        changes: impl IntoIterator<Item = lsp_types::TextDocumentContentChangeEvent>,
    ) -> Result<&str, ChangeRefused> {
        let slot = self
            .docs
            .get_mut(uri)
            .ok_or(ChangeRefused::DocumentNotOpen)?;
        *slot = apply_content_changes(slot, changes)?;
        Ok(slot)
    }

    /// Forgets a document; closing one that is not open does nothing.
    pub fn close(&mut self, uri: &lsp_types::Uri) {
        self.docs.remove(uri);
    }

    /// Latest text of an open document, or `None` when the URI is not open.
    #[must_use]
    pub fn get(&self, uri: &lsp_types::Uri) -> Option<&str> {
        self.docs.get(uri).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn uri(s: &str) -> lsp_types::Uri {
        lsp_types::Uri::from_str(s).expect("valid uri")
    }

    fn edit(
        (start_line, start_character): (u32, u32),
        (end_line, end_character): (u32, u32),
        text: &str,
    ) -> lsp_types::TextDocumentContentChangeEvent {
        lsp_types::TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position::new(start_line, start_character),
                end: lsp_types::Position::new(end_line, end_character),
            }),
            range_length: None,
            text: text.to_owned(),
        }
    }

    fn whole(text: &str) -> lsp_types::TextDocumentContentChangeEvent {
        lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.to_owned(),
        }
    }

    #[test]
    fn open_then_get_returns_the_text() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "struct s size=2x2\n".to_owned());
        assert_eq!(
            store.get(&uri("file:///a.crn")),
            Some("struct s size=2x2\n"),
        );
        assert_eq!(store.get(&uri("file:///other.crn")), None);
    }

    #[test]
    fn apply_stores_a_whole_text_event() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "old".to_owned());
        assert_eq!(
            store.apply(&uri("file:///a.crn"), [whole("new")]),
            Ok("new")
        );
        assert_eq!(store.get(&uri("file:///a.crn")), Some("new"));
    }

    #[test]
    fn apply_refuses_a_document_that_is_not_open() {
        let mut store = DocumentStore::new();
        assert_eq!(
            store.apply(&uri("file:///never.crn"), [whole("text")]),
            Err(ChangeRefused::DocumentNotOpen),
        );
        assert_eq!(store.get(&uri("file:///never.crn")), None);
    }

    #[test]
    fn apply_after_close_does_not_reopen_the_document() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "text".to_owned());
        store.close(&uri("file:///a.crn"));
        assert_eq!(
            store.apply(&uri("file:///a.crn"), [whole("later")]),
            Err(ChangeRefused::DocumentNotOpen),
        );
        assert_eq!(store.get(&uri("file:///a.crn")), None);
    }

    #[test]
    fn a_ranged_event_replaces_its_range_and_nothing_else() {
        assert_eq!(
            apply_content_changes("ab\ncd\n", [edit((1, 0), (1, 1), "X")]),
            Ok("ab\nXd\n".to_owned()),
        );
    }

    #[test]
    fn a_ranged_event_with_empty_text_deletes_its_range() {
        assert_eq!(
            apply_content_changes("abc\n", [edit((0, 1), (0, 2), "")]),
            Ok("ac\n".to_owned()),
        );
    }

    #[test]
    fn events_apply_in_order_each_against_the_text_the_last_left() {
        assert_eq!(
            apply_content_changes(
                "ab\ncd\n",
                [edit((0, 0), (0, 0), "# n\n"), edit((1, 0), (1, 2), "AB")],
            ),
            Ok("# n\nAB\ncd\n".to_owned()),
        );
    }

    #[test]
    fn an_event_without_a_range_replaces_the_whole_text() {
        assert_eq!(
            apply_content_changes("old", [edit((0, 0), (0, 0), "x"), whole("new")]),
            Ok("new".to_owned()),
        );
    }

    #[test]
    fn a_ranged_event_counts_columns_in_utf16_units() {
        assert_eq!(
            apply_content_changes("🪨b", [edit((0, 2), (0, 3), "c")]),
            Ok("🪨c".to_owned()),
        );
    }

    #[test]
    fn a_column_inside_a_surrogate_pair_takes_the_whole_char() {
        assert_eq!(
            apply_content_changes("🪨b", [edit((0, 1), (0, 2), "")]),
            Ok("b".to_owned()),
        );
    }

    #[test]
    fn a_range_across_line_breaks_deletes_them_whole() {
        for (text, start, end, expected) in [
            ("ab\ncd\n", (1, 0), (2, 0), "ab\n"),
            ("ab\r\ncd\r\n", (0, 2), (1, 0), "abcd\r\n"),
            ("ab\rcd\r", (0, 2), (1, 0), "abcd\r"),
            ("ab\r\ncd\r\n", (0, 0), (1, 2), "\r\n"),
            ("ab\r\ncd\nef", (0, 1), (2, 1), "af"),
        ] {
            assert_eq!(
                apply_content_changes(text, [edit(start, end, "")]),
                Ok(expected.to_owned()),
                "{start:?}..{end:?} on {text:?}",
            );
        }
    }

    #[test]
    fn a_column_past_the_last_line_inserts_at_its_end() {
        assert_eq!(
            apply_content_changes("abc\n", [edit((1, 0), (1, 0), "Z")]),
            Ok("abc\nZ".to_owned()),
        );
        assert_eq!(
            apply_content_changes("ab\r\ncd\nef", [edit((2, 9), (2, 9), "Z")]),
            Ok("ab\r\ncd\nefZ".to_owned()),
        );
        let past = edit((2, 0), (2, 0), "Z");
        assert_eq!(
            apply_content_changes("abc\n", [past.clone()]),
            Err(ChangeRefused::OutsideDocument {
                event_index: 0,
                range: past.range.expect("ranged"),
                line_count: 2,
            }),
        );
    }

    #[test]
    fn a_range_the_text_does_not_have_refuses_the_revision() {
        let past = edit((3, 0), (3, 0), "x");
        assert_eq!(
            apply_content_changes("ab\n", [past.clone()]),
            Err(ChangeRefused::OutsideDocument {
                event_index: 0,
                range: past.range.expect("ranged"),
                line_count: 2,
            }),
        );
        let reversed = edit((0, 2), (0, 1), "x");
        assert_eq!(
            apply_content_changes("ab\n", [whole("ab\n"), reversed.clone()]),
            Err(ChangeRefused::ReversedRange {
                event_index: 1,
                range: reversed.range.expect("ranged"),
            }),
        );
    }

    #[test]
    fn a_range_is_compared_only_once_both_ends_are_resolved() {
        assert_eq!(
            apply_content_changes("ab\n", [edit((0, 5), (0, 3), "X")]),
            Ok("abX\n".to_owned()),
        );
    }

    #[test]
    fn a_refusal_says_how_many_lines_the_text_had() {
        let refused = apply_content_changes(
            "ab\n",
            [edit((0, 0), (0, 0), "x\n"), edit((9, 0), (9, 0), "y")],
        )
        .expect_err("line 9 is not there");
        assert_eq!(
            refused.to_string(),
            "contentChanges[1] edits 9:0..9:0, but the text it applies to has 3 lines (0 to 2)",
        );
        let refused = apply_content_changes("ab", [edit((1, 0), (1, 0), "y")])
            .expect_err("line 1 is not there");
        assert_eq!(
            refused.to_string(),
            "contentChanges[0] edits 1:0..1:0, but the text it applies to has 1 line (line 0)",
        );
    }

    #[test]
    fn a_refused_revision_leaves_the_stored_text_alone() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "ab\n".to_owned());
        let refused = store.apply(
            &uri("file:///a.crn"),
            [edit((0, 0), (0, 0), "x"), edit((9, 0), (9, 0), "y")],
        );
        assert!(
            matches!(refused, Err(ChangeRefused::OutsideDocument { .. })),
            "{refused:?}",
        );
        assert_eq!(store.get(&uri("file:///a.crn")), Some("ab\n"));
    }

    #[test]
    fn close_forgets_the_document() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "text".to_owned());
        store.close(&uri("file:///a.crn"));
        assert_eq!(store.get(&uri("file:///a.crn")), None);
        // Closing again does nothing.
        store.close(&uri("file:///a.crn"));
    }
}
