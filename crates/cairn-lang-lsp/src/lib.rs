//! Language Server Protocol implementation for Cairn editors.
//!
//! Surfaces parser/lint diagnostics from `cairn-lang-core`, autocompletes canonical material tokens,
//! and feeds the self-correction loop described in the specification.
//!
//! [`line_index`] converts the byte spans `cairn-lang-core` reports into the
//! 0-based line / UTF-16 column positions the protocol mandates, and
//! [`diagnostics`] runs the `parse → lower → check` pipeline on a document
//! and maps every finding into an `lsp_types::Diagnostic`.

pub mod diagnostics;
pub mod line_index;
