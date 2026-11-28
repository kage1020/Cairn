//! A compile writes each file through the two scratch paths beside it, and an entry already at
//! either refuses the build before anything is written (`spec/versioning-editions` "Provenance and
//! lock").

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{Fixture, Run, compile, compile_as};

const HUT: &str =
    "theme t:\n  slot w -> @stone\n\nstruct hut size=3x3\n  walls mat_slot=w height=2\n";

/// `path` with `suffix` added to its file name.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_owned();
    raw.push(suffix);
    PathBuf::from(raw)
}

/// The refusal of a file `kind` at `path` for an entry at `scratch`.
fn refusal(kind: &str, path: &Path, scratch: &Path) -> String {
    format!(
        "error: {kind} `{}` is written through `{}`, which already exists\n  note: a file is \
         written at its path with `.tmp` added and renamed into place, and one it replaces is \
         moved aside to its path with `.bak` added, so the entry may be one an earlier run left \
         behind\n  fix: move `{}` aside, then compile again\n",
        path.display(),
        scratch.display(),
        scratch.display(),
    )
}

#[test]
fn a_file_at_a_scratch_path_refuses_the_build_and_is_left_as_it_was() {
    for (kind, suffix) in [
        ("artifact", ".tmp"),
        ("artifact", ".bak"),
        ("lockfile", ".tmp"),
        ("lockfile", ".bak"),
    ] {
        let fixture = Fixture::new(HUT);
        let first = Run::from(compile_as(&fixture, "java", "latest"));
        assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
        let artifact = fixture.out().join("hut.nbt");
        let path = if kind == "artifact" {
            artifact.clone()
        } else {
            fixture.lock()
        };
        let before = (fs::read(&artifact), fs::read(fixture.lock()));
        let planted = beside(&path, suffix);
        fs::write(&planted, "my backup").expect("plant the file");
        // A build that went through would now write other bytes.
        fs::write(fixture.source(), HUT.replace("height=2", "height=3")).expect("edit the source");

        let run = Run::from(compile_as(&fixture, "java", "latest"));
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr),
            (Some(1), "", refusal(kind, &path, &planted)),
            "{kind}{suffix}"
        );
        assert_eq!(
            fs::read_to_string(&planted).expect("the planted file reads"),
            "my backup"
        );
        let after = (fs::read(&artifact), fs::read(fixture.lock()));
        assert_eq!(
            (after.0.ok(), after.1.ok()),
            (before.0.ok(), before.1.ok()),
            "{kind}{suffix}"
        );
    }
}

#[test]
fn every_scratch_path_holding_an_entry_is_named() {
    let fixture = Fixture::new(HUT);
    let artifact = fixture.out().join("hut.nbt");
    fs::create_dir_all(fixture.out()).expect("make --out");
    let planted = [beside(&artifact, ".bak"), beside(&fixture.lock(), ".tmp")];
    for path in &planted {
        fs::create_dir(path).expect("plant a folder");
    }
    let run = Run::from(compile_as(&fixture, "java", "latest"));
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr),
        (
            Some(1),
            "",
            format!(
                "{}{}",
                refusal("artifact", &artifact, &planted[0]),
                refusal("lockfile", &fixture.lock(), &planted[1])
            )
        ),
    );
    assert!(!artifact.exists(), "no artifact is written");
    assert!(!fixture.lock().exists(), "no lockfile is written");
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_at_a_scratch_path_is_not_written_through() {
    for dangling in [false, true] {
        let fixture = Fixture::new(HUT);
        let victim = fixture.source().with_file_name("victim.txt");
        if !dangling {
            fs::write(&victim, "precious").expect("write the victim");
        }
        fs::create_dir_all(fixture.out()).expect("make --out");
        let artifact = fixture.out().join("hut.nbt");
        let link = beside(&artifact, ".tmp");
        std::os::unix::fs::symlink(&victim, &link).expect("plant the link");

        let run = Run::from(compile_as(&fixture, "java", "latest"));
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr),
            (Some(1), "", refusal("artifact", &artifact, &link)),
            "dangling={dangling}"
        );
        assert_eq!(
            fs::read_to_string(&victim).ok(),
            (!dangling).then(|| "precious".to_owned()),
            "dangling={dangling}"
        );
        assert!(
            fs::symlink_metadata(&link)
                .expect("the link is there")
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(&artifact).is_err(),
            "no artifact is written"
        );
    }
}

#[test]
fn a_lock_under_a_file_is_refused_by_its_write_rather_than_its_scratch_check() {
    let fixture = Fixture::new(HUT);
    let notes = fixture.source().with_file_name("notes.txt");
    fs::write(&notes, "mine").expect("write the file");
    let lock = notes.join("hut.crn.lock");
    let run = Run::from(compile(
        &fixture.source(),
        "java",
        "latest",
        &fixture.out(),
        Some(&lock),
    ));
    assert_eq!((run.code, run.stdout.as_str()), (Some(1), ""));
    let prefix = format!("error: writing lockfile `{}`: ", lock.display());
    assert!(
        run.stderr.starts_with(&prefix) && run.stderr.lines().count() == 1,
        "only the write reports the fault, in the platform's own words: {}",
        run.stderr
    );
    assert_eq!(
        fixture.artifacts(),
        ["notes.txt"],
        "the staged artifact is thrown away"
    );
}
