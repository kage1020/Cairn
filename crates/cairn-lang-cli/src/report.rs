//! How findings reach the reader: gcc-style lines on stderr, or the
//! JSON documents `--format json` writes on stdout.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::{DiagnosticNote as Note, LineStarts, RenderedDiagnostic};
use cairn_lang_core::{Diagnostic, ParseError, Severity, diagnose_parse_failure};

/// How a command reports the findings that stopped it from producing
/// its product: as prose on stderr, or as the `{"diagnostics": [...]}`
/// document on stdout.
///
/// One type rather than a branch per format enum, because every command
/// routed here owes the same answer: `spec/lint` "Machine-readable
/// payload" promises one JSON document on stdout for every input,
/// failures included. `check` writes a bare array instead and has no
/// `failure_report()`, and `synth` reads its module as [`Self::Text`]
/// whatever it prints.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum FailureReport {
    /// Findings as prose on stderr: every format but JSON, and every
    /// command without `--format`.
    Text,
    /// The failure document on stdout.
    Json,
}

/// The four values a pass reports a finding from: `file` and `lines` turn
/// a byte offset into the `path:line:col` the finding prints with,
/// `source` is what the offset is into, and `report` says whether an
/// error-severity finding prints at all or is held for the failure
/// document. A warning prints on stderr whatever `report` holds.
#[derive(Copy, Clone)]
pub(crate) struct Reporting<'a> {
    pub(crate) file: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) lines: &'a LineStarts,
    pub(crate) report: FailureReport,
}

/// What a finding renders from, so the core and redstone diagnostic
/// types — which carry different code enums — report through one path.
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

/// Report one finding on stderr, in the gcc-style shape every subcommand
/// uses, with its notes under it.
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

/// Report a parse failure the way every other finding is reported.
pub(crate) fn report_parse_failure(file: &Path, source: &str, err: &ParseError) {
    let lines = LineStarts::new(source);
    let diagnostic = diagnose_parse_failure(source, &lines, err);
    report_diagnostic(file, source, &lines, &diagnostic);
}

/// Render diagnostics into the wire form `--format json` ships, which
/// carries the `line` / `col` / `end_line` / `end_col` a serialised
/// [`Diagnostic`] lacks.
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

/// Write one JSON document to stdout, or report why it could not be
/// serialised and give the caller an exit code. Every `--format json`
/// document goes through here so they all pretty-print the same way.
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

/// Report the findings that stopped a command from producing its
/// product, and give back its exit code.
///
/// The products of `info`, `parse` and `lower` are not diagnostics lists,
/// so under `--format json` a failure is a `{"diagnostics": [ ... ]}`
/// document of its own, told apart by its keys and the exit code; stdout
/// still carries one JSON document for every input. `check` does not come
/// through here: its product is the findings, so a failure is that array
/// with one more element.
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

/// Print one finding's `note:` lines under its primary.
///
/// A note with a span prints its position, since it names a second place
/// to look. One without is indented and unprefixed, so a footer does not
/// read as a second pointer at the primary span.
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
