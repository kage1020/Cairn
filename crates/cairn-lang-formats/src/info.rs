//! What `cairn info` reports on a source, built on the [`build`](crate::build) passes and computed
//! once for every front end (`spec/versioning-editions` "The report in an editor").

mod floors;
mod inferred;
mod respelled;
mod rows;

use std::path::{Path, PathBuf};

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{LineStarts, RenderedDiagnostic};
use cairn_lang_core::lock::LockReading;
use cairn_lang_core::resolve::{VersionAxes, compute_axes, resolve};
use cairn_lang_core::{Edition, Severity, diagnose_parse_failure, lower, parse};
use serde::Serialize;

use crate::build::finding::{Finding, Found};
use crate::build::gate::build_diagnostics;
use crate::build::run_level::RunLevelMessage;
use crate::registry::builtin_pack;

pub use requested::{RequestedEditions, requested_editions};

/// Where an info run sends what its document does not carry, in the order `cairn info` prints it
/// on stderr.
pub trait InfoSink {
    /// A finding the run reports and carries on past.
    fn finding(&mut self, finding: &Found);
    /// A `note:` line, or a line continuing the finding or note before it.
    fn line(&mut self, line: &str);
}

/// The source a run reads, and the sink it reports to.
pub struct Reporter<'a> {
    /// The path the notes name the source by.
    pub file: &'a Path,
    /// The source text.
    pub source: &'a str,
    /// The line starts of `source`.
    pub lines: &'a LineStarts,
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
    /// The lockfile the `verified target` row reads.
    pub lock: &'a Path,
    /// Hold each error for the failure document, leaving the sink the warnings and notes.
    pub errors_in_document: bool,
}

/// The document `cairn info --format json` prints for a source it reports on.
#[derive(Debug, Serialize)]
pub struct InfoDocument {
    /// The version axes, one row per field.
    #[serde(flatten)]
    pub axes: VersionAxes,
    /// The `verified target` row.
    pub verified_target: VerifiedTarget,
}

/// The lockfile the `verified target` row read, and what it says.
#[derive(Debug, Serialize)]
pub struct VerifiedTarget {
    /// The lockfile's path, as the request named it.
    pub path: String,
    /// What the lockfile says.
    #[serde(flatten)]
    pub reading: LockReading,
}

/// The document a `--format json` command prints in place of its product (`spec/lint`
/// "Machine-readable payload").
#[derive(Debug, Serialize)]
pub struct FailureDocument {
    /// The findings that refused the command.
    pub diagnostics: Vec<RenderedDiagnostic>,
}

impl FailureDocument {
    /// The document refusing `source` for `found`.
    #[must_use]
    pub fn new(source: &str, found: &[Found]) -> Self {
        let lines = LineStarts::new(source);
        Self {
            diagnostics: found.iter().map(|d| d.render(source, &lines)).collect(),
        }
    }
}

/// `<source>.lock`, keeping `.crn` so no two sources share a lock.
#[must_use]
pub fn default_lock_path(source: &Path) -> PathBuf {
    let mut path = source.as_os_str().to_owned();
    path.push(".lock");
    PathBuf::from(path)
}

/// The report `cairn info` gives on `request`, its side findings and notes sent to `sink`.
///
/// # Errors
///
/// The findings that refuse the run: every gate finding when one is an error, or the errors an
/// edition's lowering found, which is empty when they went to `sink` or a palette refused the
/// run.
pub fn info(
    request: &InfoRequest<'_>,
    sink: &mut dyn InfoSink,
) -> Result<InfoDocument, Vec<Found>> {
    let source = request.source;
    let lines = LineStarts::new(source);
    let module = parse(source)
        .map_err(|err| vec![Found::Core(diagnose_parse_failure(source, &lines, &err))])?;
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
        return Err(combined.into_iter().map(Found::Core).collect());
    }

    let mut out = Reporter::new(request.file, source, &lines, sink);
    for d in &combined {
        out.finding(&Found::Core(d.clone()));
    }
    floors::report_floors_left_out_of_the_neutral_row(&mut out, &module);
    let rows = rows::edition_rows(
        &mut out,
        &module,
        &ir,
        asked,
        &combined,
        request.errors_in_document,
    )?;

    let axes = compute_axes(&module, rows);
    let verified_target = VerifiedTarget {
        path: request.lock.display().to_string(),
        reading: LockReading::of(request.lock, source),
    };
    for note in verified_target_notes(request.file, &verified_target) {
        out.line(&note);
    }
    Ok(InfoDocument {
        axes,
        verified_target,
    })
}

/// The `note:` lines saying what the row alone does not: why there is no target, or why the one
/// it names does not certify this source.
fn verified_target_notes(file: &Path, verified: &VerifiedTarget) -> Vec<String> {
    let path = &verified.path;
    match &verified.reading {
        LockReading::Missing => vec![format!(
            "note: no lockfile at {path}, so no target is verified; `cairn compile` writes one"
        )],
        LockReading::Read {
            source_unchanged,
            refused_identifier,
            ..
        } => {
            let refused = refused_identifier.iter().map(|refused| {
                format!(
                    "note: {path} records an identifier this Cairn refuses ({refused}); the next \
                     `cairn compile` replaces it"
                )
            });
            let changed = (!source_unchanged).then(|| {
                format!(
                    "note: {} has changed since {path} was written, so its target was verified \
                     for another revision of the file; the next `cairn compile` rewrites the lock",
                    file.display(),
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
        LockReading::Unreadable { error } => vec![format!(
            "note: {path}: the lockfile could not be read ({error}); the next `cairn compile` \
             replaces it"
        )],
    }
}

/// Its own module, so [`requested_editions`] is the only way to build a [`RequestedEditions`].
mod requested {
    use cairn_lang_core::Edition;

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

    /// Read an `--editions` list as the distinct editions it asks about.
    ///
    /// # Errors
    ///
    /// The message refusing an empty entry or a name that is not an edition.
    pub fn requested_editions(editions: &[String]) -> Result<RequestedEditions, String> {
        // clap's comma delimiter turns `java,,bedrock` into an empty entry.
        if editions.iter().any(|e| e.trim().is_empty()) {
            return Err("--editions value must not contain empty entries".to_owned());
        }
        let mut asked: Vec<Edition> = Vec::with_capacity(editions.len());
        for name in editions {
            let edition: Edition = name.parse().map_err(|err| format!("{err}"))?;
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
