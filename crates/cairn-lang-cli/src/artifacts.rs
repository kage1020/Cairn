//! Where each structure artifact goes, and writing the artifacts and the lockfile as one set.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_formats::build::artifact::{PreparedArtifact, prepare_artifact};
use cairn_lang_formats::build::run_level::{RunLevelNote, artifact_collision, backend_refusal};
use cairn_lang_formats::data_version::{JavaTarget, ResolvedTarget};
use cairn_lang_formats::file_identity::FileIdentity;
use cairn_lang_formats::java_structure::{OutputExt, output_filename};
use cairn_lang_formats::litematic::{LitematicError, litematic_files, prepare_litematic};
use cairn_lang_formats::schem::{SchemError, prepare_schem, schem_files};
use cairn_lang_formats::target::PreparedStructure;
use cairn_lang_formats::write_path::{self, WritePath};

use crate::cli::StructureFormat;
use crate::lockfile::build_lockfile;
use crate::name_case::NameCase;
use crate::report::report_structure_message;
use crate::staging;

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

/// What a build's artifacts are checked against and written by: its target with, on Java, the
/// file format.
#[derive(Clone, Copy)]
pub(crate) enum Backend<'t> {
    /// A structure file per array, in the target's edition's own format.
    PerArray(&'t ResolvedTarget),
    /// A `.litematic` per struct and per site.
    Litematic(&'t JavaTarget),
    /// A `.schem` per array.
    Schem(&'t JavaTarget),
}

impl<'t> Backend<'t> {
    /// The backend `target` writes through, `format` picking Java's; Bedrock has one format, and
    /// `cairn compile` refuses `--structure-format` beside it.
    pub(crate) fn new(target: &'t ResolvedTarget, format: StructureFormat) -> Self {
        match (target, format) {
            (ResolvedTarget::Java(java), StructureFormat::Litematic) => Self::Litematic(java),
            (ResolvedTarget::Java(java), StructureFormat::Schem) => Self::Schem(java),
            (ResolvedTarget::Java(_), StructureFormat::Nbt) | (ResolvedTarget::Bedrock(_), _) => {
                Self::PerArray(target)
            }
        }
    }
}

/// Check every artifact against its backend before any I/O, so only I/O can fail a write.
pub(crate) fn prepare_artifacts<'a>(
    block_ir: &'a BlockArrayIr,
    backend: Backend<'_>,
    out_dir: &Path,
    case: NameCase,
) -> Result<Vec<(PathBuf, PreparedStructure<'a>)>, PrepareRefusal> {
    let mut prepared = Vec::with_capacity(block_ir.structures.len());
    let mut claims = Claims {
        out_dir,
        case,
        litematic: matches!(backend, Backend::Litematic(_)),
        seen: std::collections::HashMap::with_capacity(block_ir.structures.len()),
    };
    let target = match backend {
        Backend::PerArray(target) => target,
        Backend::Litematic(java) => {
            for file in litematic_files(block_ir) {
                let (scope, stem) = (file.scope.clone(), file.stem.clone());
                let schematic = prepare_litematic(file, java).map_err(|err| {
                    let mut refusal = backend_refusal(&scope, &err);
                    if let Some(fix) = litematic_fix(&err) {
                        refusal.notes.push(RunLevelNote::note(fix));
                    }
                    report_structure_message(&refusal);
                    ExitCode::from(1)
                })?;
                let path = claims.claim(&scope, &output_filename(&stem, OutputExt::Litematic))?;
                prepared.push((path, PreparedStructure::Litematic(schematic)));
            }
            note_entities_left_out(block_ir, ".litematic");
            return Ok(prepared);
        }
        Backend::Schem(java) => {
            for file in schem_files(block_ir) {
                let scope = file.scope;
                let file_name = output_filename(&file.stem, OutputExt::Schem);
                let schematic = prepare_schem(file, java).map_err(|err| {
                    let mut refusal = backend_refusal(scope, &err);
                    if let Some(fix) = schem_fix(&err) {
                        refusal.notes.push(RunLevelNote::note(fix));
                    }
                    report_structure_message(&refusal);
                    ExitCode::from(1)
                })?;
                prepared.push((
                    claims.claim(scope, &file_name)?,
                    PreparedStructure::Schem(schematic),
                ));
            }
            note_entities_left_out(block_ir, ".schem");
            return Ok(prepared);
        }
    };
    for (scope, array) in &block_ir.structures {
        let PreparedArtifact {
            file_name,
            structure,
            degraded,
        } = prepare_artifact(block_ir, scope, array, target).map_err(|refusal| {
            report_structure_message(&refusal);
            ExitCode::from(1)
        })?;
        for warning in &degraded {
            report_structure_message(warning);
        }
        prepared.push((claims.claim(scope, &file_name)?, structure));
    }
    Ok(prepared)
}

/// Say how many of the entities `block_ir` places a file of `format`, which writes none, leaves out
/// (`spec/ecosystem-interop` "Litematica").
fn note_entities_left_out(block_ir: &BlockArrayIr, format: &str) {
    let left_out: usize = block_ir
        .structures
        .values()
        .map(|array| array.entities.len())
        .sum();
    let (entities, are) = match left_out {
        0 => return,
        1 => ("entity", "is"),
        _ => ("entities", "are"),
    };
    eprintln!(
        "note: a `{format}` file writes no entities, so the {left_out} {entities} the build places \
         {are} left out; `--structure-format nbt` writes them"
    );
}

/// How to build a site `refusal` keeps from being one `.litematic`, when another format can.
fn litematic_fix(refusal: &LitematicError) -> Option<&'static str> {
    match refusal {
        LitematicError::CornerOverflow { .. } => Some(
            "move the placement nearer the site's origin, or compile with `--structure-format \
             nbt`, which records only where each file's minimum corner sits",
        ),
        LitematicError::MetadataOverflow { .. } => Some(
            "split the site into smaller ones, or compile with `--structure-format nbt`, which \
             writes a file per placement and walkway",
        ),
        _ => None,
    }
}

/// How to build a structure `refusal` keeps from being a `.schem`, when another format can.
fn schem_fix(refusal: &SchemError) -> Option<&'static str> {
    match refusal {
        SchemError::DimensionOverflow { .. } => Some(
            "split the structure along that axis, or compile with `--structure-format nbt`, \
             which stores each extent as an int",
        ),
        SchemError::UnspellableState { .. } => Some(
            "compile with `--structure-format nbt` or `litematic`, which store each property \
             apart rather than spelled into one key",
        ),
        SchemError::BlockEntityWithoutId { .. } => Some(
            "name the block entity in its `raw_block`'s `nbt=` with a string `id`, such as \
             `id:\"minecraft:chest\"`",
        ),
        SchemError::DuplicateState { .. } => Some(
            "every palette the compiler builds holds each state once, so this is a bug in Cairn \
             rather than in the source; please report it with the source that produced it",
        ),
        _ => None,
    }
}

/// The directory entries the artifacts prepared so far write, each with the scope that claimed it.
struct Claims<'o> {
    out_dir: &'o Path,
    case: NameCase,
    litematic: bool,
    seen: std::collections::HashMap<EntryLocation, (PathBuf, String)>,
}

impl Claims<'_> {
    /// Where `scope`'s artifact `file_name` goes, refused when an earlier scope's goes there too.
    fn claim(&mut self, scope: &str, file_name: &str) -> Result<PathBuf, PrepareRefusal> {
        let path = artifact_path(self.out_dir, scope, file_name)?;
        // Distinct scope keys can name one file, and the resolver's `E_OUTPUT_NAME_COLLISION`
        // sees only resolved scopes.
        let location = entry_location(&path, self.case).map_err(|err| {
            eprintln!(
                "error: cannot resolve where artifact `{}` would be written: {err}",
                path.display(),
            );
            eprintln!("  note: check that `--out` is readable, then run the build again");
            ExitCode::from(1)
        })?;
        if let Some((first_path, first)) =
            self.seen.insert(location, (path.clone(), scope.to_owned()))
        {
            let collision = ArtifactCollision {
                first,
                first_path,
                scope: scope.to_owned(),
                path,
                litematic: self.litematic,
            };
            return Err(PrepareRefusal::Collision(collision));
        }
        Ok(path)
    }
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
    litematic: bool,
}

impl ArtifactCollision {
    /// Print the refusal, with a note on the fix when the collision is one of a format or of a
    /// case-folding file system.
    pub(crate) fn report(&self) {
        let Self {
            first,
            first_path,
            scope,
            path,
            litematic,
        } = self;
        let mut refusal = artifact_collision(&path.display().to_string(), first, scope);
        // Only a site written as one `.litematic` takes a name a struct can also have.
        let site = first.starts_with("site::") || scope.starts_with("site::");
        if *litematic && site && first_path == path {
            refusal.notes.push(RunLevelNote::note(
                "rename the site or the struct, or compile with `--structure-format nbt`, which \
                 writes the site as a file per placement",
            ));
        }
        if first_path != path {
            refusal.notes.push(RunLevelNote::note(format!(
                "`{}` and `{}` name one file on this file system; rename one of the scopes so \
                 their names differ by more than case",
                first_path.display(),
                path.display(),
            )));
        }
        report_structure_message(&refusal);
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

/// Refuse a build that would write any of its files through a path reaching the source `file`
/// (`spec/versioning-editions` "Provenance and lock").
pub(crate) fn check_source_is_left_alone(
    file: &Path,
    prepared: &[(PathBuf, PreparedStructure<'_>)],
    lock_path: &Path,
) -> Result<(), ExitCode> {
    // A source that reaches no file any more is one no write can reach either.
    let Ok(source) = FileIdentity::of(file) else {
        return Ok(());
    };
    let artifacts = prepared
        .iter()
        .map(|(path, _)| (staging::Kind::Artifact, path.as_path()));
    for (kind, output) in artifacts.chain([(staging::Kind::Lockfile, lock_path)]) {
        let Some(reaching) = WritePath::reaching(&source, output) else {
            continue;
        };
        let (shown, file) = (output.display(), file.display());
        match reaching {
            WritePath::Final => {
                eprintln!("error: {kind} `{shown}` names `{file}`, the source being compiled");
            }
            WritePath::Scratch(scratch) => eprintln!(
                "error: {kind} `{shown}` would put its scratch file `{}` over `{file}`, the \
                 source being compiled",
                scratch.of(output).display(),
            ),
        }
        if kind == staging::Kind::Lockfile {
            // `--lock` reaching the source is refused before reading, so only a link gets here.
            eprintln!("  note: a link makes them one file; give `--lock` a `.lock` path elsewhere");
        } else {
            eprintln!(
                "  note: compile with `--out` naming another directory, or give the source a \
                 name ending in `.crn`"
            );
        }
        return Err(ExitCode::from(1));
    }
    Ok(())
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
        for reserved in write_path::reserved_paths(artifact) {
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
    for reserved in write_path::reserved_paths(lock_path) {
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
    /// A directory that exists.
    Existing(FileIdentity),
    /// The absolute path of a directory that does not exist.
    Absent(PathBuf),
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
    match FileIdentity::of(dir) {
        Ok(identity) => Ok(DirIdentity::Existing(identity)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::path::absolute(dir).map(DirIdentity::Absent)
        }
        Err(err) => Err(err),
    }
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
