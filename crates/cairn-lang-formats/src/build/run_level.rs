//! What a run reports beside its findings: the run-level refusals and warnings that name no
//! span, which `spec/lint` "Machine-readable payload" keeps out of the findings array.

use cairn_lang_core::check::{LineStarts, RenderedNote};
use cairn_lang_core::{Diagnostic, DiagnosticCode, Edition, Severity};
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
    /// The line holding this byte offset: where the span the message is about starts.
    Offset(usize),
}

/// What a run-level message reports, which settles its code and severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLevelKind {
    /// `E_PARTIAL_BUILD`: scopes that produced no voxels.
    PartialBuild,
    /// `E_REQUIRES_CONFLICT`: a floor that admits no target declaring every id the build paints.
    RequiresConflict,
    /// `W_INTENT_DEGRADED`: an intent the target's structure file cannot carry.
    IntentDegraded,
    /// The finding by which the source's floors refuse the target, under its code.
    FloorRefusal(DiagnosticCode),
    /// A `--target` the edition's version table does not ship.
    UnsupportedTarget,
    /// A structure the target's backend refuses.
    BackendRefusal,
    /// Two structures whose files would share a name.
    ArtifactCollision,
}

impl RunLevelKind {
    /// The `E_*` / `W_*` code, when the message carries one.
    #[must_use]
    pub fn code(self) -> Option<&'static str> {
        match self {
            Self::PartialBuild => Some("E_PARTIAL_BUILD"),
            Self::RequiresConflict => Some("E_REQUIRES_CONFLICT"),
            Self::IntentDegraded => Some("W_INTENT_DEGRADED"),
            Self::FloorRefusal(code) => Some(code.as_str()),
            Self::UnsupportedTarget | Self::BackendRefusal | Self::ArtifactCollision => None,
        }
    }

    /// Whether the message refuses the run.
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::IntentDegraded => Severity::Warning,
            Self::FloorRefusal(code) => code.severity(),
            Self::PartialBuild
            | Self::RequiresConflict
            | Self::UnsupportedTarget
            | Self::BackendRefusal
            | Self::ArtifactCollision => Severity::Error,
        }
    }
}

/// One run-level refusal or warning: its first line, then the lines under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLevelMessage {
    /// What the message reports.
    pub kind: RunLevelKind,
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
    /// A `note:` line.
    Note {
        /// The byte offset of the line the note is about, when it is about one.
        at: Option<usize>,
        /// What the note says.
        text: String,
    },
    /// A `fix:` line.
    Fix(String),
    /// A line continuing the message, under no prefix.
    Continuation(String),
}

impl RunLevelNote {
    /// A `note:` line about no line of the source.
    #[must_use]
    pub fn note(text: impl Into<String>) -> Self {
        Self::Note {
            at: None,
            text: text.into(),
        }
    }
}

impl RunLevelMessage {
    /// The message as gcc-style lines, its location written as `file` and a position in `source`.
    #[must_use]
    pub fn lines(&self, file: &str, source: &str, lines: &LineStarts) -> Vec<String> {
        let code = self
            .kind
            .code()
            .map(|code| format!("[{code}]"))
            .unwrap_or_default();
        let at = match self.at {
            RunLevelAt::Nothing => String::new(),
            RunLevelAt::Source => format!("{file}: "),
            RunLevelAt::Offset(offset) => format!("{file}:{}: ", lines.position(source, offset)),
        };
        let mut out = vec![format!(
            "{}{code}: {at}{}",
            self.kind.severity().as_str(),
            self.primary
        )];
        out.extend(self.notes.iter().map(|note| match note {
            RunLevelNote::Note { at: None, text } => format!("  note: {text}"),
            RunLevelNote::Note { at: Some(at), text } => {
                format!("  note: {file}:{}: {text}", lines.position(source, *at))
            }
            RunLevelNote::Fix(text) => format!("  fix: {text}"),
            RunLevelNote::Continuation(text) => format!("  {text}"),
        }));
        out
    }

    /// The JSON form, shaped as a rendered finding is: a position only where the message has an
    /// offset, and each note's text without the `note:` its line carries.
    #[must_use]
    pub fn render(&self, source: &str, lines: &LineStarts) -> RenderedRunLevelMessage {
        let at = match self.at {
            RunLevelAt::Offset(offset) => Some(lines.position(source, offset)),
            RunLevelAt::Nothing | RunLevelAt::Source => None,
        };
        RenderedRunLevelMessage {
            code: self.kind.code(),
            severity: self.kind.severity(),
            line: at.map(|p| p.line.get()),
            col: at.map(|p| p.col.get()),
            primary: self.primary.clone(),
            notes: self
                .notes
                .iter()
                .map(|note| {
                    let (at, message) = match note {
                        RunLevelNote::Note { at, text } => (*at, text.clone()),
                        // A finding's fix is a note whose text starts `fix:`.
                        RunLevelNote::Fix(text) => (None, format!("fix: {text}")),
                        RunLevelNote::Continuation(text) => (None, text.clone()),
                    };
                    let at = at.map(|at| lines.position(source, at));
                    RenderedNote {
                        line: at.map(|p| p.line.get()),
                        col: at.map(|p| p.col.get()),
                        message,
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
        kind: RunLevelKind::PartialBuild,
        at: RunLevelAt::Source,
        primary: format!(
            "{} of {} requested scopes did not lower; {because}",
            dropped.len(),
            dropped.len() + built,
        ),
        notes: dropped
            .iter()
            .map(|scope| RunLevelNote::note(format!("`{scope}` produced no voxels")))
            .collect(),
    }
}

/// A `--target` the edition's version table does not ship.
#[must_use]
pub fn unsupported_target(err: &UnsupportedTarget) -> RunLevelMessage {
    RunLevelMessage {
        kind: RunLevelKind::UnsupportedTarget,
        at: RunLevelAt::Nothing,
        primary: err.to_string(),
        notes: Vec::new(),
    }
}

/// A structure `scope` the target's backend refuses, with the backend's reason.
#[must_use]
pub fn backend_refusal(scope: &str, reason: &dyn std::fmt::Display) -> RunLevelMessage {
    RunLevelMessage {
        kind: RunLevelKind::BackendRefusal,
        at: RunLevelAt::Nothing,
        primary: format!("checking `{scope}`: {reason}"),
        notes: Vec::new(),
    }
}

/// Two structures, `first` and then `scope`, whose files would both be `file_name`.
#[must_use]
pub fn artifact_collision(file_name: &str, first: &str, scope: &str) -> RunLevelMessage {
    RunLevelMessage {
        kind: RunLevelKind::ArtifactCollision,
        at: RunLevelAt::Nothing,
        primary: format!(
            "output filename `{file_name}` collides between scopes `{first}` and `{scope}`"
        ),
        notes: Vec::new(),
    }
}

/// A palette entry's name, `id[key=value,...]`.
#[must_use]
pub(crate) fn palette_entry_name(id: &str, states: &str) -> String {
    format!("{id}[{states}]")
}

/// `W_INTENT_DEGRADED` for one intent the structure `scope` dropped.
#[must_use]
pub(crate) fn degraded(scope: &str, note: &ParityNote) -> RunLevelMessage {
    RunLevelMessage {
        kind: RunLevelKind::IntentDegraded,
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
        kind: RunLevelKind::FloorRefusal(refusal.code),
        at: RunLevelAt::Offset(refusal.span.start),
        primary: format!("{}.", refusal.primary),
        notes: refusal
            .notes
            .iter()
            .map(|note| match &note.span {
                Some(span) => RunLevelNote::Note {
                    at: Some(span.start),
                    text: note.message.clone(),
                },
                None => RunLevelNote::Continuation(note.message.clone()),
            })
            .collect(),
    }
}

/// `E_REQUIRES_CONFLICT`: a floor that admits no `edition` target declaring every id the build
/// paints.
#[must_use]
pub(crate) fn requires_conflict(edition: Edition, conflict: &Conflict) -> RunLevelMessage {
    let Conflict { floor, ids } = conflict;
    let edition = edition.as_str();
    let bounding = &ids[0];
    let mut notes: Vec<RunLevelNote> = floor
        .inheritance()
        .map(RunLevelNote::Continuation)
        .into_iter()
        .collect();
    notes.extend(ids.iter().map(|b| RunLevelNote::Note {
        at: Some(b.painted.span.start),
        text: format!(
            "`{}` is last declared at {edition} {}",
            b.painted.id, b.version
        ),
    }));
    let replaced: Vec<String> = ids.iter().map(|b| format!("`{}`", b.painted.id)).collect();
    notes.push(RunLevelNote::Fix(format!(
        "lower the {} floor to {} or below, or replace {} with {} a {edition} target at or above \
         {} declares",
        floor.keyword(),
        bounding.version,
        replaced.join(", "),
        if ids.len() == 1 { "a block" } else { "blocks" },
        floor.version,
    )));
    RunLevelMessage {
        kind: RunLevelKind::RequiresConflict,
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
