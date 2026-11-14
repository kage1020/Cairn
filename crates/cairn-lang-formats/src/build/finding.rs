//! The findings a build reports: core's and those of the circuit a struct is built with.

use cairn_lang_core::Diagnostic;
use cairn_lang_core::Severity;
use cairn_lang_core::check::{DiagnosticNote, LineStarts, RenderedDiagnostic};

/// A core or redstone diagnostic, as a front end reports it.
pub trait Finding {
    /// The byte offset the primary span starts at.
    fn span_start(&self) -> usize;
    /// The finding's severity.
    fn severity(&self) -> Severity;
    /// The finding's `E_*` / `W_*` code.
    fn code_str(&self) -> &'static str;
    /// The primary message.
    fn primary(&self) -> &str;
    /// The notes under the primary message.
    fn notes(&self) -> &[DiagnosticNote];
    /// The `--format json` wire form, with the line and column a serialised finding lacks.
    fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic;
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
    fn notes(&self) -> &[DiagnosticNote] {
        &self.notes
    }
    fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic {
        Diagnostic::render(self, source, lines)
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
    fn notes(&self) -> &[DiagnosticNote] {
        &self.notes
    }
    fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic {
        cairn_lang_redstone::Diagnostic::render(self, source, lines)
    }
}

/// A finding of the core passes or of the circuit a struct is built with, for a list holding both.
#[derive(Debug, Clone, PartialEq)]
// The lint is quiet on wasm32, whose narrower pointers bring the variants within its threshold.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::large_enum_variant,
        reason = "a run reports a handful of findings, most of them core's"
    )
)]
pub enum Found {
    /// A finding of the core passes.
    Core(Diagnostic),
    /// A finding of the circuit a struct is built with.
    Circuit(cairn_lang_redstone::Diagnostic),
}

impl Found {
    fn finding(&self) -> &dyn Finding {
        match self {
            Self::Core(d) => d,
            Self::Circuit(d) => d,
        }
    }
}

impl Finding for Found {
    fn span_start(&self) -> usize {
        self.finding().span_start()
    }
    fn severity(&self) -> Severity {
        self.finding().severity()
    }
    fn code_str(&self) -> &'static str {
        self.finding().code_str()
    }
    fn primary(&self) -> &str {
        self.finding().primary()
    }
    fn notes(&self) -> &[DiagnosticNote] {
        self.finding().notes()
    }
    fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic {
        self.finding().render(source, lines)
    }
}

/// The `--format json` wire form of every finding, in order.
pub fn render_findings(
    source: &str,
    lines: &LineStarts,
    findings: &[impl Finding],
) -> Vec<RenderedDiagnostic> {
    findings.iter().map(|d| d.render(source, lines)).collect()
}
