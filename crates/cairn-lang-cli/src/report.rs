//! How findings reach the reader: gcc-style lines on stderr, or JSON documents on stdout.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::{DiagnosticNote as Note, LineStarts};
use cairn_lang_core::{ParseError, Severity, diagnose_parse_failure};
use cairn_lang_formats::build::finding::{Finding, Found, render_findings};
use cairn_lang_formats::build::run_level::{RunLevelAt, RunLevelMessage};
use cairn_lang_formats::info::{FailureDocument, InfoSink};

/// Where a command reports the findings that stopped it: prose on stderr, or the one JSON
/// document `spec/lint` "Machine-readable payload" promises on stdout.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum FailureReport {
    Text,
    Json,
}

/// Stderr, as an info run reports to it: each finding gcc-style, each line as written.
pub(crate) struct Stderr<'a> {
    pub(crate) file: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) lines: &'a LineStarts,
}

impl InfoSink for Stderr<'_> {
    fn finding(&mut self, finding: &Found) {
        report_diagnostic(self.file, self.source, self.lines, finding);
    }

    fn line(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

/// Report one finding and its notes on stderr, gcc-style.
pub(crate) fn report_diagnostic(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    d: &(impl Finding + ?Sized),
) {
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

/// Report the findings that stopped a command, and return its exit code.
pub(crate) fn report_failure_document(
    file: &Path,
    source: &str,
    report: FailureReport,
    diagnostics: &[impl Finding],
) -> ExitCode {
    let lines = LineStarts::new(source);
    match report {
        FailureReport::Text => {
            for d in diagnostics {
                report_diagnostic(file, source, &lines, d);
            }
        }
        FailureReport::Json => {
            let document = FailureDocument {
                diagnostics: render_findings(source, &lines, diagnostics),
            };
            if let Err(code) = print_json("diagnostics", &document) {
                return code;
            }
        }
    }
    ExitCode::from(1)
}

/// Report a run-level refusal or warning on stderr, gcc-style.
pub(crate) fn report_run_level(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    message: &RunLevelMessage,
) {
    for line in message.lines(&file.display().to_string(), source, lines) {
        eprintln!("{line}");
    }
}

/// Report a run-level message about a structure, which names no place in the source.
pub(crate) fn report_structure_message(message: &RunLevelMessage) {
    debug_assert_eq!(message.at, RunLevelAt::Nothing, "{message:?} names a place");
    report_run_level(Path::new(""), "", &LineStarts::new(""), message);
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
