use super::*;

/// How `dir` compares names, found by a second `create_new` under the other spelling.
fn create_new_answer(dir: &Path) -> NameCase {
    let create_new = |name: &str| {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
    };
    create_new("case-truth").expect("create the lower-case spelling");
    match create_new("CASE-TRUTH") {
        Ok(_) => NameCase::Sensitive,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => NameCase::Insensitive,
        Err(err) => panic!("create the upper-case spelling: {err}"),
    }
}

fn entries(dir: &Path) -> Vec<OsString> {
    fs::read_dir(dir)
        .expect("read the temp dir")
        .map(|entry| entry.expect("dir entry").file_name())
        .collect()
}

#[test]
fn the_probe_answers_how_the_temp_dir_compares_names() {
    let dir = tempfile::tempdir().expect("temp dir");

    let probed = NameCase::probe(dir.path()).expect("a writable temp dir can be probed");

    assert_eq!(
        entries(dir.path()),
        Vec::<OsString>::new(),
        "the probe left its file behind"
    );
    assert_eq!(probed, create_new_answer(dir.path()));
}

#[test]
fn a_directory_that_cannot_be_probed_is_taken_to_fold_case() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("missing");

    let err = NameCase::probe(&missing).expect_err("a missing directory holds no scratch file");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(NameCase::of_dir(&missing), NameCase::Insensitive);
    assert!(
        !missing.exists(),
        "the probe created the directory it was asked about"
    );
}

#[test]
fn only_a_folding_directory_keys_two_spellings_alike() {
    let keys = |case: NameCase| {
        (
            case.key(OsStr::new("Hut.NBT")),
            case.key(OsStr::new("hut.nbt")),
        )
    };

    assert_eq!(
        keys(NameCase::Insensitive),
        ("hut.nbt".into(), "hut.nbt".into())
    );
    assert_eq!(
        keys(NameCase::Sensitive),
        ("Hut.NBT".into(), "hut.nbt".into())
    );
}
