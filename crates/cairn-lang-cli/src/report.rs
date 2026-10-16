//! How findings reach the reader: gcc-style lines on stderr, or JSON documents on stdout.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::{DiagnosticNote as Note, LineStarts, RenderedDiagnostic};
use cairn_lang_core::{Diagnostic, ParseError, Severity, diagnose_parse_failure};
use cairn_lang_formats::bedrock_state::degradation_detail;
use cairn_lang_formats::bedrock_structure::ParityNote;

/// Where a command reports the findings that stopped it: prose on stderr, or the one JSON
/// document `spec/lint` "Machine-readable payload" promises on stdout.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum FailureReport {
    Text,
    Json,
}

/// What a pass reports findings with; under [`FailureReport::Json`] errors wait for the failure
/// document, while warnings still print on stderr.
#[derive(Copy, Clone)]
pub(crate) struct Reporting<'a> {
    pub(crate) file: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) lines: &'a LineStarts,
    pub(crate) report: FailureReport,
}

/// A core or redstone diagnostic, as the reporters read it.
pub(crate) trait Finding {
    fn span_start(&self) -> usize;
    fn severity(&self) -> Severity;
    fn code_str(&self) -> &'static str;
    fn primary(&self) -> &str;
    fn notes(&self) -> &[Note];
}

impl Finding for Diagnostic {
    fn span_start(&self) -> usize {
        self.span.start
    }
    fn severity(&self) -> Severity {
        Diagnostic::severity(self)
    }
    fn code_str(&self) -> &'static str {
        self.code.as_str()
    }
    fn primary(&self) -> &str {
        &self.primary
    }
    fn notes(&self) -> &[Note] {
        &self.notes
    }
}

impl Finding for cairn_lang_redstone::Diagnostic {
    fn span_start(&self) -> usize {
        self.span.start
    }
    fn severity(&self) -> Severity {
        cairn_lang_redstone::Diagnostic::severity(self)
    }
    fn code_str(&self) -> &'static str {
        self.code.as_str()
    }
    fn primary(&self) -> &str {
        &self.primary
    }
    fn notes(&self) -> &[Note] {
        &self.notes
    }
}

/// Report one finding and its notes on stderr, gcc-style.
pub(crate) fn report_diagnostic(file: &Path, source: &str, lines: &LineStarts, d: &impl Finding) {
    eprintln!(
        "{}:{}: {}[{}]: {}",
        file.display(),
        lines.position(source, d.span_start()),
        d.severity().as_str(),
        d.code_str(),
        d.primary(),
    );
    report_notes(file, source, lines, d.notes());
}

/// Report every finding, and say whether any was an error.
pub(crate) fn report_diagnostics<D: Finding>(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    diagnostics: &[D],
) -> bool {
    let mut has_error = false;
    for d in diagnostics {
        report_diagnostic(file, source, lines, d);
        if d.severity() == Severity::Error {
            has_error = true;
        }
    }
    has_error
}

pub(crate) fn report_parse_failure(file: &Path, source: &str, err: &ParseError) {
    let lines = LineStarts::new(source);
    let diagnostic = diagnose_parse_failure(source, &lines, err);
    report_diagnostic(file, source, &lines, &diagnostic);
}

/// The `--format json` wire form, with the line and column a serialised [`Diagnostic`] lacks.
pub(crate) fn render_diagnostics(
    source: &str,
    lines: &LineStarts,
    diagnostics: &[Diagnostic],
) -> Vec<RenderedDiagnostic> {
    diagnostics
        .iter()
        .map(|d| d.render(source, lines))
        .collect()
}

/// Write one pretty-printed JSON document to stdout, or report why it could not be serialised.
pub(crate) fn print_json<T: serde::Serialize>(what: &str, value: &T) -> Result<(), ExitCode> {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            outln!("{json}");
            Ok(())
        }
        Err(err) => {
            eprintln!("error: failed to serialise {what} as JSON: {err}");
            Err(ExitCode::from(1))
        }
    }
}

/// The document a `--format json` command writes in place of its product.
#[derive(serde::Serialize)]
struct DiagnosticsDocument {
    diagnostics: Vec<RenderedDiagnostic>,
}

/// Report the findings that stopped a command, and return its exit code.
pub(crate) fn report_failure_document(
    file: &Path,
    source: &str,
    report: FailureReport,
    diagnostics: &[Diagnostic],
) -> ExitCode {
    let lines = LineStarts::new(source);
    match report {
        FailureReport::Text => {
            for d in diagnostics {
                report_diagnostic(file, source, &lines, d);
            }
        }
        FailureReport::Json => {
            let document = DiagnosticsDocument {
                diagnostics: render_diagnostics(source, &lines, diagnostics),
            };
            if let Err(code) = print_json("diagnostics", &document) {
                return code;
            }
        }
    }
    ExitCode::from(1)
}

/// A palette entry as `cairn info`'s notes and `W_INTENT_DEGRADED` name it, `id[key=value,...]`.
pub(crate) fn palette_entry_name(id: &str, states: &str) -> String {
    format!("{id}[{states}]")
}

/// The `W_INTENT_DEGRADED` line for one intent the structure `scope` dropped.
pub(crate) fn degraded_warning(scope: &str, note: &ParityNote) -> String {
    format!(
        "warning[W_INTENT_DEGRADED]: {scope}: {}: {}",
        palette_entry_name(&note.id, &note.states),
        degradation_detail(&note.dropped),
    )
}

/// A note without a span prints indented and unprefixed, so it does not read as a second pointer.
fn report_notes(file: &Path, source: &str, lines: &LineStarts, notes: &[Note]) {
    for note in notes {
        match note.span.as_ref() {
            Some(span) => {
                let pos = lines.position(source, span.start);
                eprintln!("{}:{}:   note: {}", file.display(), pos, note.message);
            }
            None => eprintln!("  note: {}", note.message),
        }
    }
}
