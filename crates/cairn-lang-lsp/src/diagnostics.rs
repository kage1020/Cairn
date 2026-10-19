//! Source text → LSP diagnostics, from the pipeline `cairn check` runs.

use cairn_lang_core::block_array::findings_without_target;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{
    Diagnostic as CoreDiagnostic, Severity, check, diagnose_parse_failure, lower, parse,
};
use cairn_lang_formats::builtin_pack;

use crate::line_index::LineIndex;

/// The findings of `parse → lower → check` on `source`, and of block-array lowering with no
/// target, as diagnostics in the document at `uri`.
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
    let mut findings = check(&module, &ir, None);
    findings.extend(findings_without_target(&ir, None, &|edition, id| {
        builtin_pack(edition).may_declare(id)
    }));
    findings
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
mod tests;
