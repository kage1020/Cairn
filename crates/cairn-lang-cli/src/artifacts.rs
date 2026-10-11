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
    // `Path::parent` returns `Some("")` for a bare filename like `foo.crn`;
    // treat that as "current directory" so the obvious one-file invocation
    // still works.
    Some(if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    })
}

/// Check every structure against the backend up front. A backend error
/// here (abstract palette entry, stateful Bedrock entry, dimension
/// overflow, a grid that disagrees with its dims) must not leave
/// half-written artifacts behind, so the function holds off all I/O until
/// it knows the IR is serialisable: once every structure is prepared, only
/// I/O can fail its write. It builds no per-voxel tag tree: a structure's
/// per-voxel lists are encoded while it is written, so a build holds one
/// voxel's entry at a time rather than every structure's tree at once.
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
            // `id` is the block id, so a tool can group the warnings by
            // block as well as by scope. The message names the entry itself,
            // as `id[states]`, so the id is on the line twice.
            eprintln!(
                "warning[W_INTENT_DEGRADED]: {scope}: {id}: {message}",
                id = note.id,
                message = note.message,
            );
        }
        let path = artifact_path(out_dir, scope, &output_filename(scope, target.output_ext()))?;
        // Distinct scope keys can name one file, because
        // `cairn_lang_core::artifact_stem` drops what keeps them apart: a
        // placement is named after its `id=` without its site, so it
        // meets a struct or another site's placement of that name, and a
        // walkway joins place and port with the `_` an id may itself
        // carry. The resolver refuses each such pair it counts as
        // `E_OUTPUT_NAME_COLLISION`, but it counts the scopes it resolved,
        // and these keys come from lowering, which is a different set. So
        // this check is what is left if a future source of scope keys
        // skips the resolver's, and it is the last point before the
        // second file overwrites the first in the commit.
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
/// The separator check comes first and is textual, so the same names are
/// refused on every host: `Path::components` alone is host-dependent, since
/// on Unix `C:hut` and `a\b` are each one `Normal` component. What remains
/// is the shapes with no separator that are still not a file.
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
/// `check` already refuses a `place id=` carrying a path separator
/// (`E_INVALID_PLACE_ID`), so this is the second line rather than the first:
/// it keeps a future source of file names that skips the identifier
/// newtypes from choosing the directory the compiler writes to. The file
/// name is checked rather than the joined path, because `Path::join` with
/// an absolute argument discards `out_dir` and a relative one with a
/// separator lands in a subdirectory. [`artifact_name_refusal`] refuses
/// `/`, `\` and `:` textually, so a name refused on one host is refused on
/// every host.
///
/// It is narrower than the identifier rule, and does not share its
/// predicate, on purpose. That rule also refuses `.`, which every artifact
/// name carries before its extension, and the characters a Windows file
/// name cannot carry, which make the write fail rather than land somewhere
/// else — and a failed write is reported. This line is about where the
/// file goes, so it holds for those three characters only, and the rest of
/// the identifier rule has one line, not two.
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

/// Refuse a `--lock` that would land on a path the artifacts already own.
///
/// Two staged entries with one destination overwrite each other's bytes,
/// and during the commit the second deletes the backup the first took —
/// destroying the previous build's artifact with no copy left. The scratch
/// names count too: `--lock out/home1.nbt.tmp` collides during staging
/// rather than during the commit.
///
/// Paths are compared by the directory entry they name, not by how they are
/// spelled: `./out/home1.nbt`, an absolute path, and one through a symlinked
/// `--out` all name the same file as `out/home1.nbt`. See
/// [`entry_location`].
///
/// A location that cannot be resolved is not treated as free, since a key
/// that could not be computed is one that cannot match. For an artifact it
/// is an error: `--out` was just created, so failing to resolve it is not an
/// ordinary state. For the lockfile it is a warning that the check could not
/// be made, and the build goes on as it did before the check existed.
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
/// Only the parent is canonicalised. The file itself usually does not exist
/// yet, and when it is a symlink the commit renames the link rather than
/// writing through it, so the entry — not what it points at — is what two
/// staged files would fight over. Every path this is asked about either has
/// an existing parent or is already doomed: `--out` has been created by the
/// time it runs, and a lockfile whose directory is missing fails while
/// staging, before anything is renamed. So only `NotFound` falls back, to
/// the parent's absolute spelling, which still makes `./x` and `x` agree.
/// Any other failure is returned rather than papered over, as the staging
/// module's `occupant` does: a key computed some other way would not match
/// the canonical one for the same file, and the caller would read that as
/// "no collision".
///
/// macOS and Windows file systems are case-insensitive by default, so there
/// the file name is compared case-folded: `OUT/HOME1.nbt` is `out/home1.nbt`.
/// Only the name needs folding, because `canonicalize` returns the directory
/// in the case the file system recorded it, so `OUT` and `out` already
/// resolve alike. `to_lowercase` is not the file system's own fold (NTFS's
/// upcase table, APFS's fold plus normalisation), but artifact names come
/// from ASCII identifiers, where the two agree. The choice is by platform,
/// not by volume: a case-sensitive volume on either costs at most a spurious
/// refusal, and a case-insensitive one elsewhere (casefold ext4, exFAT or
/// CIFS on Linux) is not folded, so the collision there goes unseen.
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
/// Two phases. Staging writes each file beside where it belongs, so every
/// way the encode or the write can fail happens while the previous build is
/// still untouched. Committing is renames only; [`staging::StagedSet::commit`]
/// describes what makes that step recoverable.
///
/// What the phases buy is a place to fail from. The single-phase version
/// wrote each artifact atomically but *individually*, so by the time the
/// lockfile failed some destinations had already been overwritten, and the
/// only undo available was deleting them.
pub(crate) fn write_artifacts_and_lock(
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    source: &str,
    block_ir: &BlockArrayIr,
    edition: EditionArg,
    target: &ResolvedTarget,
    lock_path: &Path,
) -> ExitCode {
    // Phase 1 — stage. Nothing a previous build produced is touched, so a
    // failure here costs only our own scratch.
    let mut staged = staging::StagedSet::default();
    for (path, structure) in prepared {
        if let Err(err) = staged.stage(path, staging::Kind::Artifact, |file| structure.write(file))
        {
            staged.discard();
            eprintln!("error: writing `{}`: {err}", path.display());
            return ExitCode::from(1);
        }
    }

    // Encode before touching the filesystem: a hash or YAML failure has to
    // be indistinguishable from never having started.
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

    // Phase 2 — commit. Renames only, each keeping what it replaces until
    // the whole set has landed.
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
