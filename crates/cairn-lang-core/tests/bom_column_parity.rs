//! A BOM shifts every layer's columns by the same amount.
//!
//! Three places turn a source location into a `line:column`: the lexer,
//! counting as it walks; `LineStarts::position` here, counting characters
//! from a line's start; and `LineIndex` in the LSP, doing the same for an
//! editor. They only agree if they agree about the byte-order mark, and
//! the two span-derived ones count it — so the lexer does too.
//!
//! Nothing else pins that. A lexer that skipped the mark would put its
//! own errors one column left of every check diagnostic about the same
//! character, on the first line of exactly the files a default Windows
//! editor produces, and no test in this crate would notice.

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::lex::lex;

/// The character both layers are asked about is the `$`, whose column the
/// BOM moves by one wherever it is measured from.
const WITHOUT_BOM: &str = "struct s size=3x3 $\n";

fn with_bom() -> String {
    format!("\u{feff}{WITHOUT_BOM}")
}

/// The lexer's own position for a stray character, with and without the
/// mark in front of it.
fn lexer_column(source: &str) -> u32 {
    lex(source)
        .expect_err("the `$` is not a token")
        .position()
        .col
        .get()
}

/// The same character's column, resolved from a byte offset the way every
/// check diagnostic resolves one.
fn span_column(source: &str) -> u32 {
    let offset = source.find('$').expect("fixture holds a `$`");
    LineStarts::new(source).position(source, offset).col.get()
}

#[test]
fn the_lexer_and_span_positions_agree_about_a_byte_order_mark() {
    let bom = with_bom();
    assert_eq!(
        lexer_column(&bom),
        span_column(&bom),
        "a lexer error and a check diagnostic disagree about the same character",
    );
    assert_eq!(
        lexer_column(WITHOUT_BOM),
        span_column(WITHOUT_BOM),
        "they should already agree without a mark",
    );
}

/// And the mark moves both by exactly its own width — one character, not
/// its three bytes.
#[test]
fn a_byte_order_mark_costs_one_column_in_both() {
    let bom = with_bom();
    assert_eq!(lexer_column(&bom), lexer_column(WITHOUT_BOM) + 1);
    assert_eq!(span_column(&bom), span_column(WITHOUT_BOM) + 1);
}
