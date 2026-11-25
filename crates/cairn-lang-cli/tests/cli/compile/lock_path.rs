//! A `--lock` the compile refuses before reading the source: one that names the source, and one
//! that ends in `.crn` (`spec/versioning-editions` "Provenance and lock").

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{HUT, Run, cargo_bin, folds_case, snapshot};

/// `cairn compile <source> --edition java --lock <lock>` from `dir`, both passed as spelled.
fn compile_from(dir: &Path, source: &str, lock: &str) -> Run {
    Run::from(
        Command::new(cargo_bin())
            .current_dir(dir)
            .args(["compile", source, "--edition", "java", "--lock", lock])
            .output()
            .expect("run cairn"),
    )
}

/// A directory holding [`HUT`] as `src/hut.crn`, the source each test compiles.
fn source_dir() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("make src");
    fs::write(dir.path().join("src").join("hut.crn"), HUT).expect("write source");
    dir
}

/// Compile `src/hut.crn` from `dir` with `--lock lock`, and assert it is refused with `stderr`,
/// writing nothing and leaving every file in `src` as it was.
fn assert_refused(dir: &Path, lock: &str, stderr: &str) {
    let before = snapshot(&dir.join("src"));

    let run = compile_from(dir, "src/hut.crn", lock);

    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(2), "", stderr),
        "--lock {lock}",
    );
    assert_eq!(snapshot(&dir.join("src")), before, "--lock {lock}");
    assert_eq!(
        fs::read_to_string(dir.join("src").join("hut.crn")).expect("read source"),
        HUT,
        "--lock {lock}",
    );
}

/// The refusal of a `--lock` spelled `lock` that names `src/hut.crn`, as the command spells it.
fn names_the_source(lock: &str) -> String {
    format!(
        "error: `--lock {lock}` names `src/hut.crn`, the source being compiled\n  note: give \
         `--lock` a `.lock` path, or leave it out to write `src/hut.crn.lock`\n"
    )
}

#[test]
fn a_lock_naming_the_source_is_refused_however_it_is_spelled() {
    let dir = source_dir();
    let absolute = dir.path().join("src").join("hut.crn");
    let absolute = absolute.to_str().expect("utf-8 path");
    for lock in [
        "src/hut.crn",
        "./src/hut.crn",
        "src/../src/hut.crn",
        absolute,
    ] {
        assert_refused(dir.path(), lock, &names_the_source(lock));
    }
    assert_eq!(snapshot(dir.path()), BTreeMap::new(), "nothing beside src");
}

#[test]
fn a_lock_hard_linked_to_the_source_is_refused() {
    let dir = source_dir();
    let src = dir.path().join("src");
    fs::hard_link(src.join("hut.crn"), src.join("hard.lock")).expect("hard-link the source");

    assert_refused(
        dir.path(),
        "src/hard.lock",
        &names_the_source("src/hard.lock"),
    );
}

#[cfg(unix)]
#[test]
fn a_lock_reaching_the_source_through_a_symbolic_link_is_refused() {
    let dir = source_dir();
    let src = dir.path().join("src");
    std::os::unix::fs::symlink(&src, dir.path().join("alias")).expect("symlink the directory");
    std::os::unix::fs::symlink("hut.crn", src.join("soft.lock")).expect("symlink the source");

    for lock in ["alias/hut.crn", "src/soft.lock"] {
        assert_refused(dir.path(), lock, &names_the_source(lock));
    }
}

#[cfg(windows)]
#[test]
fn a_lock_reaching_the_source_through_a_junction_is_refused() {
    let dir = source_dir();
    let status = Command::new("cmd")
        .current_dir(dir.path())
        .args(["/C", "mklink", "/J", "alias", "src"])
        .status()
        .expect("run mklink");
    assert!(status.success(), "mklink /J failed");

    let lock = r"alias\hut.crn";
    assert_refused(dir.path(), lock, &names_the_source(lock));
}

#[test]
fn a_lock_naming_the_source_in_another_case_is_refused_where_the_directory_folds_case() {
    let dir = source_dir();
    if !folds_case(&dir.path().join("src")) {
        return;
    }
    for lock in ["src/HUT.CRN", "SRC/Hut.crn"] {
        assert_refused(dir.path(), lock, &names_the_source(lock));
    }
}

#[test]
fn a_lock_ending_in_crn_is_refused_whatever_it_names() {
    let dir = source_dir();
    fs::write(
        dir.path().join("src").join("other.crn"),
        "struct other size=3x3\n",
    )
    .expect("write another source");
    for lock in ["src/other.crn", "src/missing.crn", "build/hut.CRN"] {
        let stderr = format!(
            "error: `--lock {lock}` names a `.crn` file, and a lockfile is never a Cairn source\n  \
             note: give `--lock` a `.lock` path, such as `{lock}.lock`, or leave it out to write \
             `src/hut.crn.lock`\n"
        );
        assert_refused(dir.path(), lock, &stderr);
    }
    assert_eq!(snapshot(dir.path()), BTreeMap::new(), "nothing beside src");
    assert!(
        !dir.path().join("build").exists(),
        "no directory made for the lock"
    );
}

#[test]
fn a_lock_is_refused_before_the_source_is_read() {
    let dir = source_dir();
    fs::write(dir.path().join("src").join("hut.crn"), "struct\n").expect("break the source");

    let run = compile_from(dir.path(), "src/hut.crn", "src/hut.crn");

    assert_eq!(
        (run.code, run.stderr),
        (Some(2), names_the_source("src/hut.crn")),
        "a usage error comes before the source's own findings",
    );
}
