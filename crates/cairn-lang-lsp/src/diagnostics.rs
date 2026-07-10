//! Source text → LSP diagnostics conversion.
//!
//! [`compute_diagnostics`] runs the same pipeline as `cairn check`
//! (`parse → lower → check`) on one in-memory document and maps every
//! finding into an [`lsp_types::Diagnostic`]. The mapping preserves the
//! self-correction triple ("what is wrong / valid candidates / suggested
//! fix") verbatim: spanless notes are appended to the message body as
//! `note:` lines, exactly as the CLI text renderer prints them, and
//! span-carrying notes become `relatedInformation` entries pointing back
//! into the same document.

use cairn_lang_core::{Diagnostic as CoreDiagnostic, ParseError, Severity, check, lower, parse};

use crate::line_index::LineIndex;

/// Run `parse → lower → check` on `source` and convert every finding into
/// an LSP diagnostic anchored in the document at `uri`.
///
/// A parse/lex failure pre-empts the check passes (the AST has to be
/// well-formed before invariant collection can run — same rule as
/// `cairn check`) and yields exactly one error diagnostic. Edition is left
/// unpinned (`None`), matching `cairn check` without `--edition`: slot
/// presence checks union the per-edition theme variants.
#[must_use]
pub fn compute_diagnostics(uri: &lsp_types::Uri, source: &str) -> Vec<lsp_types::Diagnostic> {
    let index = LineIndex::new(source);
    let module = match parse(source) {
        Ok(module) => module,
        Err(err) => return vec![parse_error_diagnostic(source, &index, &err)],
    };
    let ir = lower(&module);
    check(&module, &ir, None)
        .iter()
        .map(|d| convert(uri, source, &index, d))
        .collect()
}

/// Map one core [`CoreDiagnostic`] into the LSP shape.
fn convert(
    uri: &lsp_types::Uri,
    source: &str,
    index: &LineIndex,
    diagnostic: &CoreDiagnostic,
) -> lsp_types::Diagnostic {
    let severity = match diagnostic.severity {
        Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
    };
    let mut message = diagnostic.primary.clone();
    let mut related = Vec::new();
    for note in &diagnostic.notes {
        if let Some(span) = &note.span {
            related.push(lsp_types::DiagnosticRelatedInformation {
                location: lsp_types::Location {
                    uri: uri.clone(),
                    range: index.range(source, span),
                },
                message: note.message.clone(),
            });
        } else {
            message.push_str("\nnote: ");
            message.push_str(&note.message);
        }
    }
    lsp_types::Diagnostic {
        range: index.range(source, &diagnostic.span),
        severity: Some(severity),
        code: Some(lsp_types::NumberOrString::String(
            diagnostic.code.as_str().to_owned(),
        )),
        source: Some("cairn".to_owned()),
        message,
        related_information: (!related.is_empty()).then_some(related),
        data: diagnostic
            .data
            .as_ref()
            .and_then(|data| serde_json::to_value(data).ok()),
        ..lsp_types::Diagnostic::default()
    }
}

/// Map a parse/lex failure into a single error diagnostic.
///
/// Parse errors carry a 1-based line/column [`cairn_lang_core::Position`]
/// rather than a byte span, so the range runs from that position to the end
/// of its line — wide enough for an editor squiggle to be visible — falling
/// back to the position's own point when the line remainder is empty (the
/// editor renders a zero-width range as a caret-width marker). No stable
/// `E_*` code exists for parse failures yet, so `code` stays unset rather
/// than inventing one outside the core contract.
fn parse_error_diagnostic(
    source: &str,
    index: &LineIndex,
    err: &ParseError,
) -> lsp_types::Diagnostic {
    let start_offset = index.offset_of(source, err.position());
    let end_offset = index.line_end(source, start_offset).max(start_offset);
    lsp_types::Diagnostic {
        range: index.range(source, &(start_offset..end_offset)),
        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
        source: Some("cairn".to_owned()),
        message: err.user_message(),
        ..lsp_types::Diagnostic::default()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    const DUPLICATE: &str =
        include_str!("../../cairn-lang-core/tests/fixtures/check/duplicate.crn");
    const CLEAN: &str = include_str!("../../cairn-lang-core/tests/fixtures/check/clean.crn");

    fn uri() -> lsp_types::Uri {
        lsp_types::Uri::from_str("file:///test.crn").expect("valid uri")
    }

    fn code_of(d: &lsp_types::Diagnostic) -> &str {
        match d.code.as_ref().expect("code present") {
            lsp_types::NumberOrString::String(s) => s,
            lsp_types::NumberOrString::Number(_) => panic!("expected string code"),
        }
    }

    #[test]
    fn duplicate_fixture_reports_e_duplicate_size_with_error_severity() {
        // AC2 (function level): the duplicate fixture carries a second
        // `size=5x5` on line 0; the finding surfaces with the stable code
        // string, ERROR severity, source "cairn", and a 0-based range on
        // that line.
        let diagnostics = compute_diagnostics(&uri(), DUPLICATE);
        let dup = diagnostics
            .iter()
            .find(|d| code_of(d) == "E_DUPLICATE_SIZE")
            .expect("E_DUPLICATE_SIZE reported");
        assert_eq!(dup.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
        assert_eq!(dup.source.as_deref(), Some("cairn"));
        assert_eq!(dup.range.start.line, 0);
        let second_size = u32::try_from(DUPLICATE.find("size=5x5").expect("second size"))
            .expect("offset fits u32");
        assert_eq!(dup.range.start.character, second_size);
    }

    #[test]
    fn span_notes_become_related_information() {
        // AC3 (first half): the "first declared here" note on a duplicate
        // finding points back into the same document with a distinct range.
        let diagnostics = compute_diagnostics(&uri(), DUPLICATE);
        let dup = diagnostics
            .iter()
            .find(|d| code_of(d) == "E_DUPLICATE_SIZE")
            .expect("E_DUPLICATE_SIZE reported");
        let related = dup
            .related_information
            .as_ref()
            .expect("relatedInformation present");
        assert_eq!(related[0].location.uri, uri());
        let first_size = u32::try_from(DUPLICATE.find("size=4x4").expect("first size"))
            .expect("offset fits u32");
        assert_eq!(related[0].location.range.start.character, first_size);
        assert!(
            related[0].message.contains("first"),
            "note should reference the first declaration, got: {}",
            related[0].message,
        );
    }

    #[test]
    fn spanless_notes_append_to_message_as_note_lines() {
        // AC3 (second half): the closed-set "expected one of" footer on
        // E_UNKNOWN_KEYWORD has no span of its own and must survive inside
        // the message body so the self-correction triple stays intact.
        let source = "struct s size=2x2\n  wals height=4\n";
        let diagnostics = compute_diagnostics(&uri(), source);
        let unknown = diagnostics
            .iter()
            .find(|d| code_of(d) == "E_UNKNOWN_KEYWORD")
            .expect("E_UNKNOWN_KEYWORD reported");
        assert!(
            unknown.message.contains("\nnote: "),
            "spanless note should be folded into the message, got: {}",
            unknown.message,
        );
    }

    #[test]
    fn parse_error_yields_exactly_one_error_diagnostic() {
        // AC4 (function level): parser-rejected content produces a single
        // ERROR diagnostic carrying the parse error's own message and a
        // non-empty range on the offending line.
        let source = "struct s size=2x2\n\tfloor\n";
        let err = parse(source).expect_err("tab indent should be rejected");
        let diagnostics = compute_diagnostics(&uri(), source);
        assert_eq!(diagnostics.len(), 1);
        let d = &diagnostics[0];
        assert_eq!(d.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
        assert_eq!(d.message, err.user_message());
        assert_eq!(d.code, None);
        assert_eq!(d.range.start.line, 1);
        assert!(
            d.range.end > d.range.start,
            "range should be non-empty, got {:?}",
            d.range,
        );
    }

    #[test]
    fn clean_fixture_reports_no_diagnostics() {
        // AC5 (function level): mirrors cli_4_clean_fixture_json_output_is_empty_array.
        assert_eq!(compute_diagnostics(&uri(), CLEAN), vec![]);
    }

    #[test]
    fn ranges_count_utf16_code_units_on_non_ascii_lines() {
        // AC8 (function level): a 😀 (2 UTF-16 units, 4 bytes, 1 scalar)
        // inside a string ahead of the duplicated key shifts the reported
        // column by exactly 2 units — asserting the UTF-16 count is
        // distinct from both the byte and scalar interpretations.
        let source = "struct s size=2x2\n  door id=\"😀\" id=x\n";
        let diagnostics = compute_diagnostics(&uri(), source);
        let dup = diagnostics
            .iter()
            .find(|d| code_of(d) == "E_DUPLICATE_ARG")
            .expect("E_DUPLICATE_ARG reported");
        let line = "  door id=\"😀\" id=x";
        let byte_col = line.find("id=x").expect("second id");
        let utf16_col = line[..byte_col].encode_utf16().count();
        let scalar_col = line[..byte_col].chars().count();
        assert_ne!(utf16_col, byte_col, "test must discriminate byte columns");
        assert_ne!(
            utf16_col, scalar_col,
            "test must discriminate scalar columns",
        );
        assert_eq!(dup.range.start.line, 1);
        assert_eq!(
            dup.range.start.character,
            u32::try_from(utf16_col).expect("fits u32"),
        );
    }
}
