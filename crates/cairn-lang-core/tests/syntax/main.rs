//! The lexer and the parser: tokens, refusals, spans and positions.

use std::num::NonZeroU32;

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::error::Position;
use cairn_lang_core::lex::{TokenKind, lex};
use cairn_lang_core::parse;

#[path = "../common/mod.rs"]
pub mod common;

mod block_ids;
mod coordinates;
mod decimal_literals;
mod edition_guards;
mod lex;
mod line_positions;
mod member_forms;
mod nbt_literals;
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

/// The `key=` arguments of the one member line under `struct s`, as `(key, value kind)`.
fn member_args(member: &str) -> Vec<(String, ValueKind)> {
    let source = format!("struct s size=5x5\n  {member}\n");
    let module = parse(&source).unwrap_or_else(|err| panic!("{member:?}: {err}"));
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct, got {:?}", module.items[0]);
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a member line, got {:?}", body[0]);
    };
    args.iter()
        .map(|arg| (arg.key.clone(), arg.value.kind.clone()))
        .collect()
}

/// The message `parse` refuses the one member line under `struct s` with, and its 1-based column.
fn member_refusal(member: &str) -> (String, u32) {
    let source = format!("struct s size=5x5\n  {member}\n");
    let err = parse(&source).expect_err("the member line is malformed");
    (err.user_message(), err.position().col.get())
}
