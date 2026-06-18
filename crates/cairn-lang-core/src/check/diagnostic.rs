//! [`Diagnostic`] payload used by every `check` pass.

use std::num::NonZeroU32;

use serde::{Serialize, Serializer};

use crate::error::{Position, Span};

/// Severity of a single [`Diagnostic`].
///
/// `Error` participates in the `cairn check` exit code (any error → exit 1);
/// `Warning` does not. Stable per `spec/lint.md` §11.2: errors are things
/// that, left alone, cause unintended results; warnings are advisory drift.
/// Every M2-PR2 code is `Error`; `Warning` is defined now so the discriminant
/// is locked before the first warning ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// User-impacting problem — `cairn check` exits non-zero.
    Error,
    /// Advisory finding — emitted but does not change the exit code.
    Warning,
}

impl Severity {
    /// Lowercase rendering used in the gcc-style text format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// Stable identifier for a kind of [`Diagnostic`].
///
/// The string form (`E_DUPLICATE_SIZE`, `E_UNKNOWN_KEYWORD`, ...) is the
/// contract surface: downstream tooling matches on it without inspecting
/// the prose `primary` message. Marked `#[non_exhaustive]` so adding new
/// codes during the Evolving phase (pre-M3) does not break callers' exhaust
/// matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// More than one `size=` argument in a struct or def header.
    DuplicateSize,
    /// Repeated `slot NAME` line in a single `theme` block.
    DuplicateSlot,
    /// Repeated `key=` in the same argument list (struct/def header,
    /// statement args, selector attrs / bindings).
    DuplicateArg,
    /// Two or more members in the same immediate body share an `id=`.
    DuplicateId,
    /// A statement keyword not in the M2 keyword table.
    UnknownKeyword,
    /// `id=`, `class=`, or `mat_slot=` whose value is not a label
    /// (identifier or string).
    TypeMismatchLabel,
    /// `size=` whose value is not a `WxH` literal.
    TypeMismatchSize,
}

impl DiagnosticCode {
    /// Stable string form for the gcc-style text format and JSON output.
    /// The same string is used by external matchers (LSP quick-fix etc.) so
    /// changes here are breaking for consumers.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateSize => "E_DUPLICATE_SIZE",
            Self::DuplicateSlot => "E_DUPLICATE_SLOT",
            Self::DuplicateArg => "E_DUPLICATE_ARG",
            Self::DuplicateId => "E_DUPLICATE_ID",
            Self::UnknownKeyword => "E_UNKNOWN_KEYWORD",
            Self::TypeMismatchLabel => "E_TYPE_MISMATCH_LABEL",
            Self::TypeMismatchSize => "E_TYPE_MISMATCH_SIZE",
        }
    }

    /// Severity assigned to this code. Every M2-PR2 code is an error; the
    /// method exists so future warning-severity codes can attach without a
    /// separate lookup table.
    #[must_use]
    pub fn severity(self) -> Severity {
        Severity::Error
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // JSON output uses the same `E_*` string as the text format so
        // downstream tooling can match on a single contract surface
        // regardless of which `--format` was selected.
        serializer.serialize_str(self.as_str())
    }
}

/// Secondary location for a [`Diagnostic`] (the "first declared here"
/// pointer attached to a duplicate-key error, etc.).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagnosticNote {
    /// Byte range the note refers to.
    #[serde(skip)]
    pub span: Span,
    /// Human-readable note text.
    pub message: String,
}

/// One finding emitted by a `check` pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Severity of the finding.
    pub severity: Severity,
    /// Byte range the primary message points at.
    #[serde(skip)]
    pub span: Span,
    /// Primary message rendered after the code on the first line of the
    /// gcc-style text output.
    pub primary: String,
    /// Additional locations relevant to this finding. Emitted as indented
    /// `note: ...` lines in the text format.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
}

impl Diagnostic {
    /// Convert this diagnostic's primary byte span into a 1-based
    /// `line:column` [`Position`] against the given source string.
    ///
    /// Lines are split on `\n` (covering both LF and CRLF — the trailing
    /// `\r` adds one column the user sees an extra column for, but the
    /// behaviour matches what `cairn parse` reports today). Column counts
    /// Unicode scalar values, mirroring the `Position` documentation.
    #[must_use]
    pub fn position(&self, source: &str) -> Position {
        position_at(source, self.span.start)
    }
}

/// Compute a 1-based `line:column` for a byte offset into `source`.
///
/// `usize → u32` overflow saturates to `u32::MAX` rather than wrapping: any
/// source large enough to exceed 4 billion lines is also large enough that
/// "we lost track of the exact column" is the user's last concern.
pub(super) fn position_at(source: &str, byte_offset: usize) -> Position {
    let clamped = byte_offset.min(source.len());
    let prefix = &source[..clamped];
    let line_count = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let last_line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let column_chars = source[last_line_start..clamped].chars().count() + 1;
    let line =
        NonZeroU32::new(u32::try_from(line_count).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
    let col =
        NonZeroU32::new(u32::try_from(column_chars).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
    Position { line, col }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_as_str_round_trips_for_every_variant() {
        for code in [
            DiagnosticCode::DuplicateSize,
            DiagnosticCode::DuplicateSlot,
            DiagnosticCode::DuplicateArg,
            DiagnosticCode::DuplicateId,
            DiagnosticCode::UnknownKeyword,
            DiagnosticCode::TypeMismatchLabel,
            DiagnosticCode::TypeMismatchSize,
        ] {
            let s = code.as_str();
            assert!(
                s.starts_with("E_"),
                "code {code:?} should render to an E_-prefixed string, got {s}",
            );
            assert_eq!(code.severity(), Severity::Error, "M2-PR2 codes are errors");
        }
    }

    #[test]
    fn position_at_handles_unicode_columns() {
        // Two-byte UTF-8 character: the column count must advance by 1
        // (one Unicode scalar value), not by the byte length.
        let source = "α\nβ\n";
        let pos_after_alpha = position_at(source, 2); // byte 2 = start of '\n'
        assert_eq!(pos_after_alpha.line.get(), 1);
        assert_eq!(pos_after_alpha.col.get(), 2);

        let pos_on_beta = position_at(source, 3); // byte 3 = start of 'β'
        assert_eq!(pos_on_beta.line.get(), 2);
        assert_eq!(pos_on_beta.col.get(), 1);
    }

    #[test]
    fn position_at_for_offset_past_end_clamps_to_eof() {
        let source = "abc\n";
        let pos = position_at(source, 99);
        assert_eq!(pos.line.get(), 2);
        assert_eq!(pos.col.get(), 1);
    }
}
