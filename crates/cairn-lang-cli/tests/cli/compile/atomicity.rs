//! A failed `cairn compile` leaves the previous build exactly as it was, and a refused one writes
//! nothing. Each failure is forced by a directory planted where a file has to go.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::check::parity::all_fixtures;
use crate::common::{
    Fixture, assert_stream, cargo_bin, compile, compile_as, examples_dir, stderr_of,
};

/// Every regular file under `dir`, keyed by name.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            out.insert(name, fs::read(&path).expect("read artifact"));
        }
    }
    out
}

struct Build {
    _tmp: TempDir,
    source: PathBuf,
    out_dir: PathBuf,
    /// Beside the source, so a snapshot of `out_dir` alone misses scratch left next to it.
    lock: PathBuf,
}

impl Build {
    fn lock_dir(&self) -> &Path {
        self.lock.parent().expect("the lockfile has a parent")
    }

    /// Files in either directory a finished build should not leave behind.
    fn scratch(&self) -> Vec<String> {
        let mut out: Vec<String> = snapshot(&self.out_dir)
            .into_keys()
            .filter(|name| !is_artifact(name))
            .collect();
        out.extend(
            snapshot(self.lock_dir())
                .into_keys()
                .filter(|name| is_scratch(name)),
        );
        out.sort();
        out
    }

    /// The lockfile's recorded hashes, which change with the source.
    fn lock_hashes(&self) -> (String, String) {
        let body = fs::read_to_string(&self.lock).expect("read lockfile");
        let field = |key: &str| {
            body.lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("lockfile has no `{key}`: {body}"))
                .trim()
                .to_owned()
        };
        (field("source_hash:"), field("resolved_ir_hash:"))
    }

    /// Compile the source again over this build, returning the exit code and stderr.
    fn rebuild(&self) -> (Option<i32>, String) {
        self.rebuild_with_lock(&self.lock)
    }

    /// [`Build::rebuild`] with the lockfile at `lock` instead.
    fn rebuild_with_lock(&self, lock: &Path) -> (Option<i32>, String) {
        let run = compile(&self.source, "java", "latest", &self.out_dir, Some(lock));
        (run.status.code(), stderr_of(&run))
    }
}

/// A directory holding one compiled `village.crn`: five artifacts plus a lockfile.
fn first_successful_build() -> Build {
    let tmp = TempDir::new().expect("tempdir");
    let source = tmp.path().join("village.crn");
    fs::copy(examples_dir().join("village.crn"), &source).expect("copy example");
    let out_dir = tmp.path().join("out");
    let lock = tmp.path().join("village.crn.lock");

    let run = compile(&source, "java", "latest", &out_dir, Some(&lock));
    assert_eq!(
        run.status.code(),
        Some(0),
        "the first build must succeed: {}",
        stderr_of(&run),
    );
    assert_eq!(
        snapshot(&out_dir).len(),
        5,
        "village.crn is the fixture because it produces several artifacts, \
         which is what makes a partial commit observable",
    );
    Build {
        _tmp: tmp,
        source,
        out_dir,
        lock,
    }
}

/// Edit the source so the second run writes different bytes, making "unchanged" a real check.
fn change_the_source(source: &Path) {
    let body = fs::read_to_string(source).expect("read source");
    let edited = body.replace("height=4", "height=5");
    assert_ne!(edited, body, "the edit must actually change the build");
    fs::write(source, edited).expect("write source");
}

/// Where a directory gets planted, and therefore which step fails.
const OBSTRUCTIONS: &[&str] = &[
    // Commit fails on the lockfile, after every artifact has been staged.
    "lockfile",
    // Commit fails on one artifact, with others already committed.
    "artifact",
    // Staging fails: the scratch file cannot be created.
    "artifact-staging",
    // The first entry, so the undo runs with nothing displaced and nothing committed.
    "first-artifact",
    // A leftover backup blocks the displace step.
    "artifact-backup",
];

/// The suffixes staging appends while a build is in flight.
fn is_scratch(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext == "tmp" || ext == "bak")
}

/// Java artifacts are the only files a finished build leaves in `--out`.
fn is_artifact(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|ext| ext == "nbt")
}

/// The artifact the commit reaches first, in `prepare_artifacts` order.
fn first_artifact_name(build: &Build) -> String {
    snapshot(&build.out_dir)
        .into_keys()
        .find(|name| is_artifact(name))
        .expect("the first build produced artifacts")
}

fn obstruction_path(build: &Build, label: &str) -> PathBuf {
    match label {
        "lockfile" => build.lock.clone(),
        "artifact" => build.out_dir.join("home2.nbt"),
        "artifact-staging" => build.out_dir.join("home3.nbt.tmp"),
        "first-artifact" => build.out_dir.join(first_artifact_name(build)),
        "artifact-backup" => build.out_dir.join("home2.nbt.bak"),
        other => panic!("unknown obstruction `{other}`"),
    }
}

/// Plant a directory where a file has to go, after changing the source.
fn arrange_failure(build: &Build, label: &str) -> PathBuf {
    change_the_source(&build.source);
    let obstruction = obstruction_path(build, label);
    fs::remove_file(&obstruction).ok();
    fs::create_dir_all(&obstruction).expect("plant the obstruction");
    obstruction
}

#[test]
fn a_failed_build_leaves_the_previous_one_untouched() {
    for label in OBSTRUCTIONS {
        let build = first_successful_build();
        let before_artifacts = snapshot(&build.out_dir);
        let before_lock = fs::read(&build.lock).expect("read lockfile");
        let obstruction = arrange_failure(&build, label);

        let (code, _) = build.rebuild();
        assert_eq!(code, Some(1), "{label}: the build must fail");

        // The planted directory is not an artifact to compare.
        let planted = obstruction
            .file_name()
            .expect("obstruction name")
            .to_string_lossy()
            .into_owned();
        let after = snapshot(&build.out_dir);
        let expected: Vec<&String> = before_artifacts
            .keys()
            .filter(|name| **name != planted)
            .collect();
        let actual: Vec<&String> = after.keys().filter(|name| **name != planted).collect();
        assert_eq!(actual, expected, "{label}: the set of artifacts changed");
        for name in expected {
            assert_eq!(
                after.get(name),
                before_artifacts.get(name),
                "{label}: `{name}` was replaced by the build that failed",
            );
        }
        if *label != "lockfile" {
            assert_eq!(
                fs::read(&build.lock).ok().as_ref(),
                Some(&before_lock),
                "{label}: the lockfile still describes the build on disk",
            );
        }
    }
}

#[test]
fn a_failed_build_leaves_no_scratch_files_behind() {
    for label in OBSTRUCTIONS {
        let build = first_successful_build();
        arrange_failure(&build, label);
        let (code, stderr) = build.rebuild();
        assert_eq!(
            code,
            Some(1),
            "{label}: the build must fail
stderr={stderr}"
        );

        let leftovers = build.scratch();
        assert!(
            leftovers.is_empty(),
            "{label}: scratch survived the failure: {leftovers:?}
stderr={stderr}",
        );
    }
}

#[test]
fn a_failed_build_says_which_file_stopped_it() {
    let build = first_successful_build();
    arrange_failure(&build, "artifact-backup");
    let (code, stderr) = build.rebuild();
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("home2.nbt.bak"),
        "the refusal must name the path that actually blocked it; got {stderr}",
    );

    let build = first_successful_build();
    arrange_failure(&build, "lockfile");
    let (code, stderr) = build.rebuild();
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("lockfile"),
        "a lockfile failure must say so; got {stderr}",
    );
}

#[test]
fn a_failed_first_build_leaves_nothing_at_all() {
    let tmp = TempDir::new().expect("tempdir");
    let source = tmp.path().join("village.crn");
    fs::copy(examples_dir().join("village.crn"), &source).expect("copy example");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out dir");
    let lock = tmp.path().join("village.crn.lock");
    fs::create_dir_all(&lock).expect("plant the obstruction");

    let run = compile(&source, "java", "latest", &out_dir, Some(&lock));
    assert_eq!(run.status.code(), Some(1), "{}", stderr_of(&run));
    assert!(
        snapshot(&out_dir).is_empty(),
        "a build that never completed must not leave artifacts: {:?}",
        snapshot(&out_dir).keys().collect::<Vec<_>>(),
    );
}

#[test]
fn a_successful_rebuild_replaces_everything_and_cleans_up() {
    let build = first_successful_build();
    let before = snapshot(&build.out_dir);
    let before_hashes = build.lock_hashes();
    change_the_source(&build.source);

    let (code, stderr) = build.rebuild();
    assert_eq!(code, Some(0), "stderr={stderr}");

    let after = snapshot(&build.out_dir);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>(),
        "the artifact set is the same, only the contents move",
    );
    assert!(
        after
            .iter()
            .any(|(name, bytes)| before.get(name) != Some(bytes)),
        "the edited source must produce different artifact bytes, or this test proves nothing",
    );
    // Not `lock.is_file()`, which the *first* build's lockfile satisfies.
    assert_ne!(
        build.lock_hashes(),
        before_hashes,
        "the lockfile has to describe the build that is now on disk",
    );
    let scratch = build.scratch();
    assert!(scratch.is_empty(), "scratch survived success: {scratch:?}");
}

#[test]
fn a_lock_path_that_collides_with_an_artifact_is_refused() {
    for suffix in ["", ".tmp", ".bak"] {
        let build = first_successful_build();
        let before = snapshot(&build.out_dir);
        let victim = build.out_dir.join(format!("home1.nbt{suffix}"));
        change_the_source(&build.source);

        let (code, stderr) = build.rebuild_with_lock(&victim);
        assert_eq!(
            code,
            Some(1),
            "`--lock` onto `home1.nbt{suffix}` must be refused; stderr={stderr}",
        );
        assert!(
            stderr.contains("collides"),
            "the refusal must say what it collides with; got {stderr}",
        );
        assert_eq!(
            snapshot(&build.out_dir),
            before,
            "`--lock` onto `home1.nbt{suffix}` changed the previous build",
        );
    }
}

/// `compile` run from `cwd`, with `--out` and `--lock` passed exactly as spelled.
fn compile_in(cwd: &Path, out_dir: Option<&str>, lock: &str) -> (Option<i32>, String) {
    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(cwd)
        .args(["compile", "village.crn", "--edition", "java"]);
    if let Some(out_dir) = out_dir {
        cmd.args(["--out", out_dir]);
    }
    let out = cmd.args(["--lock", lock]).output().expect("run cairn");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Rebuild from `dir` and assert the collision guard refuses it, naming `artifact` as the build
/// spells it, with `watched` left byte-identical.
fn assert_refused_by_the_guard(
    dir: &Path,
    out_dir: Option<&str>,
    lock: &str,
    artifact: &Path,
    watched: &Path,
) {
    // Edited first: `watched` may be the source's own directory.
    change_the_source(&dir.join("village.crn"));
    let before = snapshot(watched);

    let (code, stderr) = compile_in(dir, out_dir, lock);
    let args = format!("`--out {out_dir:?} --lock {lock}`");
    assert_eq!(
        code,
        Some(1),
        "{args} names an artifact and must be refused; stderr={stderr}",
    );
    // The guard's own wording: an unguarded run also exits 1, from the failed commit.
    let expected = format!(
        "collides with an artifact this build writes (`{}`)",
        artifact.display(),
    );
    assert!(
        stderr.contains(&expected),
        "{args} must be refused by the collision guard, naming the artifact, before \
         anything is staged; expected {expected:?}, got {stderr}",
    );
    assert_eq!(
        snapshot(watched),
        before,
        "{args} changed the previous build"
    );
}

/// [`assert_refused_by_the_guard`] over a fresh build, with `--out` and `--lock` from `spell`.
fn assert_lock_spelling_is_refused(spell: impl FnOnce(&Path) -> (String, String)) {
    let build = first_successful_build();
    let dir = build.source.parent().expect("source has a parent");
    let (out_dir, lock) = spell(dir);
    let artifact = Path::new(&out_dir).join("home1.nbt");
    assert_refused_by_the_guard(dir, Some(&out_dir), &lock, &artifact, &build.out_dir);
}

#[test]
fn a_lock_spelled_differently_from_its_artifact_is_still_refused() {
    // Each spelling names the file `out/home1.nbt` does.
    for lock in ["./out/home1.nbt", "out/./home1.nbt", "out/../out/home1.nbt"] {
        assert_lock_spelling_is_refused(|_| ("out".into(), lock.into()));
    }
    // A scratch name is reserved however it is spelled, too.
    assert_lock_spelling_is_refused(|_| ("out".into(), "./out/home1.nbt.bak".into()));
    // `$PWD/…`, the form scripts and CI write, against a relative `--out`.
    assert_lock_spelling_is_refused(|dir| {
        let lock = dir.join("out").join("home1.nbt");
        ("out".into(), lock.to_str().unwrap().into())
    });
    // And the other way round.
    assert_lock_spelling_is_refused(|dir| {
        let out_dir = dir.join("out");
        (out_dir.to_str().unwrap().into(), "out/home1.nbt".into())
    });
}

#[cfg(unix)]
#[test]
fn a_lock_reached_through_a_symlinked_out_is_refused() {
    // `--out` through a symlink and `--lock` through the real directory, and the reverse.
    assert_lock_spelling_is_refused(|dir| {
        std::os::unix::fs::symlink(dir.join("out"), dir.join("link")).expect("symlink");
        ("link".into(), "out/home1.nbt".into())
    });
    assert_lock_spelling_is_refused(|dir| {
        std::os::unix::fs::symlink(dir.join("out"), dir.join("link")).expect("symlink");
        ("out".into(), "link/home1.nbt".into())
    });
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_lock_differing_only_in_case_is_refused_where_case_is_folded() {
    // Case-insensitive by default on both platforms.
    assert_lock_spelling_is_refused(|_| ("out".into(), "OUT/HOME1.NBT".into()));
}

#[test]
fn a_bare_lock_name_collides_with_the_default_out() {
    // With `--out` omitted, a bare `--lock home1.nbt` has an empty parent: the current directory.
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    fs::copy(examples_dir().join("village.crn"), dir.join("village.crn")).expect("copy example");
    let (code, stderr) = compile_in(dir, None, "village.crn.lock");
    assert_eq!(
        code,
        Some(0),
        "the first build must succeed; stderr={stderr}"
    );
    assert!(
        dir.join("home1.nbt").is_file(),
        "the artifact lands beside the source"
    );

    assert_refused_by_the_guard(
        dir,
        None,
        "home1.nbt",
        &Path::new(".").join("home1.nbt"),
        dir,
    );
}

#[test]
fn a_lock_sharing_only_the_artifacts_name_still_builds() {
    // The control: the artifact's name in another directory is a different file.
    let build = first_successful_build();
    let dir = build.source.parent().expect("source has a parent");
    fs::create_dir(dir.join("other")).expect("mkdir other");
    change_the_source(&build.source);

    let (code, stderr) = compile_in(dir, Some("out"), "other/home1.nbt");
    assert_eq!(
        code,
        Some(0),
        "`--lock other/home1.nbt` is not an artifact and must build; stderr={stderr}",
    );
    assert!(dir.join("other").join("home1.nbt").is_file());
}

#[cfg(windows)]
#[test]
fn a_lock_reached_through_a_junction_is_refused() {
    // The Windows counterpart of the symlink case: a junction needs no elevation.
    let junction = |dir: &Path| {
        let status = Command::new("cmd")
            .current_dir(dir)
            .args(["/C", "mklink", "/J", "link", "out"])
            .status()
            .expect("run mklink");
        assert!(status.success(), "mklink /J failed");
    };
    assert_lock_spelling_is_refused(|dir| {
        junction(dir);
        ("link".into(), "out/home1.nbt".into())
    });
    assert_lock_spelling_is_refused(|dir| {
        junction(dir);
        ("out".into(), "link/home1.nbt".into())
    });
}

#[test]
fn two_artifacts_differing_only_in_case_are_refused_on_every_host() {
    // `home1.nbt` and `HOME1.nbt` are one entry on macOS and Windows, so every host refuses.
    let build = first_successful_build();
    let before = snapshot(&build.out_dir);
    let body = fs::read_to_string(&build.source).expect("read source");
    let anchor = "  place id=home3 use=cottage theme=medieval north_of=home1 gap=5\n";
    assert!(body.contains(anchor), "village.crn places home3");
    let edited = body.replacen(
        anchor,
        &format!("{anchor}  place id=HOME1 use=cottage theme=medieval east_of=home3 gap=4\n"),
        1,
    );
    fs::write(&build.source, edited).expect("write source");

    let (code, stderr) = build.rebuild();
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(24, 3, "error", "E_OUTPUT_NAME_COLLISION")]);
    assert!(stderr.contains("differ only in case"), "got {stderr}");
    assert_eq!(
        snapshot(&build.out_dir),
        before,
        "`home1` and `HOME1` changed the previous build",
    );
}

#[cfg(unix)]
#[test]
fn a_lock_whose_location_cannot_be_resolved_is_reported_not_passed() {
    // A parent that runs through a regular file leaves the guard no key; it must say so.
    let build = first_successful_build();
    let dir = build.source.parent().expect("source has a parent");
    change_the_source(&build.source);
    let before = snapshot(&build.out_dir);

    let (code, stderr) = compile_in(dir, Some("out"), "village.crn/sub/v.lock");
    assert!(
        stderr.contains("could not check that it is not one of this build's artifacts"),
        "an unresolvable lock location must be reported; got {stderr}",
    );
    assert_eq!(
        code,
        Some(1),
        "the lock cannot be written there; stderr={stderr}"
    );
    assert_eq!(
        snapshot(&build.out_dir),
        before,
        "the failed build changed the previous one"
    );
}

/// Sources a compile refuses before writing anything: label, source, edition, target, code.
const REFUSED: &[(&str, &str, &str, &str, &str)] = &[
    (
        "an id no edition has",
        "theme t:\n  slot floor -> @totally_not_a_block\n\
         struct s size=2x2\n  floor mat_slot=floor\n",
        "java",
        "1.21.4",
        "E_UNKNOWN_ID",
    ),
    (
        "a target below the declared floor",
        "@requires version>=1.21\ntheme t:\n  slot floor -> @oak_planks\n\
         struct s size=2x2\n  floor mat_slot=floor\n",
        "java",
        "1.20.4",
        "E_VERSION_CAP",
    ),
    (
        "a struct and a place writing one file name, on Bedrock",
        OUTPUT_NAME_COLLISION,
        "bedrock",
        "1.21.60",
        "E_OUTPUT_NAME_COLLISION",
    ),
    (
        "a place naming the one theme variant the edition lacks",
        "@cairn 2026.06\n\ntheme shop_bedrock:\n  slot floor -> @dark_oak_planks\n\n\
         def hut size=4x4:\n  floor mat_slot=floor\n\n\
         site s:\n  place id=home use=hut theme=shop_bedrock at=origin\n",
        "java",
        "latest",
        "E_THEME_VARIANT_MISSING",
    ),
    (
        "a sloped roof of a whole block, on Java",
        COBBLESTONE_ROOF,
        "java",
        "1.21.4",
        "E_INCOMPATIBLE_MATERIAL",
    ),
    (
        "a sloped roof of a whole block, on Bedrock",
        COBBLESTONE_ROOF,
        "bedrock",
        "1.21.60",
        "E_INCOMPATIBLE_MATERIAL",
    ),
    (
        "a struct with no size beside one that lowers",
        "@cairn 2026.06\n\ntheme t:\n  slot wall -> @cobblestone\n\n\
         struct good size=4x4\n  walls class=outer mat_slot=wall height=3\n\n\
         struct bad\n  walls class=outer mat_slot=wall height=3\n",
        "java",
        "latest",
        "E_PARTIAL_BUILD",
    ),
];

/// A `struct hut` and a `place id=hut` both write `hut.nbt`.
const OUTPUT_NAME_COLLISION: &str = "@cairn 2026.06\n\n\
theme t:\n  slot floor -> @oak_planks\n\n\
struct hut size=3x3\n  floor mat_slot=floor\n\n\
def house size=3x3:\n  floor mat_slot=floor\n\n\
site s:\n  place id=hut use=house theme=t at=origin\n";

/// A gable roof bound to cobblestone, which has no stair states to carry.
const COBBLESTONE_ROOF: &str = "@cairn 2026.06\n\n\
theme t:\n  slot floor -> @oak_planks\n  slot wall  -> @cobblestone\n  slot roof  -> @cobblestone\n\n\
struct hut size=5x3\n  floor mat_slot=floor\n  walls class=outer mat_slot=wall height=3\n  \
roof  kind=gable mat_slot=roof overhang=0\n";

/// `--out` is created after every refusal is decided, so a check moved later leaves it behind.
#[test]
fn a_refused_compile_writes_nothing() {
    let rows = REFUSED
        .iter()
        .map(|&(label, source, edition, target, code)| {
            (label.to_owned(), source.to_owned(), edition, target, code)
        })
        .chain(all_fixtures().map(|(code, source)| {
            (
                format!("the check fixture for {code}"),
                source.text(),
                "java",
                "latest",
                *code,
            )
        }));
    for (label, source, edition, target, code) in rows {
        let fixture = Fixture::new(&source);
        let out = compile_as(&fixture, edition, target);
        let stderr = stderr_of(&out);
        assert_eq!(out.status.code(), Some(1), "{label}: stderr={stderr}");
        assert!(
            stderr.contains(&format!("error[{code}]")),
            "{label}: refused for {code}; stderr={stderr}",
        );
        assert_eq!(
            fixture.artifacts(),
            Vec::<String>::new(),
            "{label}: a refused compile must leave no structure and no lockfile",
        );
        assert!(
            !fixture.out().exists(),
            "{label}: a refused compile must not create its output directory",
        );
    }
}
