//! What a compile refuses to write over the source it compiles: an artifact, the lock, or a
//! scratch file beside either (`spec/versioning-editions` "Provenance and lock").

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{assert_compile_refused, sources_in};

/// The note under an artifact that is the source's own entry.
const ARTIFACT_FIX: &str = "  note: compile with `--out` naming another directory, or give the \
                            source a name ending in `.crn`\n";

/// The note under an artifact a link makes the source.
const LINKED_ARTIFACT_FIX: &str =
    "  note: a link makes them one file; compile with `--out` naming another directory\n";

/// The note under a refused `<source>.lock`.
const LINKED_LOCK_FIX: &str =
    "  note: a link makes them one file; give `--lock` a `.lock` path elsewhere\n";

/// The refusal of artifact `artifact`, written through `through`, over `source`, then `fix`.
fn over_the_source(artifact: &Path, through: Option<&Path>, source: &str, fix: &str) -> String {
    let artifact = artifact.display();
    match through {
        None => format!(
            "error: artifact `{artifact}` names `{source}`, the source being compiled\n{fix}"
        ),
        Some(scratch) => format!(
            "error: artifact `{artifact}` would put its scratch file `{}` over `{source}`, the \
             source being compiled\n{fix}",
            scratch.display(),
        ),
    }
}

#[test]
fn an_artifact_named_as_the_source_is_refused() {
    let dir = sources_in("a", &["hut.nbt"]);
    let artifact = Path::new("a").join("hut.nbt");

    let stderr = over_the_source(&artifact, None, "a/hut.nbt", ARTIFACT_FIX);
    assert_compile_refused(dir.path(), "a/hut.nbt", &[], 1, &stderr, &["a"]);
}

#[test]
fn an_artifact_whose_scratch_file_is_the_source_is_refused() {
    let dir = sources_in("a", &["hut.nbt.tmp", "hut.nbt.bak"]);
    let artifact = Path::new("a").join("hut.nbt");

    for (source, scratch) in [
        ("a/hut.nbt.tmp", "hut.nbt.tmp"),
        ("a/hut.nbt.bak", "hut.nbt.bak"),
    ] {
        let scratch = Path::new("a").join(scratch);
        let stderr = over_the_source(&artifact, Some(&scratch), source, ARTIFACT_FIX);
        assert_compile_refused(dir.path(), source, &[], 1, &stderr, &["a"]);
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
        assert_compile_refused(dir.path(), source, &["--lock", "notes"], 2, &stderr, &["."]);
    }
}

#[test]
fn an_artifact_hard_linked_to_the_source_is_refused_with_the_link_fix() {
    let dir = sources_in("src", &["hut.crn"]);
    let source = dir.path().join("src").join("hut.crn");
    for (out, name) in [("final", "hut.nbt"), ("staged", "hut.nbt.tmp")] {
        fs::create_dir(dir.path().join(out)).expect("make out");
        fs::hard_link(&source, dir.path().join(out).join(name)).expect("hard link");
    }

    for (out, through) in [("final", None), ("staged", Some("hut.nbt.tmp"))] {
        let artifact = Path::new(out).join("hut.nbt");
        let through = through.map(|name| Path::new(out).join(name));
        let stderr = over_the_source(
            &artifact,
            through.as_deref(),
            "src/hut.crn",
            LINKED_ARTIFACT_FIX,
        );
        let flags = ["--out", out];
        assert_compile_refused(dir.path(), "src/hut.crn", &flags, 1, &stderr, &["src", out]);
    }
}

#[cfg(unix)]
#[test]
fn an_artifact_symlinked_to_the_source_is_refused_with_the_link_fix() {
    let dir = sources_in("src", &["hut.crn"]);
    fs::create_dir(dir.path().join("out")).expect("make out");
    std::os::unix::fs::symlink("../src/hut.crn", dir.path().join("out/hut.nbt")).expect("symlink");

    let stderr = over_the_source(
        Path::new("out/hut.nbt"),
        None,
        "src/hut.crn",
        LINKED_ARTIFACT_FIX,
    );
    let flags = ["--out", "out"];
    assert_compile_refused(
        dir.path(),
        "src/hut.crn",
        &flags,
        1,
        &stderr,
        &["src", "out"],
    );
}

#[cfg(unix)]
#[test]
fn an_out_linked_to_the_source_directory_names_the_source_by_name() {
    let dir = sources_in("src", &["hut.nbt"]);
    std::os::unix::fs::symlink("src", dir.path().join("alias")).expect("symlink the directory");

    let stderr = over_the_source(
        Path::new("alias/hut.nbt"),
        None,
        "src/hut.nbt",
        ARTIFACT_FIX,
    );
    let flags = ["--out", "alias"];
    assert_compile_refused(dir.path(), "src/hut.nbt", &flags, 1, &stderr, &["src"]);
}

#[test]
fn a_default_lock_linked_to_the_source_is_refused() {
    let dir = sources_in("src", &["hut.crn"]);
    let source = dir.path().join("src").join("hut.crn");
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

    for link in Link::ALL {
        for name in ["hut.crn.lock", "hut.crn.lock.tmp", "hut.crn.lock.bak"] {
            let path = dir.path().join("src").join(name);
            link.make(&source, &path);
            let flags = ["--out", "out"];
            let subs = ["src", "out"];
            assert_compile_refused(dir.path(), "src/hut.crn", &flags, 1, &refused(name), &subs);
            fs::remove_file(path).expect("remove the link");
        }
    }
}

/// A link a test makes to the source.
#[derive(Debug, Clone, Copy)]
enum Link {
    Hard,
    #[cfg(unix)]
    Symbolic,
}

impl Link {
    /// Every link this platform makes without privilege.
    const ALL: &[Self] = &[
        Self::Hard,
        #[cfg(unix)]
        Self::Symbolic,
    ];

    /// Make `path` this link to `source`.
    fn make(self, source: &Path, path: &Path) {
        match self {
            Self::Hard => fs::hard_link(source, path),
            #[cfg(unix)]
            Self::Symbolic => std::os::unix::fs::symlink(source, path),
        }
        .unwrap_or_else(|err| panic!("{self:?} link {}: {err}", path.display()));
    }
}

#[cfg(unix)]
#[test]
fn an_artifact_the_file_system_cannot_answer_for_is_refused() {
    let dir = sources_in("src", &["hut.crn"]);
    fs::create_dir(dir.path().join("out")).expect("make out");
    let looped = dir.path().join("out/hut.nbt.tmp");
    std::os::unix::fs::symlink("hut.nbt.tmp", &looped).expect("symlink to itself");
    let error = fs::metadata(&looped).expect_err("a loop reaches no file");

    let stderr = format!(
        "error: cannot tell whether artifact `out/hut.nbt` would be written over `src/hut.crn`, \
         the source being compiled: `out/hut.nbt.tmp`: {error}\n  note: check that \
         `out/hut.nbt.tmp` can be read, or compile with `--out` naming another directory\n"
    );
    let flags = ["--out", "out"];
    assert_compile_refused(
        dir.path(),
        "src/hut.crn",
        &flags,
        1,
        &stderr,
        &["src", "out"],
    );
}
