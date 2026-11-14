//! Source text → LSP diagnostics, from the pipeline `cairn check` runs, or `cairn compile` for a
//! pinned target.

use cairn_lang_core::block_array::{findings_without_target, lower_to_block_array};
use cairn_lang_core::check::{LineStarts, weigh_target};
use cairn_lang_core::resolve::declared_version_floors;
use cairn_lang_core::{
    Diagnostic as CoreDiagnostic, Edition, Module, Severity, check, diagnose_parse_failure, lower,
    parse, resolve,
};
use cairn_lang_formats::builtin_pack;

use crate::line_index::LineIndex;
use crate::pin::Pin;

/// The findings of `parse → lower → check` on `source`, and of block-array lowering against what
/// `pin` pins, as diagnostics in the document at `uri`.
#[must_use]
pub fn compute_diagnostics(
    uri: &lsp_types::Uri,
    source: &str,
    pin: &Pin,
) -> Vec<lsp_types::Diagnostic> {
    let index = LineIndex::new(source);
    let module = match parse(source) {
        Ok(module) => module,
        Err(err) => {
            let lines = LineStarts::new(source);
            let diagnostic = diagnose_parse_failure(source, &lines, &err);
            return vec![to_lsp_diagnostic(uri, source, &index, &diagnostic)];
        }
    };
    findings(&module, pin)
        .iter()
        .map(|d| to_lsp_diagnostic(uri, source, &index, d))
        .collect()
}

/// What `cairn check` reports on `module` with `pin`'s edition, or, for a target, what
/// `cairn compile` refuses on before it writes anything.
fn findings(module: &Module, pin: &Pin) -> Vec<CoreDiagnostic> {
    let declared = |edition: Edition, id: &str| builtin_pack(edition).may_declare(id);
    let ir = lower(module);
    match pin {
        Pin::Edition(edition) => {
            let mut found = check(module, &ir, Some(*edition));
            found.extend(findings_without_target(&ir, Some(*edition), &declared));
            found
        }
        Pin::Target { edition, version } => {
            let pack = builtin_pack(*edition);
            let targetable: Vec<String> = pack
                .targetable()
                .into_iter()
                .map(|entry| entry.mc_version.clone())
                .collect();
            let refusal = weigh_target(
                &declared_version_floors(module, *edition),
                *edition,
                version,
                &targetable,
                &|edition| builtin_pack(edition).version_order(),
            );
            // A target a floor refuses pins no version (`spec/versioning-editions` "The declared
            // floor is enforced").
            let pinned = refusal.is_none().then_some(version.as_str());
            let mut found = check(module, &ir, Some(*edition));
            let resolution = resolve(&ir, Some(*edition));
            found.extend(
                lower_to_block_array(&ir, &resolution, Some(&pack.view(pinned))).diagnostics,
            );
            found.extend(refusal);
            found
        }
        Pin::Nothing => {
            let mut found = check(module, &ir, None);
            found.extend(findings_without_target(&ir, None, &declared));
            found
        }
    }
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
