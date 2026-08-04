//! A failed `cairn compile` must leave the previous build exactly as it was.
//!
//! `write_artifacts_and_lock`'s doc states the contract — "either every
//! artifact + the lock, or none" — but the rollback it relied on could only
//! delete. `write_tag_atomically` renames over whatever is already at the
//! path, so the list of files it had written was a mix of "this run created
//! it" and "this run replaced it", and rollback deleted both. A build that
//! failed on its last step took the previous build's output with it.
//!
//! Forcing the failure portably: a **directory** at a path a file has to be
//! created or renamed onto makes that step fail on every platform, with no
//! permission bits or filesystem-specific behaviour involved. Which path the
//! directory sits on decides which phase breaks.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

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
    lock: PathBuf,
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

fn compile(source: &Path, out_dir: &Path, lock: &Path) -> Option<i32> {
    Command::new(cargo_bin())
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
        .expect("run cairn")
        .status
        .code()
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
];

/// Java artifacts are the only files a finished build leaves in `--out`.
/// Anything else is bookkeeping: a `.tmp` that never landed, or a `.bak`
/// that was never cleared.
fn is_artifact(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|ext| ext == "nbt")
}

fn obstruction_path(build: &Build, label: &str) -> PathBuf {
    match label {
        "lockfile" => build.lock.clone(),
        "artifact" => build.out_dir.join("home2.nbt"),
        "artifact-staging" => build.out_dir.join("home3.nbt.tmp"),
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
    // A retry has to start from a clean directory: an orphaned `.tmp` or
    // backup accumulates, and worse, reads as an artifact to anything
    // scanning the output.
    for label in OBSTRUCTIONS {
        let build = first_successful_build();
        arrange_failure(&build, label);
        assert_eq!(compile(&build.source, &build.out_dir, &build.lock), Some(1));

        let leftovers: Vec<String> = snapshot(&build.out_dir)
            .into_keys()
            .filter(|name| !is_artifact(name))
            .collect();
        assert!(
            leftovers.is_empty(),
            "{label}: scratch files survived the failure: {leftovers:?}",
        );
    }
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
    // The other side of the contract: when nothing obstructs it, the commit
    // has to land in full and leave no bookkeeping behind.
    let build = first_successful_build();
    let before = snapshot(&build.out_dir);
    change_the_source(&build.source);

    assert_eq!(compile(&build.source, &build.out_dir, &build.lock), Some(0));

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
        "the edited source must produce different bytes, or this test proves nothing",
    );
    let scratch: Vec<String> = after
        .into_keys()
        .filter(|name| !is_artifact(name))
        .collect();
    assert!(
        scratch.is_empty(),
        "scratch files survived success: {scratch:?}"
    );
    assert!(build.lock.is_file(), "the lockfile is written on success");
}
