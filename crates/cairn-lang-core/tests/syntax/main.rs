//! The lexer and the parser: tokens, refusals, spans and positions.

use std::num::NonZeroU32;

use cairn_lang_core::error::Position;
use cairn_lang_core::lex::{TokenKind, lex};

#[path = "../common/mod.rs"]
pub mod common;

mod lex;
mod line_positions;
mod nesting_depth;
mod parse_commands;
mod parse_diagnostic;
mod parse_refusal_positions;
mod parse_truth_rows;
mod raw_directive_values;
mod span_propagation;
mod state_literals;

/// The position `line:col`, both 1-based.
fn pos(line: u32, col: u32) -> Position {
    Position {
        line: NonZeroU32::new(line).expect("line"),
        col: NonZeroU32::new(col).expect("col"),
    }
}

/// The kinds of the tokens `source` lexes to.
fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source)
        .expect("lex")
        .into_iter()
        .map(|t| t.kind)
        .collect()
}
