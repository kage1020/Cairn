//! Where each structure artifact goes, and writing the artifacts and the lockfile as one set.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_formats::java_structure::output_filename;

use crate::lockfile::build_lockfile;
use crate::name_case::NameCase;
use crate::report::degraded_warning;
use crate::staging;
use crate::target::{PreparedStructure, ResolvedTarget};

pub(crate) fn prepare_out_dir(file: &Path, requested: Option<&Path>) -> Result<PathBuf, ExitCode> {
    let Some(out_dir) = resolve_out_dir(file, requested) else {
        eprintln!(
            "error: source `{}` has no parent directory and --out was not given",
            file.display(),
        );
        return Err(ExitCode::from(1));
    };
    std::fs::create_dir_all(&out_dir).map_err(|err| {
        eprintln!(
            "error: cannot create output directory `{}`: {err}",
            out_dir.display(),
        );
        ExitCode::from(1)
    })?;
    Ok(out_dir)
}

fn resolve_out_dir(source: &Path, requested: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = requested {
        return Some(p.to_path_buf());
    }
    let parent = source.parent()?;
    // `Path::parent` is `Some("")` for a bare `foo.crn`.
    Some(if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    })
}

/// Check every structure against the backend before any I/O, so only I/O can fail a write.
pub(crate) fn prepare_artifacts<'a>(
    block_ir: &'a BlockArrayIr,
    target: &ResolvedTarget,
    out_dir: &Path,
    case: NameCase,
) -> Result<Vec<(PathBuf, PreparedStructure<'a>)>, PrepareRefusal> {
    let mut prepared = Vec::with_capacity(block_ir.structures.len());
    let mut seen_paths: std::collections::HashMap<EntryLocation, (PathBuf, String)> =
        std::collections::HashMap::with_capacity(block_ir.structures.len());
    for (scope, array) in &block_ir.structures {
        let (structure, degraded) = target.prepare(array).map_err(|err| {
            eprintln!("error: checking `{scope}`: {err}");
            ExitCode::from(1)
        })?;
        for note in &degraded {
            eprintln!("{}", degraded_warning(scope, note));
        }
        let path = artifact_path(out_dir, scope, &output_filename(scope, target.output_ext()))?;
        // Distinct scope keys can name one file, and the resolver's `E_OUTPUT_NAME_COLLISION`
        // sees only resolved scopes.
        let location = entry_location(&path, case).map_err(|err| {
            eprintln!(
                "error: cannot resolve where artifact `{}` would be written: {err}",
                path.display(),
            );
            eprintln!("  note: check that `--out` is readable, then run the build again");
            ExitCode::from(1)
        })?;
        if let Some((first_path, first)) =
            seen_paths.insert(location, (path.clone(), scope.clone()))
        {
            let collision = ArtifactCollision {
                first,
                first_path,
                scope: scope.clone(),
                path,
            };
            return Err(PrepareRefusal::Collision(collision));
        }
        prepared.push((path, structure));
    }
    Ok(prepared)
}

/// Why [`prepare_artifacts`] refused.
#[derive(Debug)]
pub(crate) enum PrepareRefusal {
    /// Two scopes name one directory entry; the caller prints it.
    Collision(ArtifactCollision),
    /// Any other refusal, already printed, with its exit code.
    Other(ExitCode),
}

/// Two scopes whose artifacts are one directory entry, the one that claimed it first leading.
#[derive(Debug)]
pub(crate) struct ArtifactCollision {
    first: String,
    first_path: PathBuf,
    scope: String,
    path: PathBuf,
}

impl ArtifactCollision {
    /// Print the refusal; the note covers spellings only a case-folding file system makes one.
    pub(crate) fn report(&self) {
        let Self {
            first,
            first_path,
            scope,
            path,
        } = self;
        eprintln!(
            "error: output filename `{}` collides between scopes `{first}` and `{scope}`",
            path.display(),
        );
        if first_path != path {
            eprintln!(
                "  note: `{}` and `{}` name one file on this file system; rename one of \
                 the scopes so their names differ by more than case",
                first_path.display(),
                path.display(),
            );
        }
    }
}

impl From<ExitCode> for PrepareRefusal {
    fn from(code: ExitCode) -> Self {
        Self::Other(code)
    }
}

/// Why an artifact file name cannot be joined onto `--out`.
#[derive(Debug, PartialEq, Eq)]
enum ArtifactNameRefusal {
    /// `/`, `\` or `:`: a directory separator, a drive prefix or an alternate data stream.
    Separator(char),
    /// Empty, `.` or `..`, naming `--out` itself or its parent.
    NotAFile,
}

/// Why `file_name` is not a single plain file name, checked textually so every host agrees.
fn artifact_name_refusal(file_name: &str) -> Option<ArtifactNameRefusal> {
    if let Some(ch) = file_name.chars().find(|c| matches!(c, '/' | '\\' | ':')) {
        return Some(ArtifactNameRefusal::Separator(ch));
    }
    let mut components = Path::new(file_name).components();
    let plain = matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(name)), None) if name == file_name
    );
    (!plain).then_some(ArtifactNameRefusal::NotAFile)
}

/// Join an artifact's file name onto `--out`, refusing any name that is not a single plain
/// file name.
fn artifact_path(out_dir: &Path, scope: &str, file_name: &str) -> Result<PathBuf, ExitCode> {
    match artifact_name_refusal(file_name) {
        None => Ok(out_dir.join(file_name)),
        Some(ArtifactNameRefusal::Separator(ch)) => {
            eprintln!(
                "error: scope `{scope}` names its artifact `{file_name}`, which contains `{ch}`, \
                 so it would not be written directly into `{}`",
                out_dir.display(),
            );
            eprintln!(
                "  note: every artifact is written directly into --out, named after its \
                 `place id=`, its struct name, or its walkway's site and endpoints; an id may \
                 not carry `/`, `\\` or `:`, so rename it (`home1`) and compile again",
            );
            Err(ExitCode::from(1))
        }
        Some(ArtifactNameRefusal::NotAFile) => {
            eprintln!(
                "error: scope `{scope}` names its artifact `{file_name}`, which is not a file \
                 name inside `{}`",
                out_dir.display(),
            );
            eprintln!(
                "  note: every name the checker accepts gives a file name with an extension, so \
                 this is a bug in Cairn rather than in the source; please report it with the \
                 source that produced it",
            );
            Err(ExitCode::from(1))
        }
    }
}

/// Refuse a `--lock` that would land on a path the artifacts already own, scratch names included.
pub(crate) fn check_lock_path_is_free(
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    lock_path: &Path,
    case: NameCase,
) -> Result<(), ExitCode> {
    let mut reserved_paths: std::collections::HashMap<EntryLocation, &Path> =
        std::collections::HashMap::with_capacity(prepared.len() * 3);
    for (artifact, _) in prepared {
        for reserved in staging::reserved_paths(artifact) {
            let location = entry_location(&reserved, case).map_err(|err| {
                eprintln!(
                    "error: cannot resolve where artifact `{}` would be written: {err}",
                    reserved.display(),
                );
                eprintln!("  note: check that `--out` is readable, then run the build again");
                ExitCode::from(1)
            })?;
            reserved_paths.insert(location, artifact);
        }
    }
    for reserved in staging::reserved_paths(lock_path) {
        let location = match entry_location(&reserved, case) {
            Ok(location) => location,
            Err(err) => {
                eprintln!(
                    "warning: cannot resolve where lockfile path `{}` would be written ({err}); \
                     could not check that it is not one of this build's artifacts",
                    reserved.display(),
                );
                return Ok(());
            }
        };
        if let Some(artifact) = reserved_paths.get(&location) {
            eprintln!(
                "error: lockfile path `{}` collides with an artifact this build writes (`{}`)",
                lock_path.display(),
                artifact.display(),
            );
            eprintln!(
                "  note: pass a `--lock` outside `--out`, or rename the struct whose artifact \
                 shares the name"
            );
            return Err(ExitCode::from(1));
        }
    }
    Ok(())
}

/// Where a directory entry lives: its parent directory and its file name as a [`NameCase`] keys it.
type EntryLocation = (DirIdentity, std::ffi::OsString);

/// A directory as the file system tells it apart, so every spelling that reaches it is one key.
#[derive(Debug, PartialEq, Eq, Hash)]
enum DirIdentity {
    /// The device and inode of a directory that exists.
    #[cfg(unix)]
    Inode(u64, u64),
    /// The canonical path of a directory that exists, or the absolute path of one that does not.
    Path(PathBuf),
}

/// The directory entry `path` names, as its parent's identity and its file name keyed by `case`.
fn entry_location(path: &Path, case: NameCase) -> std::io::Result<EntryLocation> {
    // A path ending in `..`, or a root, has no entry to split off.
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok((dir_identity(path)?, std::ffi::OsString::new()));
    };
    Ok((dir_identity(parent)?, case.key(name)))
}

/// How `dir`, or the current directory when it is empty, is told apart from every other.
///
/// Only `NotFound` falls back; an identity computed another way would read as "no collision".
fn dir_identity(dir: &Path) -> std::io::Result<DirIdentity> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    match existing_dir_identity(dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::path::absolute(dir).map(DirIdentity::Path)
        }
        identity => identity,
    }
}

/// `dir`'s device and inode, which `canonicalize` cannot stand in for: on Linux it keeps each
/// component's spelling, so `out` and `OUT` in a case-folding directory would be two keys.
#[cfg(unix)]
fn existing_dir_identity(dir: &Path) -> std::io::Result<DirIdentity> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(dir)?;
    Ok(DirIdentity::Inode(metadata.dev(), metadata.ino()))
}

/// `dir`'s canonical path, which spells each component as the directory stores it.
#[cfg(not(unix))]
fn existing_dir_identity(dir: &Path) -> std::io::Result<DirIdentity> {
    dir.canonicalize().map(DirIdentity::Path)
}

/// Write the prepared structures and the lockfile as one set: all of them, or the directory as
/// it was.
pub(crate) fn write_artifacts_and_lock(
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    source: &str,
    block_ir: &BlockArrayIr,
    edition: Edition,
    target: &ResolvedTarget,
    lock_path: &Path,
) -> ExitCode {
    let mut staged = staging::StagedSet::default();
    for (path, structure) in prepared {
        if let Err(err) = staged.stage(path, staging::Kind::Artifact, |file| structure.write(file))
        {
            staged.discard();
            eprintln!("error: writing `{}`: {err}", path.display());
            return ExitCode::from(1);
        }
    }

    let lock_body = match build_lockfile(source, block_ir, edition, target)
        .map_err(|err| err.to_string())
        .and_then(|lockfile| lockfile.to_yaml().map_err(|err| err.to_string()))
    {
        Ok(body) => body,
        Err(err) => {
            staged.discard();
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    if let Err(err) = staged.stage(lock_path, staging::Kind::Lockfile, |file| {
        std::io::Write::write_all(file, lock_body.as_bytes())
    }) {
        staged.discard();
        eprintln!("error: writing lockfile `{}`: {err}", lock_path.display());
        return ExitCode::from(1);
    }

    match staged.commit() {
        Ok(written) => {
            for path in written {
                outln!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("error: {failure}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests;
