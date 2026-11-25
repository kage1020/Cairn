//! What `cairn info` reports on a source, built on the [`build`](crate::build) passes and computed
//! once for every front end (`spec/versioning-editions` "The report in an editor").

mod floors;
mod inferred;
mod respelled;
mod rows;

use std::path::Path;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{LineStarts, RenderedDiagnostic};
use cairn_lang_core::lock::{LockReading, UnreadableKind};
use cairn_lang_core::resolve::{VersionAxes, compute_axes, resolve};
use cairn_lang_core::{Edition, Severity, diagnose_parse_failure, lower, parse};
use serde::Serialize;

use crate::build::finding::{Finding, Found};
use crate::build::gate::build_diagnostics;
use crate::build::run_level::RunLevelMessage;
use crate::file_identity::{Reach, Unidentified};
use crate::lock_path::{
    LockPathRefusal, default_lock_path, default_lock_refusal, lock_path_refusal,
};
use crate::registry::builtin_pack;
use crate::write_path::{Reached, WritePath};

pub use requested::{EditionsRefused, RequestedEditions, requested_editions};

/// Where an info run sends what its document does not carry, in the order `cairn info` prints it
/// on stderr.
pub trait InfoSink {
    /// A finding the run reports and carries on past.
    fn finding(&mut self, finding: &Found);
    /// A line as `cairn info` writes it on stderr, other than a finding's.
    fn line(&mut self, line: &str);
}

/// The source a run reads, and the sink it reports to.
pub struct Reporter<'a> {
    /// The path the notes name the source by.
    pub(crate) file: &'a Path,
    /// The source text.
    pub(crate) source: &'a str,
    /// The line starts of `source`.
    pub(crate) lines: &'a LineStarts,
    sink: &'a mut dyn InfoSink,
}

impl<'a> Reporter<'a> {
    /// A reporter naming `source` by `file`, reporting to `sink`.
    pub fn new(
        file: &'a Path,
        source: &'a str,
        lines: &'a LineStarts,
        sink: &'a mut dyn InfoSink,
    ) -> Self {
        Self {
            file,
            source,
            lines,
            sink,
        }
    }

    fn finding(&mut self, finding: &Found) {
        self.sink.finding(finding);
    }

    fn line(&mut self, line: &str) {
        self.sink.line(line);
    }

    /// Send `message` as its gcc-style lines.
    fn message(&mut self, message: &RunLevelMessage) {
        let file = self.file.display().to_string();
        for line in message.lines(&file, self.source, self.lines) {
            self.sink.line(&line);
        }
    }

    /// `file:line:col` for the byte offset `at`.
    fn at(&self, at: usize) -> String {
        format!(
            "{}:{}",
            self.file.display(),
            self.lines.position(self.source, at)
        )
    }
}

/// What an info run is asked: the source, the editions to report on, and the lockfile to read.
pub struct InfoRequest<'a> {
    /// The path the notes name the source by.
    pub file: &'a Path,
    /// The source text.
    pub source: &'a str,
    /// The editions to report on.
    pub editions: &'a RequestedEditions,
    /// The lockfile `--lock` names, or `None` for [`default_lock_path`] of `file`.
    pub lock: Option<&'a Path>,
    /// The format the run reports in.
    pub format: ReportFormat,
}

/// The `--format` a run reports in, which decides where an edition's errors go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// The text report: each error goes to the sink as it is found.
    Text,
    /// The JSON document: each error is held for the failure document.
    Json,
}

/// Why a run gave no report.
#[derive(Debug)]
#[non_exhaustive]
pub struct Refusal {
    /// The findings the sink was not sent: every edition-neutral finding when one is an error,
    /// else, in a JSON run, the errors an edition's lowering found.
    pub findings: Vec<Found>,
    /// The lines refusing each palette that carries blockstates a registry pack is expected to
    /// refuse, which no finding stands for (`spec/lint` "Machine-readable payload"); the sink was
    /// sent them too.
    pub palettes: Vec<String>,
}

impl Refusal {
    fn findings(findings: Vec<Found>) -> Self {
        Self {
            findings,
            palettes: Vec::new(),
        }
    }

    /// The failure document `--format json` prints for this refusal of `source`.
    #[must_use]
    pub fn document(&self, source: &str) -> FailureDocument {
        let lines = LineStarts::new(source);
        FailureDocument::new(
            self.findings
                .iter()
                .map(|d| d.render(source, &lines))
                .collect(),
        )
    }
}

/// The document `cairn info --format json` prints for a source it reports on.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct InfoDocument {
    /// The version axes, one row per field.
    #[serde(flatten)]
    pub axes: VersionAxes,
    /// The `verified target` row.
    pub verified_target: VerifiedTarget,
}

/// The lockfile the `verified target` row read, and what it says.
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct VerifiedTarget {
    /// The lockfile's path, as `--lock` named it or as [`default_lock_path`] gives it.
    pub path: String,
    /// What the lockfile says.
    #[serde(flatten)]
    pub reading: LockReading,
}

/// The document a `--format json` command prints in place of its product (`spec/lint`
/// "Machine-readable payload").
#[derive(Debug, Serialize)]
#[non_exhaustive]
pub struct FailureDocument {
    /// The findings that refused the command.
    pub diagnostics: Vec<RenderedDiagnostic>,
}

impl FailureDocument {
    /// The document carrying `diagnostics`.
    #[must_use]
    pub fn new(diagnostics: Vec<RenderedDiagnostic>) -> Self {
        Self { diagnostics }
    }
}

/// The report `cairn info` gives on `request`, its side findings and notes sent to `sink`.
///
/// # Errors
///
/// A [`Refusal`] when the source does not parse, a finding is an error, or a palette is refused.
pub fn info(request: &InfoRequest<'_>, sink: &mut dyn InfoSink) -> Result<InfoDocument, Refusal> {
    let source = request.source;
    let lines = LineStarts::new(source);
    let module = parse(source).map_err(|err| {
        Refusal::findings(vec![Found::Core(diagnose_parse_failure(
            source, &lines, &err,
        ))])
    })?;
    let ir = lower(&module);
    let asked = request.editions.as_slice();

    // The gate is edition-neutral; the rows run the strict per-edition pass.
    let resolution = resolve(&ir, None);
    // Unpinned: `info` spans every target, and pinning one would refuse ids fine on another.
    let mut block_ir = lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_pack(Edition::Java).view(None)),
    );
    // Only the requested editions: a Java-only finding must not refuse `--editions bedrock`.
    let combined = build_diagnostics(
        &module,
        &ir,
        None,
        asked,
        std::mem::take(&mut block_ir.diagnostics),
    );
    if combined.iter().any(|d| d.severity() == Severity::Error) {
        return Err(Refusal::findings(
            combined.into_iter().map(Found::Core).collect(),
        ));
    }

    let mut out = Reporter::new(request.file, source, &lines, sink);
    for d in &combined {
        out.finding(&Found::Core(d.clone()));
    }
    floors::report_floors_left_out_of_the_neutral_row(&mut out, &module);
    let rows = rows::edition_rows(&mut out, &module, &ir, asked, &combined, request.format)?;

    let axes = compute_axes(&module, rows);
    let lock = request
        .lock
        .map_or_else(|| default_lock_path(request.file), Path::to_path_buf);
    let reading = LockReading::of(&lock, source);
    let writer = LockWriter::of(request.file, request.lock);
    for note in verified_target_notes(request.file, &lock, &writer, &reading) {
        out.line(&note);
    }
    Ok(InfoDocument {
        axes,
        verified_target: VerifiedTarget {
            path: lock.display().to_string(),
            reading,
        },
    })
}

/// The compile that writes the lock a run read, which its notes name, or why none does.
#[derive(Debug)]
enum LockWriter {
    /// `cairn compile`, for `<source>.lock`.
    Plain,
    /// `cairn compile --lock <path>`, since a compile without the flag leaves that path alone.
    Flagged,
    /// None: `cairn compile` would write `<source>.lock` through this path, a link to the source.
    DefaultRefused(WritePath),
    /// None: `cairn compile` refuses the path `--lock` names.
    FlagRefused(LockPathRefusal),
    /// None: `cairn compile` cannot tell a path the lock is written through from the source.
    Unidentified(Unidentified),
}

impl LockWriter {
    /// What writes the lock of `source` at `lock`, the path `--lock` names, or at
    /// `<source>.lock` without one.
    fn of(source: &Path, lock: Option<&Path>) -> Self {
        let refused = match lock {
            None => default_lock_refusal(source).map(|path| path.map(Self::DefaultRefused)),
            Some(lock) => {
                lock_path_refusal(source, lock).map(|refusal| refusal.map(Self::FlagRefused))
            }
        };
        match (refused, lock) {
            (Ok(Some(refused)), _) => refused,
            (Ok(None), None) => Self::Plain,
            (Ok(None), Some(_)) => Self::Flagged,
            (Err(unidentified), _) => Self::Unidentified(unidentified),
        }
    }
}

/// The `note:` lines saying what the row alone does not: why there is no target, why the one it
/// names does not certify this source, which identifier the lock records that this Cairn refuses,
/// and why no compile writes the lock at its path.
fn verified_target_notes(
    file: &Path,
    lock: &Path,
    writer: &LockWriter,
    reading: &LockReading,
) -> Vec<String> {
    let path = lock.display();
    let compile = match writer {
        LockWriter::Plain => Some("`cairn compile`".to_owned()),
        LockWriter::Flagged => Some(format!("`cairn compile --lock {path}`")),
        LockWriter::DefaultRefused(_)
        | LockWriter::FlagRefused(_)
        | LockWriter::Unidentified(_) => None,
    };
    let next = |does: &str| {
        compile.as_ref().map_or_else(String::new, |compile| {
            format!("; the next {compile} {does}")
        })
    };
    let mut notes = match reading {
        LockReading::Missing => {
            let dir = lock
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let remedy = match &compile {
                None => String::new(),
                Some(compile) if dir.is_dir() => format!("; {compile} writes one"),
                Some(compile) => format!(
                    "; {} is not a directory, so {compile} cannot write one there",
                    dir.display()
                ),
            };
            vec![format!(
                "note: no lockfile at {path}, so no target is verified{remedy}"
            )]
        }
        LockReading::Read {
            source_unchanged,
            refused_identifier,
            ..
        } => {
            let refused = refused_identifier.iter().map(|refused| {
                format!(
                    "note: {path} records an identifier this Cairn refuses ({refused}){}",
                    next("replaces it"),
                )
            });
            let changed = (!source_unchanged).then(|| {
                format!(
                    "note: {} has changed since {path} was written, so its target was verified \
                     for another revision of the file{}",
                    file.display(),
                    next("rewrites the lock"),
                )
            });
            refused.chain(changed).collect()
        }
        LockReading::NewerSchema {
            lock_schema_version,
            supported,
        } => vec![format!(
            "note: {path} declares lock schema version {lock_schema_version}, and this Cairn \
             reads {supported}; the newer Cairn that wrote it can report it"
        )],
        LockReading::Unreadable {
            kind: UnreadableKind::Invalid,
            error,
        } => vec![format!(
            "note: {path}: the lockfile could not be read ({error}){}",
            next("replaces it"),
        )],
        LockReading::Unreadable {
            kind: UnreadableKind::Io,
            error,
        } => vec![format!(
            "note: {path}: the lockfile could not be read ({error})"
        )],
    };
    notes.extend(refused_lock_note(file, lock, writer));
    notes
}

/// The note saying why `cairn compile` writes no lock of `file` at `lock`, and what to pass
/// instead, or `None` when it writes one there.
fn refused_lock_note(file: &Path, lock: &Path, writer: &LockWriter) -> Option<String> {
    let (shown, source) = (lock.display(), file.display());
    let names = |path: WritePath| match path {
        WritePath::Final => format!("which names {source}, the source"),
        WritePath::Scratch(scratch) => format!(
            "whose scratch file {} names {source}, the source",
            scratch.of(lock).display(),
        ),
    };
    let linked = "a link makes them one file, so give `--lock` a `.lock` path elsewhere";
    let (why, fix) = match writer {
        LockWriter::Plain | LockWriter::Flagged => return None,
        LockWriter::Unidentified(Unidentified { path, error }) => {
            return Some(format!(
                "note: `cairn compile` refuses to write {shown}, since it cannot tell {} from \
                 {source}, the source ({error}); check that {} can be read, or give `--lock` a \
                 `.lock` path elsewhere",
                path.display(),
                path.display(),
            ));
        }
        LockWriter::DefaultRefused(path) => {
            return Some(format!(
                "note: `cairn compile` refuses to write {shown}, {}; {linked}",
                names(*path),
            ));
        }
        LockWriter::FlagRefused(LockPathRefusal::Source(Reached { path, by })) => (
            names(*path),
            match by {
                Reach::Name => "give `--lock` a `.lock` path".to_owned(),
                Reach::Link => linked.to_owned(),
            },
        ),
        LockWriter::FlagRefused(LockPathRefusal::SourceName) => (
            "which ends in `.crn` as a source does".to_owned(),
            format!(
                "give `--lock` a `.lock` path, such as {}",
                default_lock_path(lock).display(),
            ),
        ),
    };
    let leave_out = match default_lock_refusal(file) {
        Ok(None) => format!(
            ", or leave it out to read {}",
            default_lock_path(file).display()
        ),
        Ok(Some(_)) | Err(_) => String::new(),
    };
    Some(format!(
        "note: `cairn compile` refuses `--lock {shown}`, {why}; {fix}{leave_out}"
    ))
}

/// Its own module, so [`requested_editions`] is the only way to build a [`RequestedEditions`].
mod requested {
    use cairn_lang_core::{Edition, UnknownEdition};

    /// The distinct editions an `--editions` list names, in the order it first names them.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RequestedEditions(Vec<Edition>);

    impl RequestedEditions {
        /// The editions, in the order the list first names them.
        #[must_use]
        pub fn as_slice(&self) -> &[Edition] {
            &self.0
        }
    }

    /// Both editions, Java first, as `--editions` defaults to.
    impl Default for RequestedEditions {
        fn default() -> Self {
            Self(vec![Edition::Java, Edition::Bedrock])
        }
    }

    /// Why an editions list names nothing to report on.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum EditionsRefused {
        /// The list has no entry.
        Empty,
        /// An entry is empty or blank.
        EmptyEntry,
        /// An entry names no edition.
        Unknown(UnknownEdition),
    }

    /// Read an editions list as the distinct editions it asks about.
    ///
    /// # Errors
    ///
    /// An [`EditionsRefused`] for an empty list, an empty entry, or a name that is not an edition.
    pub fn requested_editions(editions: &[String]) -> Result<RequestedEditions, EditionsRefused> {
        if editions.is_empty() {
            return Err(EditionsRefused::Empty);
        }
        if editions.iter().any(|e| e.trim().is_empty()) {
            return Err(EditionsRefused::EmptyEntry);
        }
        let mut asked: Vec<Edition> = Vec::with_capacity(editions.len());
        for name in editions {
            let edition: Edition = name.parse().map_err(EditionsRefused::Unknown)?;
            if !asked.contains(&edition) {
                asked.push(edition);
            }
        }
        Ok(RequestedEditions(asked))
    }
}

/// What a run sent its sink: each finding as its code, and each line as written.
#[cfg(test)]
#[derive(Debug, Default)]
struct Recorded(Vec<String>);

#[cfg(test)]
impl InfoSink for Recorded {
    fn finding(&mut self, finding: &Found) {
        let code = match finding {
            Found::Core(d) => d.code.as_str(),
            Found::Circuit(d) => d.code.as_str(),
        };
        self.0.push(format!("finding {code}"));
    }

    fn line(&mut self, line: &str) {
        self.0.push(line.to_owned());
    }
}

#[cfg(test)]
mod tests;
