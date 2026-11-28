use std::io::Write as _;

use super::*;

#[test]
fn staging_over_an_entry_at_the_scratch_path_fails_and_leaves_it_as_it_was() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("hut.nbt");
    let tmp_path = Scratch::Staged.of(&path);
    fs::write(&tmp_path, "mine").expect("plant the file");
    let mut staged = StagedSet::default();
    let err = staged
        .stage(&path, Kind::Artifact, |file| file.write_all(b"new"))
        .expect_err("the scratch path is taken");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&tmp_path).expect("reads"), "mine");
    assert!(fs::symlink_metadata(&path).is_err(), "nothing is placed");
}

#[test]
fn a_commit_that_finds_the_backup_path_taken_puts_the_directory_back() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("hut.nbt");
    fs::write(&path, "old").expect("write the previous build");
    let mut staged = StagedSet::default();
    staged
        .stage(&path, Kind::Artifact, |file| file.write_all(b"new"))
        .expect("stages");
    let backup_path = Scratch::Backup.of(&path);
    fs::write(&backup_path, "mine").expect("plant the file");
    let failure = staged.commit().expect_err("the backup path is taken");
    assert_eq!(
        failure.to_string(),
        format!(
            "the existing artifact could not be moved aside to `{}`: entity already exists",
            backup_path.display()
        ),
    );
    assert_eq!(fs::read_to_string(&path).expect("reads"), "old");
    assert_eq!(fs::read_to_string(&backup_path).expect("reads"), "mine");
    assert!(
        fs::symlink_metadata(Scratch::Staged.of(&path)).is_err(),
        "the staged file is thrown away"
    );
}

#[test]
fn a_commit_that_finds_a_later_backup_path_taken_puts_every_earlier_file_back() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let (first, second) = (dir.path().join("a.nbt"), dir.path().join("b.nbt"));
    fs::write(&first, "old-a").expect("write the previous build");
    fs::write(&second, "old-b").expect("write the previous build");
    let mut staged = StagedSet::default();
    for path in [&first, &second] {
        staged
            .stage(path, Kind::Artifact, |file| file.write_all(b"new"))
            .expect("stages");
    }
    let backup_path = Scratch::Backup.of(&second);
    fs::write(&backup_path, "mine").expect("plant the file");
    let failure = staged
        .commit()
        .expect_err("the second backup path is taken");
    assert_eq!(
        failure.to_string(),
        format!(
            "the existing artifact could not be moved aside to `{}`: entity already exists",
            backup_path.display()
        ),
    );
    let mut left: Vec<(String, String)> = fs::read_dir(dir.path())
        .expect("lists")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let body = fs::read_to_string(entry.path()).expect("reads");
            (entry.file_name().to_string_lossy().into_owned(), body)
        })
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            ("a.nbt", "old-a"),
            ("b.nbt", "old-b"),
            ("b.nbt.bak", "mine")
        ]
        .map(|(name, body)| (name.to_owned(), body.to_owned()))
    );
}
