//! A failed `cairn compile` must leave the previous build exactly as it was.
//!
//! The mechanism that made that hard: a rename consumes whatever is already
//! at its destination, so a list of "files this run wrote" mixes files it
//! created with files it replaced. An undo that only deletes therefore takes
//! the previous build's artifacts with it — and the step most likely to fail
//! is the last one, after every artifact has already landed.
//!
//! Forcing the failure portably: a **directory** at a path a file has to be
//! created or renamed onto makes that step fail on every platform, with no
//! permission bits or filesystem-specific behaviour involved. Which path the
//! directory sits on decides which step breaks.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

mod common;
use common::{cargo_bin, examples_dir};

/// Every regular file under `dir`, keyed by name, so a snapshot can be
/// compared byte-for-byte before and after a failed run.
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
    /// The lockfile lives beside the *source*, not in `out_dir`. Scratch
    /// left next to it is therefore invisible to a snapshot of `out_dir`
    /// alone, which is how a leaked `village.crn.lock.tmp` went unnoticed.
    lock: PathBuf,
}

impl Build {
    fn lock_dir(&self) -> &Path {
        self.lock.parent().expect("the lockfile has a parent")
    }

    /// Files in either directory that a finished build has no business
    /// leaving behind.
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
}

/// A directory holding one successfully compiled `village.crn`: five
/// artifacts plus a lockfile, all of which a second run will replace.
fn first_successful_build() -> Build {
    let tmp = TempDir::new().expect("tempdir");
    let source = tmp.path().join("village.crn");
    fs::copy(examples_dir().join("village.crn"), &source).expect("copy example");
    let out_dir = tmp.path().join("out");
    let lock = tmp.path().join("village.crn.lock");

    let status = compile(&source, &out_dir, &lock);
    assert_eq!(status, Some(0), "the first build must succeed");
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

/// Returns the exit code and stderr — the latter so an assertion failure
/// carries the CLI's own account of what went wrong instead of only the
/// resulting directory listing.
fn compile_with_output(source: &Path, out_dir: &Path, lock: &Path) -> (Option<i32>, String) {
    let out = Command::new(cargo_bin())
        .args([
            "compile",
            source.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.to_str().unwrap(),
            "--lock",
            lock.to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn compile(source: &Path, out_dir: &Path, lock: &Path) -> Option<i32> {
    compile_with_output(source, out_dir, lock).0
}

/// Edit the source so the second run genuinely produces different bytes —
/// otherwise "unchanged" would hold whether the rollback worked or not.
fn change_the_source(source: &Path) {
    let body = fs::read_to_string(source).expect("read source");
    let edited = body.replace("height=4", "height=5");
    assert_ne!(edited, body, "the edit must actually change the build");
    fs::write(source, edited).expect("write source");
}

/// Where a directory gets planted, and therefore which step fails.
const OBSTRUCTIONS: &[&str] = &[
    // Commit fails on the lockfile, after every artifact has been staged.
    // This is the reported case.
    "lockfile",
    // Commit fails on one artifact, with others already committed.
    "artifact",
    // Staging fails: the scratch file cannot even be created, so nothing
    // should have been touched yet.
    "artifact-staging",
    // The first entry in the set, so the undo runs with nothing displaced
    // and nothing committed — the branch every other case skips past.
    "first-artifact",
    // A leftover backup blocks the *displace* step rather than the place
    // step, which is the one whose failure used to be reported against the
    // wrong path entirely.
    "artifact-backup",
];

/// The suffixes this code appends while a build is in flight. Either one
/// surviving the run means the set was not cleaned up.
fn is_scratch(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext == "tmp" || ext == "bak")
}

/// Java artifacts are the only files a finished build leaves in `--out`.
/// Anything else is bookkeeping: a `.tmp` that never landed, or a `.bak`
/// that was never cleared.
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

/// Plant a directory where a file has to go, after making the source
/// produce different bytes so "unchanged" is a real observation.
fn arrange_failure(build: &Build, label: &str) -> PathBuf {
    change_the_source(&build.source);
    let obstruction = obstruction_path(build, label);
    fs::remove_file(&obstruction).ok();
    fs::create_dir_all(&obstruction).expect("plant the obstruction");
    obstruction
}

#[test]
fn atomic_1_a_failed_build_leaves_the_previous_one_untouched() {
    for label in OBSTRUCTIONS {
        let build = first_successful_build();
        let before_artifacts = snapshot(&build.out_dir);
        let before_lock = fs::read(&build.lock).expect("read lockfile");
        let obstruction = arrange_failure(&build, label);

        let code = compile(&build.source, &build.out_dir, &build.lock);
        assert_eq!(code, Some(1), "{label}: the build must fail");

        // The obstruction itself is a directory we planted, not an artifact
        // to compare; everything around it is.
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
fn atomic_2_a_failed_build_leaves_no_scratch_files_behind() {
    // A retry has to start from a clean slate: an orphaned `.tmp` or `.bak`
    // accumulates, and a `.bak` in particular is a copy of the previous
    // build under a name that reads as scratch to anything scanning the
    // output.
    //
    // Both directories, because the lockfile does not live in `--out`: a
    // leaked `village.crn.lock.tmp` is invisible to a snapshot of the
    // artifact directory, which is where it hid.
    for label in OBSTRUCTIONS {
        let build = first_successful_build();
        arrange_failure(&build, label);
        let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &build.lock);
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
fn atomic_2b_a_failed_build_says_which_file_stopped_it() {
    // The obstruction is the thing to fix, so it is the thing to name. A
    // stale `home2.nbt.bak` blocking the displace step used to be reported
    // as a failure writing `home2.nbt`, sending the operator to a file that
    // was fine.
    let build = first_successful_build();
    arrange_failure(&build, "artifact-backup");
    let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &build.lock);
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("home2.nbt.bak"),
        "the refusal must name the path that actually blocked it; got {stderr}",
    );

    // And the lockfile keeps its own noun through both phases: a commit-time
    // failure on it used to be indistinguishable from an artifact's.
    let build = first_successful_build();
    arrange_failure(&build, "lockfile");
    let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &build.lock);
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("lockfile"),
        "a lockfile failure must say so; got {stderr}",
    );
}

#[test]
fn atomic_3_a_failed_first_build_leaves_nothing_at_all() {
    // Nothing existed beforehand, so "restore what was there" and "remove
    // what we made" have the same answer, and neither may be skipped.
    let tmp = TempDir::new().expect("tempdir");
    let source = tmp.path().join("village.crn");
    fs::copy(examples_dir().join("village.crn"), &source).expect("copy example");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out dir");
    let lock = tmp.path().join("village.crn.lock");
    fs::create_dir_all(&lock).expect("plant the obstruction");

    assert_eq!(compile(&source, &out_dir, &lock), Some(1));
    assert!(
        snapshot(&out_dir).is_empty(),
        "a build that never completed must not leave artifacts: {:?}",
        snapshot(&out_dir).keys().collect::<Vec<_>>(),
    );
}

#[test]
fn atomic_4_a_successful_rebuild_replaces_everything_and_cleans_up() {
    // The other side of the contract: with nothing obstructing it the commit
    // has to land in full and leave no bookkeeping behind.
    let build = first_successful_build();
    let before = snapshot(&build.out_dir);
    let before_hashes = build.lock_hashes();
    change_the_source(&build.source);

    let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &build.lock);
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
    // Not `lock.is_file()`: the lockfile from the *first* build satisfies
    // that whether or not the second one replaced it, so a commit that
    // rolled the lockfile back while keeping the new artifacts would pass.
    assert_ne!(
        build.lock_hashes(),
        before_hashes,
        "the lockfile has to describe the build that is now on disk",
    );
    let scratch = build.scratch();
    assert!(scratch.is_empty(), "scratch survived success: {scratch:?}");
}

#[test]
fn atomic_5_a_lock_path_that_collides_with_an_artifact_is_refused() {
    // Two set entries sharing a destination staged over each other and then
    // fought over one backup during the commit, which deleted the previous
    // artifact with no copy left anywhere — the same loss this suite exists
    // to prevent, reached through an argument the CLI accepted in silence.
    //
    // The scratch names collide just as destructively, one phase earlier.
    for suffix in ["", ".tmp", ".bak"] {
        let build = first_successful_build();
        let before = snapshot(&build.out_dir);
        let victim = build.out_dir.join(format!("home1.nbt{suffix}"));
        change_the_source(&build.source);

        let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &victim);
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

/// `compile` run from `cwd`, with `--out` (when given) and `--lock` passed
/// exactly as spelled — the spellings are the point, so nothing here joins
/// them onto a directory first.
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

/// Rebuild from `dir` with `out_dir` and `lock`, and assert the collision
/// guard refuses it naming `artifact` — the artifact's final path as the
/// build spells it, never a scratch name or the lock's own spelling — with
/// `watched` left byte-identical and nothing staged in it.
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
    // Keyed on the guard's own wording: the unguarded run also exits 1, from
    // the failed commit, after the artifact is already gone.
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

/// [`assert_refused_by_the_guard`] over a fresh [`first_successful_build`],
/// with `--out` and `--lock` as `spell` returns them for the build's
/// directory. The artifact is `home1.nbt` under that `--out`, as spelled.
fn assert_lock_spelling_is_refused(spell: impl FnOnce(&Path) -> (String, String)) {
    let build = first_successful_build();
    let dir = build.source.parent().expect("source has a parent");
    let (out_dir, lock) = spell(dir);
    let artifact = Path::new(&out_dir).join("home1.nbt");
    assert_refused_by_the_guard(dir, Some(&out_dir), &lock, &artifact, &build.out_dir);
}

#[test]
fn atomic_6_a_lock_spelled_differently_from_its_artifact_is_still_refused() {
    // The guard compared paths as spelled, so only the byte-identical
    // `out/home1.nbt` was refused. Every spelling below names that same
    // file, and each one reached the commit and destroyed the previous
    // artifact with no backup left.
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
fn atomic_7_a_lock_reached_through_a_symlinked_out_is_refused() {
    // `--out` through a symlink and `--lock` through the real directory
    // name one file, and so do the reverse; neither spelling shares a
    // single component with the other past the directory.
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
fn atomic_8_a_lock_differing_only_in_case_is_refused_where_case_is_folded() {
    // Case-insensitive by default on both platforms, so `OUT/HOME1.NBT` is
    // the artifact's own directory entry.
    assert_lock_spelling_is_refused(|_| ("out".into(), "OUT/HOME1.NBT".into()));
}

#[test]
fn atomic_9_a_bare_lock_name_collides_with_the_default_out() {
    // With `--out` omitted the artifacts go beside the source, and a bare
    // `--lock home1.nbt` has an empty parent. That empty parent is the
    // current directory, the same entry as `./home1.nbt`.
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
fn atomic_10_a_lock_sharing_only_the_artifacts_name_still_builds() {
    // The control for the tests above: the key is the parent *and* the name,
    // so the artifact's name in another directory is a different file.
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
fn atomic_7_windows_a_lock_reached_through_a_junction_is_refused() {
    // The Windows counterpart of the symlink case: a directory junction
    // needs no elevation, and names `out` under a spelling that shares
    // nothing with it but the file name.
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
fn atomic_11_two_artifacts_differing_only_in_case_are_refused_on_every_host() {
    // `home1` and `HOME1` are distinct ids, but `home1.nbt` and `HOME1.nbt`
    // are one directory entry on macOS and Windows. Staged together there
    // they overwrote each other and the commit lost both, the previous one
    // included; on Linux they built as two files. The names are compared
    // ignoring case before lowering, so every host refuses the source and
    // the previous build stays as it was.
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

    let (code, stderr) = compile_with_output(&build.source, &build.out_dir, &build.lock);
    assert_eq!(code, Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("error[E_OUTPUT_NAME_COLLISION]") && stderr.contains("differ only in case"),
        "the refusal must be the case-folded name collision; got {stderr}",
    );
    assert_eq!(
        snapshot(&build.out_dir),
        before,
        "`home1` and `HOME1` changed the previous build",
    );
}

#[cfg(unix)]
#[test]
fn atomic_12_a_lock_whose_location_cannot_be_resolved_is_reported_not_passed() {
    // Only a missing directory is an ordinary reason not to resolve a
    // path. Anything else — here a parent that runs through a regular file —
    // leaves the guard without a key to compare, and it says so rather than
    // reading "no key" as "no collision".
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
