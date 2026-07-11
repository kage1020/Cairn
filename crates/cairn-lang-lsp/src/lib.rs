//! Language Server Protocol implementation for Cairn editors.
//!
//! Surfaces parser/lint diagnostics from `cairn-lang-core`, autocompletes canonical material tokens,
//! and feeds the self-correction loop described in the specification.
//!
//! The crate splits into a pure conversion layer and a thin transport shell:
//! [`line_index`] converts the byte spans `cairn-lang-core` reports into the
//! 0-based line / UTF-16 column positions the protocol mandates,
//! [`diagnostics`] runs the `parse → lower → check` pipeline on a document and
//! maps every finding into an `lsp_types::Diagnostic`, [`completion`] turns a
//! cursor position into closed-vocabulary candidates, and [`server`] owns the
//! stdio JSON-RPC loop wiring both to the editor, reading request-time
//! document text from the [`store`].

pub mod completion;
pub mod diagnostics;
pub mod line_index;
pub mod server;
pub mod store;
