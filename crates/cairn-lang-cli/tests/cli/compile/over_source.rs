//! What a compile refuses to write over the source it compiles: an artifact, the lock, or a
//! scratch file beside either (`spec/versioning-editions` "Provenance and lock").

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{HUT, Run, cargo_bin, snapshot};

/// The note under a refused artifact.
const ARTIFACT_FIX: &str = "  note: compile with `--out` naming another directory, or give the \
                            source a name ending in `.crn`\n";

/// The note under a refused `<source>.lock`.
#[cfg(unix)]
const LINKED_LOCK_FIX: &str =
    "  note: a link makes them one file; give `--lock` a `.lock` path elsewhere\n";

/// Compile `source` from `dir` with `cairn compile <source> --edition java <flags>`, and assert it
/// exits `code` with `stderr` alone, every file in `dir/sub` for each of `subs` as it was, and the
/// source byte for byte as it was.
fn assert_refused(
    dir: &Path,
    source: &str,
    flags: &[&str],
    code: i32,
    stderr: &str,
    subs: &[&str],
) {
    let before: Vec<_> = subs.iter().map(|sub| snapshot(&dir.join(sub))).collect();
    let text = fs::read(dir.join(source)).expect("read source");

    let run = Run::from(
        Command::new(cargo_bin())
            .current_dir(dir)
            .args(["compile", source, "--edition", "java"])
            .args(flags)
            .output()
            .expect("run cairn"),
    );

    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (Some(code), "", stderr),
        "{source} {flags:?}",
    );
    let after: Vec<_> = subs.iter().map(|sub| snapshot(&dir.join(sub))).collect();
    assert_eq!(after, before, "{source} {flags:?}");
    assert_eq!(fs::read(dir.join(source)).expect("read source"), text);
}

/// A directory holding [`HUT`] under each of `names`, in `a/`.
fn sources_in_a(names: &[&str]) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir(dir.path().join("a")).expect("make a");
    for name in names {
        fs::write(dir.path().join("a").join(name), HUT).expect("write source");
    }
    dir
}

#[test]
fn an_artifact_named_as_the_source_is_refused() {
    let dir = sources_in_a(&["hut.nbt"]);
    let artifact = Path::new("a").join("hut.nbt");

    let stderr = format!(
        "error: artifact `{}` names `a/hut.nbt`, the source being compiled\n{ARTIFACT_FIX}",
        artifact.display(),
    );
    assert_refused(dir.path(), "a/hut.nbt", &[], 1, &stderr, &["a"]);
}

#[test]
fn an_artifact_whose_scratch_file_is_the_source_is_refused() {
    let dir = sources_in_a(&["hut.nbt.tmp", "hut.nbt.bak"]);
    let artifact = Path::new("a").join("hut.nbt");

    for (source, scratch) in [
        ("a/hut.nbt.tmp", "hut.nbt.tmp"),
        ("a/hut.nbt.bak", "hut.nbt.bak"),
    ] {
        let stderr = format!(
            "error: artifact `{}` would put its scratch file `{}` over `{source}`, the source \
             being compiled\n{ARTIFACT_FIX}",
            artifact.display(),
            Path::new("a").join(scratch).display(),
        );
        assert_refused(dir.path(), source, &[], 1, &stderr, &["a"]);
    }
}

#[test]
fn a_lock_whose_scratch_file_is_the_source_is_refused_before_the_source_is_read() {
    let dir = TempDir::new().expect("tempdir");
    for name in ["notes.tmp", "notes.bak"] {
        fs::write(dir.path().join(name), "struct\n").expect("write a source that does not parse");
    }

    for source in ["notes.tmp", "notes.bak"] {
        let stderr = format!(
            "error: `--lock notes` would put its scratch file `{source}` over `{source}`, the \
             source being compiled\n  note: give `--lock` a `.lock` path, or leave it out to \
             write `{source}.lock`\n"
        );
        assert_refused(dir.path(), source, &["--lock", "notes"], 2, &stderr, &["."]);
    }
}

#[cfg(unix)]
#[test]
fn an_artifact_linked_to_the_source_is_refused() {
    let dir = TempDir::new().expect("tempdir");
    for sub in ["src", "out", "hard"] {
        fs::create_dir(dir.path().join(sub)).expect("make dir");
    }
    fs::write(dir.path().join("src/hut.crn"), HUT).expect("write source");
    std::os::unix::fs::symlink("../src/hut.crn", dir.path().join("out/hut.nbt")).expect("symlink");
    fs::hard_link(
        dir.path().join("src/hut.crn"),
        dir.path().join("hard/hut.nbt.tmp"),
    )
    .expect("hard link");

    let soft = format!(
        "error: artifact `out/hut.nbt` names `src/hut.crn`, the source being compiled\n\
         {ARTIFACT_FIX}"
    );
    let hard = format!(
        "error: artifact `hard/hut.nbt` would put its scratch file `hard/hut.nbt.tmp` over \
         `src/hut.crn`, the source being compiled\n{ARTIFACT_FIX}"
    );
    for (out, stderr) in [("out", soft), ("hard", hard)] {
        let subs = ["src", out];
        assert_refused(
            dir.path(),
            "src/hut.crn",
            &["--out", out],
            1,
            &stderr,
            &subs,
        );
    }
}

#[cfg(unix)]
#[test]
fn a_default_lock_linked_to_the_source_is_refused() {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("make src");
    fs::write(dir.path().join("src/hut.crn"), HUT).expect("write source");
    let refused = |link: &str| {
        let why = match link {
            "hut.crn.lock" => "names `src/hut.crn`".to_owned(),
            scratch => format!("would put its scratch file `src/{scratch}` over `src/hut.crn`"),
        };
        format!(
            "error: lockfile `src/hut.crn.lock` {why}, the source being compiled\n\
             {LINKED_LOCK_FIX}"
        )
    };

    for link in ["hut.crn.lock", "hut.crn.lock.tmp", "hut.crn.lock.bak"] {
        let path = dir.path().join("src").join(link);
        std::os::unix::fs::symlink("hut.crn", &path).expect("symlink");
        let subs = ["src", "out"];
        assert_refused(
            dir.path(),
            "src/hut.crn",
            &["--out", "out"],
            1,
            &refused(link),
            &subs,
        );
        fs::remove_file(path).expect("remove the link");
    }
}
