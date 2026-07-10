//! Language Server Protocol implementation for Cairn editors.
//!
//! Surfaces parser/lint diagnostics from `cairn-lang-core`, autocompletes canonical material tokens,
//! and feeds the self-correction loop described in the specification.
//!
//! [`line_index`] converts the byte spans `cairn-lang-core` reports into the
//! 0-based line / UTF-16 column positions the protocol mandates.

pub mod line_index;
