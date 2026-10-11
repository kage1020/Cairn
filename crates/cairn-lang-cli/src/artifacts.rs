//! Where each structure artifact is written, and writing the artifacts
//! and the lockfile as one set.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_formats::bedrock_structure::McStructure;
use cairn_lang_formats::java_structure::{JavaStructure, output_filename};

use crate::cli::EditionArg;
use crate::lockfile::build_lockfile;
use crate::staging;
use crate::target::ResolvedTarget;

/// A structure its edition's backend has checked, waiting to be written.
pub(crate) enum PreparedStructure<'a> {
    /// Java vanilla structure (`.nbt`, gzip).
    Java(JavaStructure<'a>),
    /// Bedrock structure (`.mcstructure`, uncompressed).
    Bedrock(McStructure<'a>),
}

impl PreparedStructure<'_> {
    /// Write the structure in its edition's on-disk form: Java `.nbt` is
    /// gzip-wrapped big-endian, Bedrock `.mcstructure` is raw
    /// little-endian.
    fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        let encoded = match self {
            PreparedStructure::Java(structure) => structure.write_gzip(writer),
            PreparedStructure::Bedrock(structure) => structure.write(writer),
        };
        encoded.map_err(|e| std::io::Error::other(format!("nbt encode: {e}")))
    }
}

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
    // `Path::parent` is `Some("")` for a bare `foo.crn`: the current
    // directory.
    Some(if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    })
}

/// Check every structure against the backend before any I/O, so a backend
/// error (abstract palette entry, stateful Bedrock entry, dimension
/// overflow, a grid that disagrees with its dims) leaves no half-written
/// artifacts: once every structure is prepared, only I/O can fail a write.
/// No per-voxel tag tree is built; a structure is encoded as it is
/// written.
pub(crate) fn prepare_artifacts<'a>(
    block_ir: &'a BlockArrayIr,
    target: &ResolvedTarget,
    out_dir: &Path,
) -> Result<Vec<(PathBuf, PreparedStructure<'a>)>, PrepareRefusal> {
    let mut prepared = Vec::with_capacity(block_ir.structures.len());
    let mut seen_paths: std::collections::HashMap<
        (PathBuf, std::ffi::OsString),
        (PathBuf, String),
    > = std::collections::HashMap::with_capacity(block_ir.structures.len());
    for (scope, array) in &block_ir.structures {
        let (structure, degraded) = target.prepare(array).map_err(|err| {
            eprintln!("error: checking `{scope}`: {err}");
            ExitCode::from(1)
        })?;
        for note in degraded {
            // The id field repeats the message's `id[states]` so a tool can
            // group the warnings by block.
            eprintln!(
                "warning[W_INTENT_DEGRADED]: {scope}: {id}: {message}",
                id = note.id,
                message = note.message,
            );
        }
        let path = artifact_path(out_dir, scope, &output_filename(scope, target.output_ext()))?;
        // Distinct scope keys can name one file: `cairn_lang_core::artifact_stem`
        // drops a placement's site, and a walkway joins place and port with
        // a `_` an id may carry. The resolver's `E_OUTPUT_NAME_COLLISION`
        // counts resolved scopes, but these keys come from lowering, so this
        // is the last check before the commit overwrites the first file.
        let location = entry_location(&path).map_err(|err| {
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
    /// Two scopes name one directory entry. The caller prints it with
    /// [`ArtifactCollision::report`].
    Collision(ArtifactCollision),
    /// Any other refusal, already printed, with its exit code.
    Other(ExitCode),
}

/// Two scopes whose artifacts are one directory entry: the scope that
/// claimed it first and its path, then the one that met it and its path.
#[derive(Debug)]
pub(crate) struct ArtifactCollision {
    first: String,
    first_path: PathBuf,
    scope: String,
    path: PathBuf,
}

impl ArtifactCollision {
    /// Print the refusal. The note is for paths spelled differently, which
    /// only a case-folding file system makes one entry.
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
    /// The name carries `/`, `\` or `:`, the characters that make a name a
    /// path on some host: a directory separator, or a Windows drive prefix
    /// (`C:hut`) or alternate data stream (`hut.nbt:ads`).
    Separator(char),
    /// The name is empty, `.` or `..`: it names `--out` itself or its
    /// parent rather than a file inside it.
    NotAFile,
}

/// Decide whether `file_name` is a single plain file name.
///
/// The separator check is textual, so the same names are refused on every
/// host: on Unix, `Path::components` reads `C:hut` and `a\b` as one
/// `Normal` component each.
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

/// Join an artifact's file name onto `--out`, refusing any name that is not
/// a single plain file name.
///
/// A second line behind `check`'s `E_INVALID_PLACE_ID`, so that no source
/// of file names skipping the identifier newtypes can choose the directory
/// the compiler writes to. The name is checked rather than the joined
/// path, because `Path::join` with an absolute argument discards `out_dir`.
///
/// Narrower than the identifier rule on purpose: that rule also refuses
/// `.`, which every artifact name carries, and characters Windows cannot
/// store, which make the write fail and be reported rather than land
/// elsewhere. This line is only about where the file goes.
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

/// Refuse a `--lock` that would land on a path the artifacts already own,
/// scratch names included.
///
/// Two staged entries with one destination overwrite each other's bytes,
/// and during the commit the second deletes the backup the first took,
/// destroying the previous build's artifact. Paths are compared by the
/// directory entry they name, not by spelling ([`entry_location`]).
///
/// A location that cannot be resolved is not treated as free, since an
/// uncomputed key cannot match. For an artifact that is an error, as
/// `--out` was just created; for the lockfile it is a warning that the
/// check could not be made, and the build goes on.
pub(crate) fn check_lock_path_is_free(
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    lock_path: &Path,
) -> Result<(), ExitCode> {
    let mut reserved_paths: std::collections::HashMap<(PathBuf, std::ffi::OsString), &Path> =
        std::collections::HashMap::with_capacity(prepared.len() * 3);
    for (artifact, _) in prepared {
        for reserved in staging::reserved_paths(artifact) {
            let location = entry_location(&reserved).map_err(|err| {
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
        let location = match entry_location(&reserved) {
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

/// The directory entry `path` names, as a key two spellings of one file
/// compare equal on: the canonical parent directory and the file name.
///
/// Only the parent is canonicalised: the file usually does not exist yet,
/// and the commit renames a symlink rather than writing through it, so the
/// entry is what two staged files fight over. Only `NotFound` falls back,
/// to the parent's absolute spelling, since `--out` exists by now and a
/// lockfile whose directory is missing fails while staging. Any other
/// failure is returned: a key computed another way would not match the
/// canonical one and would read as "no collision".
///
/// On macOS and Windows, case-insensitive by default, the file name is
/// compared lowercased; `canonicalize` already returns the directory in its
/// recorded case. `to_lowercase` is not the file system's own fold, but
/// artifact names come from ASCII identifiers, where the two agree. The
/// choice is by platform, not volume: a case-sensitive volume there costs
/// at most a spurious refusal, and a case-insensitive one elsewhere
/// (casefold ext4, exFAT or CIFS on Linux) goes unfolded.
fn entry_location(path: &Path) -> std::io::Result<(PathBuf, std::ffi::OsString)> {
    let resolve = |dir: &Path| {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        match dir.canonicalize() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => std::path::absolute(dir),
            resolved => resolved,
        }
    };
    // No file name means the path ends in `..` or is a root; there is no
    // entry to split off, so the whole path is the location.
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok((resolve(path)?, std::ffi::OsString::new()));
    };
    let name = if cfg!(any(target_os = "macos", windows)) {
        name.to_string_lossy().to_lowercase().into()
    } else {
        name.to_os_string()
    };
    Ok((resolve(parent)?, name))
}

/// Write the prepared structure files and the lockfile as one set: either
/// every artifact plus the lock, or the directory exactly as it was.
///
/// Staging writes each file beside where it belongs, so every encode or
/// write failure happens while the previous build is untouched; writing
/// each file in place on its own would leave earlier destinations
/// overwritten when a later one failed. Committing is renames only;
/// [`staging::StagedSet::commit`] describes what makes it recoverable.
pub(crate) fn write_artifacts_and_lock(
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
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

    // Encoded before the commit, so a hash or YAML failure leaves the
    // directory as it was.
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
