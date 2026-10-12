//! Source text → LSP diagnostics, from the pipeline `cairn check` runs.

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{
    Diagnostic as CoreDiagnostic, Severity, check, diagnose_parse_failure, lower, parse,
};

use crate::line_index::LineIndex;

/// The findings of `parse → lower → check` on `source`, as diagnostics in the document at `uri`.
#[must_use]
pub fn compute_diagnostics(uri: &lsp_types::Uri, source: &str) -> Vec<lsp_types::Diagnostic> {
    let index = LineIndex::new(source);
    let module = match parse(source) {
        Ok(module) => module,
        Err(err) => {
            let lines = LineStarts::new(source);
            let diagnostic = diagnose_parse_failure(source, &lines, &err);
            return vec![to_lsp_diagnostic(uri, source, &index, &diagnostic)];
        }
    };
    let ir = lower(&module);
    // No edition, as `cairn check` without `--edition`.
    check(&module, &ir, None)
        .iter()
        .map(|d| to_lsp_diagnostic(uri, source, &index, d))
        .collect()
}

/// Map one core [`CoreDiagnostic`] into the LSP shape.
fn to_lsp_diagnostic(
    uri: &lsp_types::Uri,
    source: &str,
    index: &LineIndex,
    diagnostic: &CoreDiagnostic,
) -> lsp_types::Diagnostic {
    let severity = match diagnostic.severity() {
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
        data: diagnostic.data.as_ref().and_then(|data| {
            // Logged rather than dropped silently, since a quick-fix reads this payload.
            match serde_json::to_value(data) {
                Ok(value) => Some(value),
                Err(err) => {
                    eprintln!("cairn-lsp: dropping unserialisable diagnostic data payload: {err}");
                    None
                }
            }
        }),
        ..lsp_types::Diagnostic::default()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::test_fixtures::{CLEAN, DUPLICATE};

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
        // The "expected one of" footer on E_UNKNOWN_KEYWORD has no span.
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
        let source = "struct s size=2x2\n\tfloor\n";
        let err = parse(source).expect_err("tab indent should be rejected");
        let diagnostics = compute_diagnostics(&uri(), source);
        assert_eq!(diagnostics.len(), 1);
        let d = &diagnostics[0];
        assert_eq!(d.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
        assert_eq!(d.message, err.user_message());
        assert_eq!(code_of(d), "E_PARSE");
        assert_eq!(d.range.start.line, 1);
        assert!(
            d.range.end > d.range.start,
            "range should be non-empty, got {:?}",
            d.range,
        );
    }

    #[test]
    fn artifacts_that_share_a_file_name_report_e_output_name_collision() {
        let source = "theme t:\n  slot floor -> @oak_planks\n\n\
                      struct hut size=3x3\n  floor mat_slot=floor\n\n\
                      def house size=3x3:\n  floor mat_slot=floor\n\n\
                      site s:\n  place id=hut use=house theme=t at=origin\n";
        let diagnostics = compute_diagnostics(&uri(), source);
        let found: Vec<&lsp_types::Diagnostic> = diagnostics
            .iter()
            .filter(|d| code_of(d) == "E_OUTPUT_NAME_COLLISION")
            .collect();
        assert_eq!(found.len(), 1, "one collision expected: {diagnostics:?}");
        assert_eq!(
            found[0].severity,
            Some(lsp_types::DiagnosticSeverity::ERROR)
        );
        assert_eq!(found[0].range.start.line, 10);
        let related = found[0]
            .related_information
            .as_ref()
            .expect("relatedInformation present");
        assert_eq!(
            related[0].location.range.start.line, 3,
            "the related pointer names the struct",
        );
    }

    #[test]
    fn clean_fixture_reports_no_diagnostics() {
        assert_eq!(compute_diagnostics(&uri(), CLEAN), vec![]);
    }

    #[test]
    fn ranges_count_utf16_code_units_on_non_ascii_lines() {
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

    #[test]
    fn a_diagnostic_covers_the_same_text_whatever_the_line_ending() {
        let base = "struct s size=2x2\n  floor\n  wals height=4\nstruct t size=3x3 size=4x4\n";
        let mut selected = Vec::new();
        for rendering in [
            base.to_owned(),
            base.replace('\n', "\r\n"),
            base.replace('\n', "\r"),
        ] {
            let source = rendering.as_str();
            let index = LineIndex::new(source);
            let covered: Vec<(String, String)> = compute_diagnostics(&uri(), source)
                .iter()
                .map(|d| {
                    let start = index
                        .offset_at(source, d.range.start)
                        .expect("range start is inside the document");
                    let end = index
                        .offset_at(source, d.range.end)
                        .expect("range end is inside the document");
                    (code_of(d).to_owned(), source[start..end].to_owned())
                })
                .collect();
            selected.push(covered);
        }
        assert_eq!(selected[1], selected[0], "CRLF disagrees with LF");
        assert_eq!(selected[2], selected[0], "lone CR disagrees with LF");
        assert!(
            selected[0]
                .iter()
                .any(|(code, text)| code == "E_UNKNOWN_KEYWORD" && text == "wals height=4"),
            "expected the unknown keyword itself to be covered, got {:?}",
            selected[0],
        );
    }

    #[test]
    fn a_parse_error_lands_on_its_own_line_whatever_the_line_ending() {
        let base = "struct s size=2x2\n  floor mat_slot=\n  walls height=4\n";
        for rendering in [
            base.to_owned(),
            base.replace('\n', "\r\n"),
            base.replace('\n', "\r"),
        ] {
            let source = rendering.as_str();
            let diagnostics = compute_diagnostics(&uri(), source);
            assert_eq!(diagnostics.len(), 1, "{source:?}");
            let range = diagnostics[0].range;
            // The end of `  floor mat_slot=`, not the start of line 2.
            assert_eq!(range.start.line, 1, "{source:?}");
            assert_eq!(range.start.character, 17, "{source:?}");
            assert_eq!(range.end, range.start, "{source:?}");
        }
    }

    #[test]
    fn a_parse_error_inside_a_line_covers_the_rest_of_it() {
        let source = "struct s size=2x2\n\tfloor\n";
        let diagnostics = compute_diagnostics(&uri(), source);
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].range;
        assert_eq!(range.start.line, 1);
        assert!(range.end > range.start, "{range:?}");
    }
}
