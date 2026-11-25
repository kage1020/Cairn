use std::fs;

use super::*;
use crate::write_path::Scratch;

/// A directory holding an empty `hut.crn`, removed when the test ends, and that file's path.
fn hut() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("hut.crn");
    fs::write(&file, "").expect("write source");
    (dir, file)
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
    let refused = |lock: &Path| lock_path_refusal(&file, lock);

    for lock in [
        file.clone(),
        dir.path().join(".").join("hut.crn"),
        dir.path().join("sub").join("..").join("hut.crn"),
    ] {
        assert_eq!(
            refused(&lock),
            Some(LockPathRefusal::Source(WritePath::Final)),
            "{lock:?}"
        );
    }
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
    fs::write(dir.path().join("hut.crn.lock"), "").expect("write lock");
    assert_eq!(refused(&dir.path().join("hut.crn.lock")), None);
}

#[test]
fn a_lock_path_hard_linked_to_the_source_is_refused() {
    let (dir, file) = hut();
    fs::hard_link(&file, dir.path().join("hard.lock")).expect("hard link");

    assert_eq!(
        lock_path_refusal(&file, &dir.path().join("hard.lock")),
        Some(LockPathRefusal::Source(WritePath::Final)),
    );
}

#[cfg(unix)]
#[test]
fn a_lock_path_symlinked_to_the_source_is_refused() {
    let (dir, file) = hut();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("alias")).expect("symlink dir");
    std::os::unix::fs::symlink(&file, dir.path().join("soft.lock")).expect("symlink");

    for lock in ["alias/hut.crn", "soft.lock"] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            Some(LockPathRefusal::Source(WritePath::Final)),
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
        lock_path_refusal(&file, &lock)
    };

    assert_eq!(
        refused("notes.tmp"),
        Some(LockPathRefusal::Source(WritePath::Scratch(Scratch::Staged))),
    );
    assert_eq!(
        refused("notes.bak"),
        Some(LockPathRefusal::Source(WritePath::Scratch(Scratch::Backup))),
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
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            Some(LockPathRefusal::SourceName),
            "{lock:?}",
        );
    }
    for lock in [
        "other.crn.lock",
        "crn",
        "other.crnx",
        "other.crn.d/hut.lock",
    ] {
        assert_eq!(
            lock_path_refusal(&file, &dir.path().join(lock)),
            None,
            "{lock:?}"
        );
    }
}

#[test]
fn the_default_lock_is_refused_only_where_a_link_makes_one_of_its_paths_the_source() {
    let (dir, file) = hut();
    assert_eq!(default_lock_refusal(&file), None);

    for (name, path) in [
        ("hut.crn.lock.tmp", WritePath::Scratch(Scratch::Staged)),
        ("hut.crn.lock", WritePath::Final),
    ] {
        fs::hard_link(&file, dir.path().join(name)).expect("hard link");
        assert_eq!(default_lock_refusal(&file), Some(path), "{name}");
    }
}
