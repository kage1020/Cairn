//! What a run reports beside its findings: the run-level refusals and warnings that name no
//! span, which `spec/lint` "Machine-readable payload" keeps out of the findings array.

use cairn_lang_core::check::{LineStarts, RenderedNote};
use cairn_lang_core::{Diagnostic, Edition, Severity};
use serde::Serialize;

use super::conflicts::Conflict;
use crate::bedrock_structure::ParityNote;
use crate::data_version::UnsupportedTarget;

/// Where a run-level message points, when it points anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLevelAt {
    /// Nowhere: the message is about the command line or a structure.
    Nothing,
    /// The source as a whole.
    Source,
    /// The byte offset of the line the message is about.
    Offset(usize),
}

/// One run-level refusal or warning: its first line, then the lines under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLevelMessage {
    /// Whether the message refuses the run.
    pub severity: Severity,
    /// The `E_*` / `W_*` code, when the message carries one.
    pub code: Option<&'static str>,
    /// Where it points.
    pub at: RunLevelAt,
    /// The first line's text, after its prefix.
    pub primary: String,
    /// The lines under the first.
    pub notes: Vec<RunLevelNote>,
}

/// One line under a run-level message's first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunLevelNote {
    /// A line as written after its indent: `note: ...`, `fix: ...`, or one continuing the message.
    Line(String),
    /// A `note:` about the line holding the byte offset `at`.
    At {
        /// The byte offset the note points at.
        at: usize,
        /// What the note says.
        note: String,
    },
}

impl RunLevelMessage {
    /// The message as gcc-style lines, its location written as `file` and a position in `source`.
    #[must_use]
    pub fn lines(&self, file: &str, source: &str, lines: &LineStarts) -> Vec<String> {
        let code = self
            .code
            .map(|code| format!("[{code}]"))
            .unwrap_or_default();
        let at = match self.at {
            RunLevelAt::Nothing => String::new(),
            RunLevelAt::Source => format!("{file}: "),
            RunLevelAt::Offset(offset) => format!("{file}:{}: ", lines.position(source, offset)),
        };
        let mut out = vec![format!(
            "{}{code}: {at}{}",
            self.severity.as_str(),
            self.primary
        )];
        out.extend(self.notes.iter().map(|note| match note {
            RunLevelNote::Line(line) => format!("  {line}"),
            RunLevelNote::At { at, note } => {
                format!("  note: {file}:{}: {note}", lines.position(source, *at))
            }
        }));
        out
    }

    /// The JSON form, shaped as a rendered finding is, with a position only when it has an offset.
    #[must_use]
    pub fn render(&self, source: &str, lines: &LineStarts) -> RenderedRunLevelMessage {
        let at = match self.at {
            RunLevelAt::Offset(offset) => Some(lines.position(source, offset)),
            RunLevelAt::Nothing | RunLevelAt::Source => None,
        };
        RenderedRunLevelMessage {
            code: self.code,
            severity: self.severity,
            line: at.map(|p| p.line.get()),
            col: at.map(|p| p.col.get()),
            primary: self.primary.clone(),
            notes: self
                .notes
                .iter()
                .map(|note| match note {
                    RunLevelNote::Line(line) => RenderedNote {
                        line: None,
                        col: None,
                        message: line.clone(),
                    },
                    RunLevelNote::At { at, note } => {
                        let position = lines.position(source, *at);
                        RenderedNote {
                            line: Some(position.line.get()),
                            col: Some(position.col.get()),
                            message: format!("note: {note}"),
                        }
                    }
                })
                .collect(),
        }
    }
}

/// The JSON form of a [`RunLevelMessage`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedRunLevelMessage {
    /// The `E_*` / `W_*` code, when the message carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    /// Whether the message refuses the run.
    pub severity: Severity,
    /// 1-based line of the line the message is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column of the same byte, in Unicode scalar values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// The first line's text.
    pub primary: String,
    /// The lines under it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<RenderedNote>,
}

/// `E_PARTIAL_BUILD`: the scopes a lowering lost, the line ending with `because`.
#[must_use]
pub fn partial_build(dropped: &[String], built: usize, because: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_PARTIAL_BUILD"),
        at: RunLevelAt::Source,
        primary: format!(
            "{} of {} requested scopes did not lower; {because}",
            dropped.len(),
            dropped.len() + built,
        ),
        notes: dropped
            .iter()
            .map(|scope| RunLevelNote::Line(format!("note: `{scope}` produced no voxels")))
            .collect(),
    }
}

/// A `--target` the edition's version table does not ship.
#[must_use]
pub fn unsupported_target(err: &UnsupportedTarget) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: err.to_string(),
        notes: Vec::new(),
    }
}

/// A structure `scope` the target's backend refuses, with the backend's reason.
#[must_use]
pub fn backend_refusal(scope: &str, reason: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: format!("checking `{scope}`: {reason}"),
        notes: Vec::new(),
    }
}

/// Two structures, `first` and then `scope`, whose files would both be `file_name`.
#[must_use]
pub fn artifact_collision(file_name: &str, first: &str, scope: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: format!(
            "output filename `{file_name}` collides between scopes `{first}` and `{scope}`"
        ),
        notes: Vec::new(),
    }
}

/// A palette entry's name, `id[key=value,...]`.
#[must_use]
pub fn palette_entry_name(id: &str, states: &str) -> String {
    format!("{id}[{states}]")
}

/// `W_INTENT_DEGRADED` for one intent the structure `scope` dropped.
#[must_use]
pub fn degraded(scope: &str, note: &ParityNote) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Warning,
        code: Some("W_INTENT_DEGRADED"),
        at: RunLevelAt::Nothing,
        primary: format!(
            "{scope}: {}: {}",
            palette_entry_name(&note.id, &note.states),
            note.dropped.reason,
        ),
        notes: Vec::new(),
    }
}

/// The refusal of a `--target` the source's floors rule out, `refusal` read as a run-level
/// message that ends its first line.
#[must_use]
pub fn floor_refusal_message(refusal: &Diagnostic) -> RunLevelMessage {
    RunLevelMessage {
        severity: refusal.severity(),
        code: Some(refusal.code.as_str()),
        at: RunLevelAt::Offset(refusal.span.start),
        primary: format!("{}.", refusal.primary),
        notes: refusal
            .notes
            .iter()
            .map(|note| RunLevelNote::Line(note.message.clone()))
            .collect(),
    }
}

/// `E_REQUIRES_CONFLICT`: a floor that admits no `edition` target declaring every id the build
/// paints.
#[must_use]
pub fn requires_conflict(edition: Edition, conflict: &Conflict) -> RunLevelMessage {
    let Conflict { floor, ids } = conflict;
    let edition = edition.as_str();
    let bounding = &ids[0];
    let mut notes: Vec<RunLevelNote> = floor
        .inheritance()
        .map(RunLevelNote::Line)
        .into_iter()
        .collect();
    notes.extend(ids.iter().map(|b| RunLevelNote::At {
        at: b.painted.span.start,
        note: format!(
            "`{}` is last declared at {edition} {}",
            b.painted.id, b.version
        ),
    }));
    let replaced: Vec<String> = ids.iter().map(|b| format!("`{}`", b.painted.id)).collect();
    notes.push(RunLevelNote::Line(format!(
        "fix: lower the {} floor to {} or below, or replace {} with {} a {edition} target at or \
         above {} declares",
        floor.keyword(),
        bounding.version,
        replaced.join(", "),
        if ids.len() == 1 { "a block" } else { "blocks" },
        floor.version,
    )));
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_REQUIRES_CONFLICT"),
        at: RunLevelAt::Offset(floor.span.start),
        primary: format!(
            "{} requires {}, but no {edition} target it admits declares `{}`, which this build \
             paints.",
            floor.declarer(),
            floor.rendered(),
            bounding.painted.id,
        ),
        notes,
    }
}

#[cfg(test)]
mod tests;
