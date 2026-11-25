use std::fs;

use super::*;
use crate::file_identity::Reach;
use crate::write_path::Scratch;

/// A directory holding an empty `hut.crn`, removed when the test ends, and that file's path.
fn hut() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("hut.crn");
    fs::write(&file, "").expect("write source");
    (dir, file)
}

/// The refusal of a lock that `path` makes the source, `by` name or through a link.
fn source(path: WritePath, by: Reach) -> LockPathRefusal {
    LockPathRefusal::Source(Reached { path, by })
}

#[test]
fn the_default_lock_keeps_the_source_extension() {
    assert_eq!(
        default_lock_path(Path::new("site/shop.crn")),
        Path::new("site/shop.crn.lock"),
    );
}

#[test]
fn a_lock_path_reaching_the_source_is_refused_however_it_is_spelled() {
    let (dir, file) = hut();
    fs::create_dir(dir.path().join("sub")).expect("make sub");
    let refused = |lock: &Path| lock_path_refusal(&file, lock).expect("identified");

    for lock in [
        file.clone(),
        dir.path().join(".").join("hut.crn"),
        dir.path().join("sub").join("..").join("hut.crn"),
    ] {
        assert_eq!(
            refused(&lock),
            Some(source(WritePath::Final, Reach::Name)),
            "{lock:?}"
        );
    }
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
    fs::write(dir.path().join("hut.crn.lock"), "").expect("write lock");
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
}

#[test]
fn a_lock_path_hard_linked_to_the_source_is_refused_through_the_link() {
    let (dir, file) = hut();
    fs::hard_link(&file, dir.path().join("hard.lock")).expect("hard link");
    fs::hard_link(&file, dir.path().join("linked.lock.bak")).expect("hard link");

    assert_eq!(
        lock_path_refusal(&file, &dir.path().join("hard.lock")).expect("identified"),
        Some(source(WritePath::Final, Reach::Link)),
    );
    assert_eq!(
        lock_path_refusal(&file, &dir.path().join("linked.lock")).expect("identified"),
        Some(source(WritePath::Scratch(Scratch::Backup), Reach::Link)),
    );
}

#[cfg(unix)]
#[test]
fn a_lock_path_symlinked_to_the_source_is_refused_through_the_link() {
    let (dir, file) = hut();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("alias")).expect("symlink dir");
    std::os::unix::fs::symlink(&file, dir.path().join("soft.lock")).expect("symlink");

    for (lock, by) in [("alias/hut.crn", Reach::Name), ("soft.lock", Reach::Link)] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)).expect("identified"),
            Some(source(WritePath::Final, by)),
            "{lock}",
        );
    }
}

#[test]
fn a_lock_path_whose_scratch_file_reaches_the_source_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock = dir.path().join("notes");
    let refused = |name: &str| {
        let file = dir.path().join(name);
        fs::write(&file, "").expect("write source");
        lock_path_refusal(&file, &lock).expect("identified")
    };

    assert_eq!(
        refused("notes.tmp"),
        Some(source(WritePath::Scratch(Scratch::Staged), Reach::Name)),
    );
    assert_eq!(
        refused("notes.bak"),
        Some(source(WritePath::Scratch(Scratch::Backup), Reach::Name)),
    );
    assert_eq!(refused("notes.crn"), None);
}

#[test]
fn a_lock_path_ending_in_crn_is_refused_whatever_it_names() {
    let (dir, file) = hut();
    fs::write(dir.path().join("other.crn"), "").expect("write other source");

    for lock in [
        "other.crn",
        "missing.crn",
        "OTHER.CRN",
        ".crn",
        "no/such/dir/x.Crn",
        "other.crn.",
        "other.crn ",
        "other.CRN. .",
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)).expect("identified"),
            Some(LockPathRefusal::SourceName),
            "{lock:?}",
        );
    }
    for lock in [
        "other.crn.lock",
        "crn",
        "crn.",
        "other.crnx",
        "other.crn.d/hut.lock",
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)).expect("identified"),
            None,
            "{lock:?}"
        );
    }
}

#[test]
fn a_lock_refused_by_name_is_offered_with_lock_added_and_no_trailing_dot_or_space() {
    for (lock, suggested) in [
        ("src/other.crn", "src/other.crn.lock"),
        ("src/other.crn.", "src/other.crn.lock"),
        ("src/other.CRN .", "src/other.CRN.lock"),
    ] {
        assert_eq!(
            suggested_lock_path(Path::new(lock)),
            Path::new(suggested),
            "{lock:?}"
        );
    }
}

#[test]
fn the_default_lock_is_refused_only_where_a_link_makes_one_of_its_paths_the_source() {
    let (dir, file) = hut();
    assert_eq!(default_lock_refusal(&file).expect("identified"), None);

    for (name, path) in [
        ("hut.crn.lock.tmp", WritePath::Scratch(Scratch::Staged)),
        ("hut.crn.lock", WritePath::Final),
    ] {
        fs::hard_link(&file, dir.path().join(name)).expect("hard link");
        assert_eq!(
            default_lock_refusal(&file).expect("identified"),
            Some(path),
            "{name}"
        );
    }
}

#[test]
fn a_source_that_is_not_there_refuses_no_lock_path_but_by_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("missing.crn");

    assert_eq!(
        lock_path_refusal(&file, &file).expect("no file is no error"),
        Some(LockPathRefusal::SourceName)
    );
    assert_eq!(
        lock_path_refusal(&file, &dir.path().join("missing.crn.lock"))
            .expect("no file is no error"),
        None
    );
    assert_eq!(
        default_lock_refusal(&file).expect("no file is no error"),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_lock_path_the_file_system_cannot_answer_for_is_unidentified() {
    let (dir, file) = hut();
    let looped = dir.path().join("notes.tmp");
    std::os::unix::fs::symlink(&looped, &looped).expect("symlink to itself");
    let expected = fs::metadata(&looped).expect_err("a loop reaches no file");
    std::os::unix::fs::symlink("hut.crn.lock.bak", dir.path().join("hut.crn.lock.bak"))
        .expect("symlink to itself");

    let unidentified =
        lock_path_refusal(&file, &dir.path().join("notes")).expect_err("a loop is no answer");
    assert_eq!(
        (unidentified.path, unidentified.error.kind()),
        (looped, expected.kind())
    );
    let unidentified = default_lock_refusal(&file).expect_err("a loop is no answer");
    assert_eq!(unidentified.path, dir.path().join("hut.crn.lock.bak"));
}
