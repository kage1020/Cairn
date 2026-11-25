//! `cairn install`: Java structure files placed in a world, where `/place template` loads them,
//! each held to the world's version (`spec/versioning-editions` "Cross-version application").

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::suggest::{candidate_list, did_you_mean, nearest_match};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};
use cairn_lang_formats::world::{
    CrossVersion, DEFAULT_NAMESPACE, IdPart, LEVEL_DAT, LevelDatError, Namespace,
    StructureFileError, StructureId, StructureIdError, WorldVersion, read_level_dat,
    read_structure_data_version, recompile_target, worlds_in,
};
use cairn_lang_formats::write_path::reserved_paths;

use crate::cli::InstallArgs;
use crate::staging::{Kind, StagedSet};

/// The exit code of a refused file or world.
const REFUSED: u8 = 1;

/// The exit code of a structure file or world that cannot be located, or unusable arguments.
const NOT_LOCATED: u8 = 2;

/// A structure file that passed every check, waiting to be written.
struct Planned {
    id: StructureId,
    bytes: Vec<u8>,
    target: PathBuf,
    replacing: bool,
}

/// The world a run installs into.
struct World<'a> {
    path: &'a Path,
    version: WorldVersion,
}

pub(crate) fn run_install(args: &InstallArgs) -> ExitCode {
    if args.name.is_some() && args.structures.len() > 1 {
        eprintln!(
            "error: `--name` names one structure file, and {} were given",
            args.structures.len(),
        );
        eprintln!(
            "  fix: install each file in a run of its own with `--name`, or leave `--name` out \
             to install each under its file name"
        );
        return ExitCode::from(NOT_LOCATED);
    }
    let names: Vec<String> = args
        .structures
        .iter()
        .map(|path| name_for(path, args.name.as_deref()))
        .collect();
    if let Some((first, second, name)) = shared_name(&args.structures, &names) {
        eprintln!(
            "error: `{}` and `{}` would both install as `{}:{name}`",
            first.display(),
            second.display(),
            args.namespace,
        );
        eprintln!("  fix: install one of them in a run of its own, with `--name`");
        return ExitCode::from(NOT_LOCATED);
    }
    let version = match read_level_dat(&args.world) {
        Ok(version) => version,
        Err(err) => {
            report_world(&args.world, &err);
            let code = if matches!(err, LevelDatError::Missing) {
                NOT_LOCATED
            } else {
                REFUSED
            };
            return ExitCode::from(code);
        }
    };
    let world = World {
        path: &args.world,
        version,
    };
    let namespace = Namespace::new(&args.namespace)
        .inspect_err(|err| report_invalid_namespace(&args.namespace, err))
        .ok();
    let pack = builtin_pack(Edition::Java);
    let mut planned = Vec::new();
    let mut refused: Option<u8> = namespace.is_none().then_some(REFUSED);
    for (path, name) in args.structures.iter().zip(&names) {
        match check_structure(path, name, namespace.as_ref(), args, &world, pack) {
            Ok(plan) => planned.push(plan),
            Err(code) => refused = Some(refused.map_or(code, |worst| worst.max(code))),
        }
    }
    if let Some(code) = refused {
        return ExitCode::from(code);
    }
    if args.dry_run {
        for plan in &planned {
            outln!(
                "would install {} at {}{}",
                plan.id,
                plan.target.display(),
                replacing(plan),
            );
        }
        return ExitCode::SUCCESS;
    }
    write(&planned, world.path)
}

/// The name `path` installs under: `name` when given, else its file name, without `.nbt` when it
/// ends in that.
fn name_for(path: &Path, name: Option<&str>) -> String {
    if let Some(name) = name {
        return name.to_owned();
    }
    let file_name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    file_name
        .strip_suffix(".nbt")
        .unwrap_or(&file_name)
        .to_owned()
}

/// The first two files that would install under one name, with that name.
fn shared_name<'a>(
    paths: &'a [PathBuf],
    names: &'a [String],
) -> Option<(&'a Path, &'a Path, &'a str)> {
    names.iter().enumerate().find_map(|(later, name)| {
        let earlier = names[..later].iter().position(|other| other == name)?;
        Some((
            paths[earlier].as_path(),
            paths[later].as_path(),
            name.as_str(),
        ))
    })
}

/// Every check one structure file must pass, each refusal reported; the exit code it earns.
fn check_structure(
    path: &Path,
    name: &str,
    namespace: Option<&Namespace>,
    args: &InstallArgs,
    world: &World<'_>,
    pack: &RegistryPack,
) -> Result<Planned, u8> {
    let id = checked_id(path, name, namespace, args);
    let bytes = fs::read(path).map_err(|err| {
        eprintln!(
            "error[E_STRUCTURE_UNREADABLE]: `{}` cannot be read: {err}",
            path.display(),
        );
        eprintln!("  fix: name a structure file `cairn compile --edition java` wrote");
        if err.kind() == io::ErrorKind::NotFound {
            NOT_LOCATED
        } else {
            REFUSED
        }
    })?;
    let stamp = read_structure_data_version(&bytes).map_err(|err| {
        report_structure_file(path, &err, world, pack);
        REFUSED
    })?;
    let holds = holds_to_world(path, stamp, args.allow_cross_version, world, pack);
    let Some(id) = id else {
        return Err(REFUSED);
    };
    let target = id.file_in(world.path, &world.version);
    let replacing = check_target(path, &id, &target, args);
    match replacing {
        Some(replacing) if holds => Ok(Planned {
            id,
            bytes,
            target,
            replacing,
        }),
        _ => Err(REFUSED),
    }
}

/// The id a file installs under, `None` once a refusal of its name, or of the namespace, is
/// reported.
fn checked_id(
    path: &Path,
    name: &str,
    namespace: Option<&Namespace>,
    args: &InstallArgs,
) -> Option<StructureId> {
    let checked = match namespace {
        Some(namespace) => StructureId::new(namespace, name).map(Some),
        None => IdPart::Name.check(name).map(|()| None),
    };
    match checked {
        Err(err) => {
            report_invalid_name(path, name, args, &err);
            None
        }
        Ok(_) if Path::new(name).extension().is_some_and(|ext| ext == "nbt") => {
            report_nbt_name(path, name, args);
            None
        }
        Ok(id) => id,
    }
}

/// Whether installing `id` at `target` replaces a file there, `None` once the reason the world
/// cannot take it there is reported.
fn check_target(path: &Path, id: &StructureId, target: &Path, args: &InstallArgs) -> Option<bool> {
    let shown = path.display();
    let replacing = match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => {
            eprintln!(
                "error[E_STRUCTURE_EXISTS]: `{shown}` would install as `{id}`, at `{}`, which is \
                 a folder",
                target.display(),
            );
            eprintln!("  fix: move that folder aside, or install under another id");
            return None;
        }
        Ok(_) if !args.replace => {
            eprintln!(
                "error[E_STRUCTURE_EXISTS]: `{shown}` would install as `{id}`, and the world \
                 already has a structure file at `{}`",
                target.display(),
            );
            if args.structures.len() == 1 {
                eprintln!(
                    "  fix: pass `--replace` to overwrite it, or `--name` to install under \
                     another name"
                );
            } else {
                eprintln!(
                    "  fix: pass `--replace` to overwrite it, or `--namespace` to install under \
                     another namespace"
                );
            }
            return None;
        }
        Ok(_) => true,
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            if let Some(blocked) = file_on_the_way(target) {
                eprintln!(
                    "error[E_WORLD_UNWRITABLE]: `{shown}` would install as `{id}`, at `{}`, and \
                     `{}` is a file, not the folder that path goes through",
                    target.display(),
                    blocked.display(),
                );
                eprintln!("  fix: move that file aside, or install under another id");
                return None;
            }
            false
        }
        Err(err) => {
            report_unlookable(path, id, target, &err);
            return None;
        }
    };
    for scratch in &reserved_paths(target)[1..] {
        match fs::symlink_metadata(scratch) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                eprintln!(
                    "error[E_STRUCTURE_EXISTS]: `{shown}` would install as `{id}`, and the world \
                     has a file at `{}`, which the install writes through",
                    scratch.display(),
                );
                eprintln!(
                    "  note: the file is written beside its path with `.tmp` added, and one it \
                     replaces is moved aside with `.bak` added"
                );
                eprintln!("  fix: move that file aside, or install under another id");
                return None;
            }
            Err(err) => {
                report_unlookable(path, id, scratch, &err);
                return None;
            }
        }
    }
    Some(replacing)
}

/// The nearest existing ancestor of `target` when it is no folder, so that no folder can be made
/// under it.
fn file_on_the_way(target: &Path) -> Option<PathBuf> {
    for ancestor in target.ancestors().skip(1) {
        match fs::metadata(ancestor) {
            Ok(meta) if meta.is_dir() => return None,
            Ok(_) => return Some(ancestor.to_path_buf()),
            Err(_) => {}
        }
    }
    None
}

/// The refusal of a path in the world that cannot be looked at.
fn report_unlookable(path: &Path, id: &StructureId, at: &Path, err: &io::Error) {
    eprintln!(
        "error[E_WORLD_UNWRITABLE]: `{}` would install as `{id}`, and `{}` cannot be looked at: \
         {err}",
        path.display(),
        at.display(),
    );
    eprintln!("  fix: make the world's folder readable and writable, then install again");
}

/// Whether a file stamped `stamp` may go into `world`, reporting why not or the warning its
/// install earns.
fn holds_to_world(
    path: &Path,
    stamp: i32,
    allowed: bool,
    world: &World<'_>,
    pack: &RegistryPack,
) -> bool {
    let world_dv = world.version.data_version;
    let built = format!(
        "`{}` is built for {}",
        path.display(),
        structure_version(pack, stamp)
    );
    let worlds = world_version(&world.version);
    let newest = recompile_target(pack, world_dv);
    match world.version.cross_version(stamp) {
        CrossVersion::Same => true,
        CrossVersion::Downgrade => {
            eprintln!("error[E_CROSS_VERSION_DOWNGRADE]: {built}, newer than the world's {worlds}");
            eprintln!(
                "  note: a release cannot read a structure newer than itself, and \
                 `--allow-cross-version` installs only an older one"
            );
            match newest {
                Some(row) if row.data_version == world_dv => eprintln!(
                    "  fix: recompile its source for the world, with `--edition java --target {}`",
                    row.mc_version,
                ),
                Some(row) => eprintln!(
                    "  fix: recompile its source with `--edition java --target {}`, the newest \
                     target no newer than the world, and install that with \
                     `--allow-cross-version`",
                    row.mc_version,
                ),
                None => eprintln!(
                    "  fix: no target Cairn builds for is as old as the world; open the world in \
                     {} or newer, which upgrades it, and install again",
                    pack.targetable()
                        .first()
                        .map_or("a newer release", |row| row.mc_version.as_str()),
                ),
            }
            false
        }
        CrossVersion::Upgrade => {
            let closer = newest.filter(|row| row.data_version > stamp);
            let (code, end) = if allowed {
                (
                    "warning[W_CROSS_VERSION_UPGRADE]",
                    ", as `--allow-cross-version` allows",
                )
            } else {
                ("error[E_CROSS_VERSION_UPGRADE]", "")
            };
            eprintln!("{code}: {built}, older than the world's {worlds}{end}");
            eprintln!(
                "  note: it keeps DataVersion {stamp}, so the game upgrades it through \
                 DataFixerUpper as it loads, which can lose items, signs, paintings and block \
                 entities"
            );
            match (closer, allowed) {
                (Some(row), _) if row.data_version == world_dv => eprintln!(
                    "  fix: recompile its source for the world, with `--edition java --target \
                     {}`{}",
                    row.mc_version,
                    if allowed {
                        ""
                    } else {
                        ", or install this file as it is with `--allow-cross-version`"
                    },
                ),
                (Some(row), _) => eprintln!(
                    "  fix: recompile its source with `--edition java --target {}`, the newest \
                     target no newer than the world, to leave the game less to upgrade{}",
                    row.mc_version,
                    if allowed {
                        ""
                    } else {
                        ", and install it with `--allow-cross-version`"
                    },
                ),
                (None, false) => eprintln!(
                    "  fix: pass `--allow-cross-version`; no target Cairn builds for is closer to \
                     the world"
                ),
                (None, true) => {}
            }
            allowed
        }
    }
}

/// `DataVersion N`, with the release the built-in table names it, as in `DataVersion 4189
/// (1.21.4)`.
fn structure_version(pack: &RegistryPack, stamp: i32) -> String {
    match pack
        .data_versions()
        .versions
        .iter()
        .find(|row| row.data_version == stamp)
    {
        Some(row) => format!("DataVersion {stamp} ({})", row.mc_version),
        None => format!("DataVersion {stamp}"),
    }
}

/// The world's `DataVersion`, with the release its `level.dat` names, as in `3953 (1.21)`.
fn world_version(version: &WorldVersion) -> String {
    match &version.name {
        Some(name) => format!("{} ({name})", version.data_version),
        None => version.data_version.to_string(),
    }
}

/// ", replacing the file there" for a plan that overwrites one.
fn replacing(plan: &Planned) -> &'static str {
    if plan.replacing {
        ", replacing the file there"
    } else {
        ""
    }
}

fn write(planned: &[Planned], world: &Path) -> ExitCode {
    let mut staged = StagedSet::default();
    let mut made = Vec::new();
    for plan in planned {
        let failure = match plan
            .target
            .parent()
            .map(|folder| make_folders(folder, &mut made))
        {
            Some(Err((folder, err))) => Some(format!("creating `{}`: {err}", folder.display())),
            _ => staged
                .stage(&plan.target, Kind::Structure, |file| {
                    file.write_all(&plan.bytes)
                })
                .err()
                .map(|err| format!("writing `{}`: {err}", plan.target.display())),
        };
        if let Some(failure) = failure {
            staged.discard();
            remove_folders(&made);
            eprintln!("error[E_WORLD_UNWRITABLE]: {failure}");
            report_unwritable_fix(world);
            return ExitCode::from(REFUSED);
        }
    }
    match staged.commit() {
        Ok(_) => {
            for plan in planned {
                outln!(
                    "installed {} at {}{}",
                    plan.id,
                    plan.target.display(),
                    replacing(plan),
                );
            }
            ExitCode::SUCCESS
        }
        Err(failure) => {
            remove_folders(&made);
            eprintln!("error[E_WORLD_UNWRITABLE]: {failure}");
            report_unwritable_fix(world);
            ExitCode::from(REFUSED)
        }
    }
}

/// Makes `folder` and the folders above it that are missing, adding each made to `made`, the
/// outermost first.
///
/// # Errors
///
/// The folder that could not be made, with why.
fn make_folders(folder: &Path, made: &mut Vec<PathBuf>) -> Result<(), (PathBuf, io::Error)> {
    let missing: Vec<&Path> = folder
        .ancestors()
        .take_while(|ancestor| fs::symlink_metadata(ancestor).is_err())
        .collect();
    for folder in missing.into_iter().rev() {
        fs::create_dir(folder).map_err(|err| (folder.to_path_buf(), err))?;
        made.push(folder.to_path_buf());
    }
    Ok(())
}

/// Removes the folders a failed run made, innermost first, leaving any that is not empty.
fn remove_folders(made: &[PathBuf]) {
    for folder in made.iter().rev() {
        let _ = fs::remove_dir(folder);
    }
}

/// The fix of a write into the world that failed.
fn report_unwritable_fix(world: &Path) {
    eprintln!(
        "  fix: make `{}` writable, with room for the files, then install again",
        world.display()
    );
}

fn report_world(world: &Path, err: &LevelDatError) {
    let shown = world.display();
    match err {
        LevelDatError::Bedrock { last_opened_with } => {
            let last_opened = last_opened_with
                .as_ref()
                .map(|release| format!(", last opened with {release}"))
                .unwrap_or_default();
            eprintln!(
                "error[E_INSTALL_EDITION_UNSUPPORTED]: `{shown}` is a Bedrock world{last_opened}"
            );
            eprintln!(
                "  note: `cairn install` installs into Java worlds; a Bedrock world keeps the \
                 structures it saves in its database, and loads none from files in its folder"
            );
            eprintln!(
                "  fix: compile with `--edition bedrock` and put the `.mcstructure` in a behavior \
                 pack's `structures` folder, which `/structure load` reads"
            );
        }
        LevelDatError::Missing => {
            eprintln!("error[E_WORLD_UNREADABLE]: `{shown}` {err}, so it is no world's folder");
            report_worlds_near(world);
        }
        LevelDatError::NotGzip | LevelDatError::Nbt(_) => {
            eprintln!("error[E_WORLD_UNREADABLE]: `{shown}` {err}");
            eprintln!(
                "  note: a Java world's `level.dat` is gzip NBT, and the save before the last is \
                 kept beside it as `level.dat_old`"
            );
            eprintln!("  fix: restore `level.dat` from `level.dat_old`, then install again");
        }
        LevelDatError::NoDataVersion => {
            eprintln!("error[E_WORLD_UNREADABLE]: `{shown}` {err}");
            eprintln!("  note: a world last saved before 1.9 records none");
            eprintln!("  fix: open the world in a current release and save it, then install again");
        }
        LevelDatError::Io(_) => {
            eprintln!("error[E_WORLD_UNREADABLE]: `{shown}` {err}");
            eprintln!(
                "  fix: make `{}` readable, then install again",
                world.join(LEVEL_DAT).display(),
            );
        }
        _ => {
            eprintln!("error[E_WORLD_UNREADABLE]: `{shown}` {err}");
            eprintln!("  fix: name the folder of a world a Java release saved");
        }
    }
}

/// The note and fix for a `--world` with no `level.dat`: the worlds inside it, or one beside it
/// spelled nearly as it is.
fn report_worlds_near(world: &Path) {
    let inside = worlds_in(world);
    if let Some(first) = inside.first() {
        eprintln!("  note: it holds the worlds {}", candidate_list(&inside));
        eprintln!(
            "  fix: name the world's own folder, as `--world {}`",
            quoted(&world.join(first)),
        );
        return;
    }
    let parent = world.parent().unwrap_or(Path::new(""));
    let beside = worlds_in(if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    });
    let named = world
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    match named
        .as_deref()
        .and_then(|named| nearest_match(named, beside.iter().map(String::as_str)))
    {
        Some(near) => eprintln!(
            "  note: {}",
            did_you_mean(&format!("--world {}", quoted(&parent.join(near))))
        ),
        None => eprintln!(
            "  note: a world's folder, inside the game's `saves` folder, holds its `level.dat`"
        ),
    }
    eprintln!("  fix: name the folder that holds the world's `level.dat`");
}

/// `path` as a shell takes it in one argument: in double quotes when it holds whitespace.
fn quoted(path: &Path) -> String {
    let shown = path.display().to_string();
    if shown.contains(char::is_whitespace) {
        format!("\"{shown}\"")
    } else {
        shown
    }
}

fn report_structure_file(
    path: &Path,
    err: &StructureFileError,
    world: &World<'_>,
    pack: &RegistryPack,
) {
    let shown = path.display();
    if matches!(err, StructureFileError::Bedrock) {
        eprintln!(
            "error[E_INSTALL_EDITION_UNSUPPORTED]: `{shown}` {err}, and `cairn install` installs \
             Java structure files"
        );
        match recompile_target(pack, world.version.data_version) {
            Some(row) => eprintln!(
                "  fix: compile its source with `--edition java --target {}` and install the \
                 `.nbt` it writes",
                row.mc_version,
            ),
            None => eprintln!(
                "  fix: compile its source with `--edition java` and install the `.nbt` it writes"
            ),
        }
        return;
    }
    eprintln!("error[E_STRUCTURE_UNREADABLE]: `{shown}` {err}");
    eprintln!(
        "  note: a Java structure file is gzip NBT whose root holds `size`, `blocks`, a `palette` \
         or `palettes` list and an integer `DataVersion`, as `cairn compile --edition java` \
         writes it"
    );
    eprintln!("  fix: name a structure file `cairn compile --edition java` wrote");
}

fn report_invalid_namespace(namespace: &str, err: &StructureIdError) {
    eprintln!(
        "error[E_INVALID_STRUCTURE_ID]: `--namespace {namespace}` names no namespace the game \
         loads: {err}"
    );
    eprintln!("  note: {}", rule_broken(err));
    match suggested(IdPart::Namespace, namespace) {
        Some(namespace) => eprintln!("  fix: --namespace {namespace}"),
        None => eprintln!("  fix: --namespace {DEFAULT_NAMESPACE}"),
    }
}

fn report_invalid_name(path: &Path, name: &str, args: &InstallArgs, err: &StructureIdError) {
    eprintln!(
        "error[E_INVALID_STRUCTURE_ID]: `{}` would install as `{}:{name}`, which the game does \
         not load: {err}",
        path.display(),
        args.namespace,
    );
    eprintln!("  note: {}", rule_broken(err));
    report_name_fix(suggested(IdPart::Name, name), args);
}

fn report_nbt_name(path: &Path, name: &str, args: &InstallArgs) {
    eprintln!(
        "error[E_INVALID_STRUCTURE_ID]: `{}` would install as `{}:{name}`, whose name ends in \
         `.nbt`",
        path.display(),
        args.namespace,
    );
    eprintln!(
        "  note: the game adds `.nbt` to a name itself, so the file would be `{name}.nbt` and \
         `/place template` would name it with the `.nbt` kept"
    );
    report_name_fix(suggested(IdPart::Name, name), args);
}

/// The fix of a name refused: `fixed`, the name the file could install under instead.
fn report_name_fix(fixed: Option<String>, args: &InstallArgs) {
    let Some(fixed) = fixed else {
        eprintln!("  fix: install it under another name with `--name`");
        return;
    };
    if args.structures.len() == 1 {
        eprintln!("  fix: --name {fixed}");
    } else {
        eprintln!(
            "  fix: install it in a run of its own with `--name {fixed}`, or rename the file"
        );
    }
}

/// The rule of a structure id `err` breaks, as a note states it.
fn rule_broken(err: &StructureIdError) -> &'static str {
    match err {
        StructureIdError::Empty { .. } => "a structure id is `namespace:name`, neither part empty",
        StructureIdError::Character {
            part: IdPart::Namespace,
            ..
        } => "a namespace holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`",
        StructureIdError::Character { .. } => {
            "a name holds only `a`-`z`, `0`-`9`, `_`, `-` and `.`, and `/` between its folders"
        }
        StructureIdError::EmptySegment { .. } => {
            "each folder and the file name in a name holds at least one character"
        }
        StructureIdError::TrailingDot { .. } => {
            "the game refuses a folder name that ends in `.`, as `.` and `..` do"
        }
        StructureIdError::DeviceName { .. } => {
            "the game refuses a folder or file name Windows reserves for a device, such as `con` \
             or `com1`, with or without an extension"
        }
        _ => "the game loads an id only when its namespace and name keep to its path rules",
    }
}

/// `value` made into a namespace or name the game loads, `None` when nothing of it is left.
fn suggested(part: IdPart, value: &str) -> Option<String> {
    let mapped: String = value
        .chars()
        .map(|ch| {
            let ch = ch.to_ascii_lowercase();
            if part.allows(ch) { ch } else { '_' }
        })
        .collect();
    let mapped = mapped.strip_suffix(".nbt").unwrap_or(&mapped);
    let mut segments: Vec<String> = mapped
        .split('/')
        .map(|segment| segment.trim_end_matches('.'))
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    for _ in 0..=segments.len() {
        let joined = segments.join("/");
        match part.check(&joined) {
            Ok(()) => return Some(joined),
            Err(StructureIdError::DeviceName { segment, .. }) => {
                let at = segments.iter().position(|s| *s == segment)?;
                let stem_end = segment.find('.').unwrap_or(segment.len());
                segments[at].insert(stem_end, '_');
            }
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_write_removes_the_folders_it_made() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("world");
        fs::create_dir(&world).expect("mkdir");
        fs::write(dir.path().join("file"), b"").expect("write");
        let namespace = Namespace::new(DEFAULT_NAMESPACE).expect("valid");
        let plan = |name: &str, target: PathBuf| Planned {
            id: StructureId::new(&namespace, name).expect("valid"),
            bytes: b"structure".to_vec(),
            target,
            replacing: false,
        };
        let made = world.join("generated").join("minecraft").join("structures");
        let planned = [
            plan("a", made.join("a.nbt")),
            plan("b", dir.path().join("file").join("b.nbt")),
        ];
        assert_eq!(write(&planned, &world), ExitCode::from(REFUSED));
        assert_eq!(
            fs::read_dir(&world).expect("read").count(),
            0,
            "{:?}",
            fs::read_dir(&world).expect("read").collect::<Vec<_>>()
        );
    }
}
