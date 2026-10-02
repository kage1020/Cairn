//! In-memory store of open documents.
//!
//! A `textDocument/completion` request identifies its document by URI only,
//! so the server has to remember the last synced text to answer it. This
//! store is that memory: URI → full text of the latest revision.
//!
//! It is also what a `didChange` is applied to. The server advertises FULL
//! sync, so a conforming client sends the whole new text in every event; a
//! client that sends a ranged edit anyway is out of spec, but taking that
//! edit's text as the whole document would leave the server diagnosing and
//! completing text nobody has. [`DocumentStore::apply`] applies each event
//! the way the protocol defines it, ranged or not.

use std::collections::HashMap;

use crate::line_index::LineIndex;

/// A `didChange` event whose range the document it applies to does not
/// have, so the revision cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeOutsideDocument {
    /// Index of the refused event in the notification's `contentChanges`.
    pub event: usize,
    /// The range it carried.
    pub range: lsp_types::Range,
}

impl std::fmt::Display for ChangeOutsideDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lsp_types::Range { start, end } = self.range;
        write!(
            f,
            "contentChanges[{}] edits {}:{}..{}:{}, which is not a range of the document as \
             the events before it leave it",
            self.event, start.line, start.character, end.line, end.character,
        )
    }
}

impl std::error::Error for ChangeOutsideDocument {}

/// Apply a `didChange`'s events to `text`, in order, and return the text
/// they build.
///
/// An event with no `range` replaces the whole text. One with a `range`
/// replaces that range — 0-based lines and UTF-16 columns, read against the
/// text as the events before it left it, which is how the protocol orders
/// them. Columns are resolved by [`LineIndex::offset_at`], so a column past
/// its line clamps to the line end; a line the text does not have, or a
/// range whose end comes before its start, refuses the whole revision.
///
/// # Errors
///
/// [`ChangeOutsideDocument`] for the first event whose range the text does
/// not have.
pub fn apply_content_changes(
    text: &str,
    changes: &[lsp_types::TextDocumentContentChangeEvent],
) -> Result<String, ChangeOutsideDocument> {
    let mut text = text.to_owned();
    for (event, change) in changes.iter().enumerate() {
        let Some(range) = change.range else {
            text.clone_from(&change.text);
            continue;
        };
        let lines = LineIndex::new(&text);
        let span = lines
            .offset_at(&text, range.start)
            .zip(lines.offset_at(&text, range.end))
            .filter(|(start, end)| start <= end);
        let Some((start, end)) = span else {
            return Err(ChangeOutsideDocument { event, range });
        };
        text.replace_range(start..end, &change.text);
    }
    Ok(text)
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

    /// Replace the stored text with a `didChange` revision (FULL sync — the
    /// notification carries the complete new content), handing back the
    /// text now stored, or `None` when the URI names no open document.
    ///
    /// A URI the store does not hold is refused rather than inserted,
    /// because the store's contract is to mirror the client's open set and
    /// `insert` did not: it resurrected a document the client had closed,
    /// leaving the server answering completion for a buffer the editor no
    /// longer has.
    ///
    /// Three ways to reach the `None`, and the third is the server's own
    /// doing:
    ///
    /// - a `didChange` after `didClose`,
    /// - a `didChange` for a URI never opened,
    /// - a `didChange` whose `didOpen` the server itself dropped, because
    ///   the payload did not match the method's schema. That document
    ///   stays unknown until the client opens it again — a keystroke no
    ///   longer revives it — which is a real cost, paid only by a client
    ///   that violated the protocol on the way in.
    ///
    /// The return value is the guard the caller needs, so it is an
    /// `Option` rather than a `bool`: ignoring it is a warning rather than
    /// a silently republished document.
    pub fn change(&mut self, uri: &lsp_types::Uri, text: String) -> Option<&str> {
        let slot = self.docs.get_mut(uri)?;
        *slot = text;
        Some(slot)
    }

    /// Apply a `didChange`'s events to an open document, handing back the
    /// text now stored, or `None` when the URI names no open document —
    /// the guard [`Self::change`] describes.
    ///
    /// The events are applied by [`apply_content_changes`]. When it refuses
    /// one, nothing is stored and the document keeps the text it had.
    ///
    /// # Errors
    ///
    /// `Some(Err(_))` carries the [`ChangeOutsideDocument`] that refused the
    /// revision.
    pub fn apply(
        &mut self,
        uri: &lsp_types::Uri,
        changes: &[lsp_types::TextDocumentContentChangeEvent],
    ) -> Option<Result<&str, ChangeOutsideDocument>> {
        let slot = self.docs.get_mut(uri)?;
        Some(apply_content_changes(slot, changes).map(|text| {
            *slot = text;
            slot.as_str()
        }))
    }

    /// Forget a document on `didClose`. Closing a URI that was never opened
    /// is a no-op — the store's contract is "mirror the client's open set",
    /// not to police the client's notification ordering.
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
    fn change_replaces_the_stored_text() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "old".to_owned());
        assert_eq!(
            store.change(&uri("file:///a.crn"), "new".to_owned()),
            Some("new"),
        );
        assert_eq!(store.get(&uri("file:///a.crn")), Some("new"));
    }

    #[test]
    fn change_refuses_a_document_that_is_not_open() {
        let mut store = DocumentStore::new();
        assert_eq!(
            store.change(&uri("file:///never.crn"), "text".to_owned()),
            None
        );
        assert_eq!(store.get(&uri("file:///never.crn")), None);
    }

    #[test]
    fn change_after_close_does_not_reopen_the_document() {
        // The order that made the store outlive the client's open set.
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "text".to_owned());
        store.close(&uri("file:///a.crn"));
        assert_eq!(
            store.change(&uri("file:///a.crn"), "later".to_owned()),
            None
        );
        assert_eq!(store.get(&uri("file:///a.crn")), None);
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
    fn a_ranged_event_replaces_its_range_and_nothing_else() {
        assert_eq!(
            apply_content_changes("ab\ncd\n", &[edit((1, 0), (1, 1), "X")]),
            Ok("ab\nXd\n".to_owned()),
        );
    }

    #[test]
    fn events_apply_in_order_each_against_the_text_the_last_left() {
        // The second range is only right against the first's result: on the
        // original text, line 1 is `cd`.
        assert_eq!(
            apply_content_changes(
                "ab\ncd\n",
                &[edit((0, 0), (0, 0), "# n\n"), edit((1, 0), (1, 2), "AB")],
            ),
            Ok("# n\nAB\ncd\n".to_owned()),
        );
    }

    #[test]
    fn an_event_without_a_range_replaces_the_whole_text() {
        assert_eq!(
            apply_content_changes("old", &[edit((0, 0), (0, 0), "x"), whole("new")]),
            Ok("new".to_owned()),
        );
    }

    #[test]
    fn a_ranged_event_counts_columns_in_utf16_units() {
        // `🪨` is two UTF-16 units and four bytes, so column 2 is the byte
        // after it.
        assert_eq!(
            apply_content_changes("🪨b", &[edit((0, 2), (0, 3), "c")]),
            Ok("🪨c".to_owned()),
        );
    }

    #[test]
    fn a_range_the_text_does_not_have_refuses_the_revision() {
        let past = edit((3, 0), (3, 0), "x");
        assert_eq!(
            apply_content_changes("ab\n", std::slice::from_ref(&past)),
            Err(ChangeOutsideDocument {
                event: 0,
                range: past.range.expect("ranged"),
            }),
        );
        let reversed = edit((0, 2), (0, 1), "x");
        assert_eq!(
            apply_content_changes("ab\n", &[whole("ab\n"), reversed.clone()]),
            Err(ChangeOutsideDocument {
                event: 1,
                range: reversed.range.expect("ranged"),
            }),
        );
    }

    #[test]
    fn a_refused_revision_leaves_the_stored_text_alone() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "ab\n".to_owned());
        let refused = store.apply(
            &uri("file:///a.crn"),
            &[edit((0, 0), (0, 0), "x"), edit((9, 0), (9, 0), "y")],
        );
        assert!(matches!(refused, Some(Err(_))), "{refused:?}");
        assert_eq!(store.get(&uri("file:///a.crn")), Some("ab\n"));
        assert_eq!(store.apply(&uri("file:///b.crn"), &[whole("x")]), None);
    }

    #[test]
    fn close_forgets_the_document() {
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "text".to_owned());
        store.close(&uri("file:///a.crn"));
        assert_eq!(store.get(&uri("file:///a.crn")), None);
        // Closing again (or a never-opened URI) is a no-op.
        store.close(&uri("file:///a.crn"));
    }
}
