//! The specification a model is given: every English page under `spec/`, as `cairn docs` prints
//! it, from the copy this checkout embeds.

use std::fmt::Write as _;

use cairn_lang_core::lock::HashHex;

/// The specification pages, joined as the system prompt carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecPages {
    text: String,
    count: usize,
}

impl SpecPages {
    /// The English specification pages this checkout embeds, in chapter order.
    #[must_use]
    pub fn embedded() -> Self {
        let pages: Vec<_> = cairn_lang_docs::pages()
            .iter()
            .filter(|page| page.path() == "spec" || page.path().starts_with("spec/"))
            .collect();
        let text = pages.iter().fold(String::new(), |mut text, page| {
            let _ = write!(
                text,
                "<page path=\"/{}/\">\n{}</page>\n",
                page.path(),
                page.document()
            );
            text
        });
        Self {
            text,
            count: pages.len(),
        }
    }

    /// The pages, each in a `<page>` element naming the path the site serves it at.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many pages there are.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The sha256 of [`Self::text`], which a run record carries as `spec_pages`.
    #[must_use]
    pub fn hash(&self) -> HashHex {
        HashHex::from_bytes(self.text.as_bytes())
    }
}
