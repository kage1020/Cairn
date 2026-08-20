//! In-memory store of open documents.
//!
//! Diagnostics can be computed from the notification payload alone (every
//! `didOpen`/`didChange` carries the full text under FULL sync), but a
//! `textDocument/completion` request identifies its document by URI only —
//! the server has to remember the last synced text to answer it. This store
//! is that memory: URI → full text of the latest revision.

use std::collections::HashMap;

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
    /// notification carries the complete new content), reporting whether
    /// there was an open document to replace.
    ///
    /// A URI the store does not hold is refused rather than inserted. The
    /// store's contract is to mirror the client's open set, and `insert`
    /// broke it in the one direction that cannot be undone: a `didChange`
    /// after `didClose` — or for a document never opened at all — put the
    /// URI back, so the set only ever grew and the server answered
    /// completion for a buffer the editor no longer has.
    pub fn change(&mut self, uri: &lsp_types::Uri, text: String) -> bool {
        let Some(slot) = self.docs.get_mut(uri) else {
            return false;
        };
        *slot = text;
        true
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
        assert!(store.change(&uri("file:///a.crn"), "new".to_owned()));
        assert_eq!(store.get(&uri("file:///a.crn")), Some("new"));
    }

    #[test]
    fn change_refuses_a_document_that_is_not_open() {
        let mut store = DocumentStore::new();
        assert!(!store.change(&uri("file:///never.crn"), "text".to_owned()));
        assert_eq!(store.get(&uri("file:///never.crn")), None);
    }

    #[test]
    fn change_after_close_does_not_reopen_the_document() {
        // The order that made the store outlive the client's open set.
        let mut store = DocumentStore::new();
        store.open(uri("file:///a.crn"), "text".to_owned());
        store.close(&uri("file:///a.crn"));
        assert!(!store.change(&uri("file:///a.crn"), "later".to_owned()));
        assert_eq!(store.get(&uri("file:///a.crn")), None);
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
