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
/// One type rather than a branch per format enum, because the answer is
/// the same for every command that routes through here — `spec/lint`
/// "Machine-readable payload" promises one JSON document on stdout for
/// every input, and a failure is the run where that promise is easiest
/// to break.
///
/// Not every command that takes `--format json` is one of them. `check`
/// writes a bare array rather than this document and so has no
/// `failure_report()` at all, and `synth` reads its module as
/// [`Self::Text`] whatever it will go on to print. Both are stated where
/// they are done rather than promised here.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum FailureReport {
    /// Findings as prose on stderr. What every format that is not JSON
    /// does, and what a command with no `--format` at all does.
    Text,
    /// The failure document on stdout.
    Json,
}

/// The four values a pass reports a finding from: `file` and `lines` turn
/// a byte offset into the `path:line:col` the finding prints with,
/// `source` is what the offset is into, and `report` says whether an
/// error-severity finding prints at all or is held for the failure
/// document. A warning prints on stderr whatever `report` holds.
///
/// A bundle rather than four parameters in a fixed order, and only where
/// the list is long enough that the order would be the only thing holding
/// it together: the free functions that report still take them one by one.
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

/// Render diagnostics into the wire form `--format json` ships.
///
/// The `RenderedDiagnostic` form rather than the `Diagnostic` one, because
/// that is what carries `line` / `col` / `end_line` / `end_col`: serialising
/// the diagnostic itself would ship `code` / `severity` / `primary` /
/// `notes` with no source position at all.
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

/// The document `info --format json` writes where it has no report.
///
/// A struct rather than a `serde_json::json!` object or a format string,
/// so the findings render through the same serializer as every other
/// document.
#[derive(serde::Serialize)]
struct DiagnosticsDocument {
    diagnostics: Vec<RenderedDiagnostic>,
}

/// Report the findings that stopped a command from producing its
/// product, and give back its exit code.
///
/// `info`'s product is the `VersionAxes` document, `parse`'s is the AST
/// and `lower`'s is the block-array IR. None of the three is a
/// diagnostics list, so a failure cannot be "the product with a hole in
/// it". Under `--format json` it is a document of its own —
/// `{"diagnostics": [ ... ]}`, told apart from a product by its keys and
/// by the exit code — which is what keeps the flag's promise of one JSON
/// document on stdout for every input. Every other format reads as it
/// always has, on stderr.
///
/// `check` is the one command that does not come through here: its
/// product *is* the findings, so a failure is that array with one more
/// element in it rather than a document of another shape.
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
/// A note that carries a span is printed with that position, the way the
/// primary is: it names a second place in the file the reader has to go
/// look at. A note without one is indented and left unprefixed, so a
/// footer does not read as a second pointer at the primary span.
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
