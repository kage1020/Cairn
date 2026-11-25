//! A `--lock` the compile refuses before reading the source: one that names the source, and one
//! that ends in `.crn` (`spec/versioning-editions` "Provenance and lock").

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::common::{assert_compile_refused, compile_in, folds_case, snapshot, sources_in};

/// The note under a `--lock` that names `src/hut.crn` by its own name.
const BY_NAME_FIX: &str =
    "  note: give `--lock` a `.lock` path, or leave it out to write `src/hut.crn.lock`\n";

/// The note under a `--lock` a link makes `src/hut.crn`, when `src/hut.crn.lock` is free.
const LINKED_FIX: &str = "  note: a link makes them one file; give `--lock` a `.lock` path \
                          elsewhere, or leave it out to write `src/hut.crn.lock`\n";

/// Compile `src/hut.crn` from `dir` with `--lock lock`, and assert it is refused with exit 2 and
/// `stderr`, leaving every file in `src` as it was.
fn assert_refused(dir: &Path, lock: &str, stderr: &str) {
    assert_compile_refused(dir, "src/hut.crn", &["--lock", lock], 2, stderr, &["src"]);
}

/// The refusal of a `--lock` spelled `lock` that names `src/hut.crn`, followed by `fix`.
fn names_the_source(lock: &str, fix: &str) -> String {
    format!("error: `--lock {lock}` names `src/hut.crn`, the source being compiled\n{fix}")
}

/// The refusal of a `--lock` spelled `lock` whose file name ends in `.crn`, offering `suggested`.
fn names_a_crn(lock: &str, suggested: &str) -> String {
    format!(
        "error: `--lock {lock}` names a `.crn` file, and a lockfile is never a Cairn source\n  \
         note: give `--lock` a `.lock` path, such as `{suggested}`, or leave it out to write \
         `src/hut.crn.lock`\n"
    )
}

#[test]
fn a_lock_naming_the_source_is_refused_however_it_is_spelled() {
    let dir = sources_in("src", &["hut.crn"]);
    let absolute = dir.path().join("src").join("hut.crn");
    let absolute = absolute.to_str().expect("utf-8 path");
    for lock in [
        "src/hut.crn",
        "./src/hut.crn",
        "src/../src/hut.crn",
        absolute,
    ] {
        assert_refused(dir.path(), lock, &names_the_source(lock, BY_NAME_FIX));
    }
    assert_eq!(snapshot(dir.path()), BTreeMap::new(), "nothing beside src");
}

#[test]
fn a_lock_hard_linked_to_the_source_is_refused_with_the_link_fix() {
    let dir = sources_in("src", &["hut.crn"]);
    let src = dir.path().join("src");
    fs::hard_link(src.join("hut.crn"), src.join("hard.lock")).expect("hard-link the source");

    assert_refused(
        dir.path(),
        "src/hard.lock",
        &names_the_source("src/hard.lock", LINKED_FIX),
    );
}

#[cfg(unix)]
#[test]
fn a_lock_symlinked_to_the_source_is_refused_with_the_link_fix_and_a_linked_directory_by_name() {
    let dir = sources_in("src", &["hut.crn"]);
    let src = dir.path().join("src");
    std::os::unix::fs::symlink(&src, dir.path().join("alias")).expect("symlink the directory");
    std::os::unix::fs::symlink("hut.crn", src.join("soft.lock")).expect("symlink the source");

    for (lock, fix) in [
        ("alias/hut.crn", BY_NAME_FIX),
        ("src/soft.lock", LINKED_FIX),
    ] {
        assert_refused(dir.path(), lock, &names_the_source(lock, fix));
    }
}

#[test]
fn a_lock_at_the_default_path_linked_to_the_source_offers_no_default() {
    let dir = sources_in("src", &["hut.crn"]);
    let src = dir.path().join("src");
    fs::hard_link(src.join("hut.crn"), src.join("hut.crn.lock")).expect("hard-link the source");

    assert_refused(
        dir.path(),
        "src/hut.crn.lock",
        &names_the_source(
            "src/hut.crn.lock",
            "  note: a link makes them one file; give `--lock` a `.lock` path elsewhere\n",
        ),
    );
}

#[cfg(windows)]
#[test]
fn a_lock_reaching_the_source_through_a_junction_is_refused() {
    let dir = sources_in("src", &["hut.crn"]);
    let status = std::process::Command::new("cmd")
        .current_dir(dir.path())
        .args(["/C", "mklink", "/J", "alias", "src"])
        .status()
        .expect("run mklink");
    assert!(status.success(), "mklink /J failed");

    let lock = r"alias\hut.crn";
    assert_refused(dir.path(), lock, &names_the_source(lock, BY_NAME_FIX));
}

#[test]
fn a_lock_naming_the_source_in_another_case_is_the_source_only_where_the_directory_folds_case() {
    let dir = sources_in("src", &["hut.crn"]);
    let lock = "src/HUT.CRN";
    let stderr = if folds_case(&dir.path().join("src")) {
        names_the_source(lock, BY_NAME_FIX)
    } else {
        names_a_crn(lock, "src/HUT.CRN.lock")
    };
    assert_refused(dir.path(), lock, &stderr);
}

#[test]
fn a_lock_ending_in_crn_is_refused_whatever_it_names() {
    let dir = sources_in("src", &["hut.crn"]);
    fs::write(
        dir.path().join("src").join("other.crn"),
        "struct other size=3x3\n",
    )
    .expect("write another source");
    for (lock, suggested) in [
        ("src/other.crn", "src/other.crn.lock"),
        ("src/missing.crn", "src/missing.crn.lock"),
        ("build/hut.CRN", "build/hut.CRN.lock"),
    ] {
        assert_refused(dir.path(), lock, &names_a_crn(lock, suggested));
    }
    assert_eq!(snapshot(dir.path()), BTreeMap::new(), "nothing beside src");
    assert!(
        !dir.path().join("build").exists(),
        "no directory made for the lock"
    );
}

#[test]
fn a_lock_is_refused_before_the_source_is_read() {
    let dir = sources_in("src", &["hut.crn"]);
    fs::write(dir.path().join("src").join("hut.crn"), "struct\n").expect("break the source");

    let run = compile_in(dir.path(), "src/hut.crn", &["--lock", "src/hut.crn"]);

    assert_eq!(
        (run.code, run.stderr),
        (Some(2), names_the_source("src/hut.crn", BY_NAME_FIX)),
        "a usage error comes before the source's own findings",
    );
}

#[cfg(unix)]
#[test]
fn a_lock_the_file_system_cannot_answer_for_is_refused_before_the_source_is_read() {
    let dir = sources_in("src", &["hut.crn"]);
    fs::write(dir.path().join("src").join("hut.crn"), "struct\n").expect("break the source");
    let looped = dir.path().join("notes.tmp");
    std::os::unix::fs::symlink("notes.tmp", &looped).expect("symlink to itself");
    let error = fs::metadata(&looped).expect_err("a loop reaches no file");

    assert_compile_refused(
        dir.path(),
        "src/hut.crn",
        &["--lock", "notes"],
        1,
        &format!(
            "error: cannot tell whether lockfile `notes` would be written over `src/hut.crn`, \
             the source being compiled: `notes.tmp`: {error}\n  note: check that `notes.tmp` \
             can be read, or give `--lock` a `.lock` path elsewhere\n"
        ),
        &["src", "."],
    );
}
