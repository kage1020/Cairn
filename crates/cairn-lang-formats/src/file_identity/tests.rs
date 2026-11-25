use std::fs;

use super::*;

/// A directory holding an empty `hut.crn`, removed when the test ends, and that file's path.
fn hut() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("hut.crn");
    fs::write(&file, "").expect("write file");
    (dir, file)
}

/// How `path` reaches the file at `file`.
fn reach(file: &Path, path: &Path) -> Option<Reach> {
    NamedFile::at(file)
        .expect("the file is identified")
        .expect("the file is there")
        .reached_by(path)
        .expect("the path is identified")
}

#[test]
fn every_spelling_of_a_file_reaches_it_by_name_and_no_other_path_reaches_it() {
    let (dir, file) = hut();
    fs::write(dir.path().join("other.crn"), "").expect("write other");
    fs::create_dir(dir.path().join("sub")).expect("make sub");

    for (path, expected) in [
        (file.clone(), Some(Reach::Name)),
        (
            dir.path().join("sub").join("..").join("hut.crn"),
            Some(Reach::Name),
        ),
        (dir.path().join(".").join("hut.crn"), Some(Reach::Name)),
        (dir.path().join("other.crn"), None),
        (dir.path().join("missing.crn"), None),
        (dir.path().join("missing").join("hut.crn"), None),
        (file.join("hut.crn"), None),
        (dir.path().join("sub"), None),
    ] {
        assert_eq!(reach(&file, &path), expected, "{path:?}");
    }
    assert!(
        NamedFile::at(&dir.path().join("missing.crn"))
            .expect("a missing file is no error")
            .is_none()
    );
}

#[test]
fn a_hard_link_reaches_the_file_through_a_link() {
    let (dir, file) = hut();
    fs::create_dir(dir.path().join("sub")).expect("make sub");
    for link in ["hard.lock", "sub/hut.crn"] {
        fs::hard_link(&file, dir.path().join(link)).expect("hard link");
        assert_eq!(
            reach(&file, &dir.path().join(link)),
            Some(Reach::Link),
            "{link}"
        );
        assert_eq!(
            FileIdentity::of(&file).expect("identity"),
            FileIdentity::of(&dir.path().join(link)).expect("identity"),
            "{link}",
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_reaches_the_file_through_a_link_and_a_linked_directory_by_name() {
    let (dir, file) = hut();
    std::os::unix::fs::symlink(&file, dir.path().join("soft")).expect("symlink");
    std::os::unix::fs::symlink(dir.path(), dir.path().join("alias")).expect("symlink dir");

    assert_eq!(reach(&file, &dir.path().join("soft")), Some(Reach::Link));
    assert_eq!(
        reach(&file, &dir.path().join("alias").join("hut.crn")),
        Some(Reach::Name)
    );
}

#[cfg(windows)]
#[test]
fn a_directory_junction_reaches_the_file_by_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(dir.path().join("src")).expect("make src");
    let file = dir.path().join("src").join("hut.crn");
    fs::write(&file, "").expect("write file");
    let status = std::process::Command::new("cmd")
        .current_dir(dir.path())
        .args(["/C", "mklink", "/J", "alias", "src"])
        .status()
        .expect("run mklink");
    assert!(status.success(), "mklink /J failed");

    assert_eq!(
        reach(&file, &dir.path().join("alias").join("hut.crn")),
        Some(Reach::Name)
    );
}

#[test]
fn another_letter_case_reaches_the_file_by_name_only_where_the_directory_folds_case() {
    let (dir, file) = hut();
    let upper = dir.path().join("HUT.CRN");
    let folds = upper.exists();

    assert_eq!(
        reach(&file, &upper),
        folds.then_some(Reach::Name),
        "folds case: {folds}",
    );
}

#[test]
fn a_trailing_dot_or_space_reaches_the_file_by_name_only_on_windows() {
    let (dir, file) = hut();
    for name in ["hut.crn.", "hut.crn ", "hut.crn. ."] {
        assert_eq!(
            reach(&file, &dir.path().join(name)),
            cfg!(windows).then_some(Reach::Name),
            "{name:?}",
        );
    }
}

#[cfg(unix)]
#[test]
fn a_path_the_file_system_cannot_answer_for_is_unidentified_rather_than_absent() {
    let (dir, file) = hut();
    let looped = dir.path().join("loop");
    std::os::unix::fs::symlink(&looped, &looped).expect("symlink to itself");
    let expected = fs::metadata(&looped).expect_err("a loop reaches no file");

    let at = NamedFile::at(&looped).expect_err("a loop is no answer");
    let reached = NamedFile::at(&file)
        .expect("identified")
        .expect("there")
        .reached_by(&looped)
        .expect_err("a loop is no answer");

    for unidentified in [at, reached] {
        assert_eq!(unidentified.path, looped);
        assert_eq!(unidentified.error.kind(), expected.kind());
        assert_eq!(
            unidentified.to_string(),
            format!("`{}`: {expected}", looped.display())
        );
    }
}
