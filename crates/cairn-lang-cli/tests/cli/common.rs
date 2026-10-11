//! Helpers shared by the test modules: where the binary and the examples
//! are, how to run one, and the temp directories a run writes into.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use flate2::read::GzDecoder;
use tempfile::TempDir;

/// The `cairn` binary cargo built for this test run.
pub fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// The number of `.crn` files `examples/` ships. Bump it with the next
/// example; every sweep refuses a different count, and a loop over a
/// filtered-down set passes silently.
pub const SHIPPED_EXAMPLES: usize = 12;

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Run the binary with `argv` and wait for it to exit.
pub fn cairn_argv(argv: &[&str]) -> Output {
    Command::new(cargo_bin())
        .args(argv)
        .output()
        .expect("failed to invoke cairn binary")
}

/// Run one subcommand of the binary and wait for it to exit.
pub fn cairn(subcommand: &str, args: &[&str]) -> Output {
    Command::new(cargo_bin())
        .arg(subcommand)
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

/// Copy `examples/<name>` into a fresh temp dir so a test can rely on the
/// source's parent being writable without leaving lock artefacts in the
/// repository.
pub fn example_in_tempdir(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join(name);
    fs::copy(examples_dir().join(name), &dst).expect("copy example");
    (tmp, dst)
}

/// Write `source` to `dir/name` and return the path.
pub fn write_source(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, source).expect("write source");
    path
}

/// Every `.crn` under `examples/`, sorted, so a sweep covers a new example
/// the moment it lands. Refuses a set too small to be the shipped one: a
/// sweep over nothing passes.
pub fn crn_examples() -> Vec<PathBuf> {
    let dir = examples_dir();
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
    let mut found: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("cannot read an entry of {}: {err}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        SHIPPED_EXAMPLES,
        "found {} examples under {}, not the {SHIPPED_EXAMPLES} shipped; bump SHIPPED_EXAMPLES \
         when adding one",
        found.len(),
        dir.display(),
    );
    found
}

/// A source plus the directory its artifacts would land in, both removed
/// when the test ends.
pub struct Fixture {
    /// Directory holding the source, the output, and the lock.
    dir: TempDir,
}

impl Fixture {
    /// Write `source` as `s.crn` in a fresh directory.
    pub fn new(source: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("s.crn"), source).expect("write source");
        Self { dir }
    }

    pub fn source(&self) -> PathBuf {
        self.dir.path().join("s.crn")
    }

    pub fn lock(&self) -> PathBuf {
        self.dir.path().join("s.crn.lock")
    }

    pub fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    /// Every file the compile could have produced.
    ///
    /// Every I/O failure panics rather than reading as "nothing there":
    /// the absence of artifacts is what some of these tests observe, and a
    /// walk that quietly gives up folds them toward passing.
    pub fn artifacts(&self) -> Vec<String> {
        fn walk(dir: &Path, into: &mut Vec<String>) {
            let entries = fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
            for entry in entries {
                let path = entry
                    .unwrap_or_else(|err| {
                        panic!("cannot read an entry of {}: {err}", dir.display())
                    })
                    .path();
                if path.is_dir() {
                    walk(&path, into);
                } else if path.extension().and_then(|e| e.to_str()) != Some("crn") {
                    into.push(path.file_name().expect("named").to_string_lossy().into());
                }
            }
        }
        let mut found = Vec::new();
        walk(self.dir.path(), &mut found);
        found.sort();
        found
    }
}

/// `cairn compile <src> --edition <edition> --target <target> --out <out>`,
/// with `--lock <lock>` when one is given; without it the lockfile lands
/// beside the source.
pub fn compile(src: &Path, edition: &str, target: &str, out: &Path, lock: Option<&Path>) -> Output {
    let mut command = Command::new(cargo_bin());
    command
        .arg("compile")
        .arg(src)
        .args(["--edition", edition, "--target", target])
        .arg("--out")
        .arg(out);
    if let Some(lock) = lock {
        command.arg("--lock").arg(lock);
    }
    command.output().expect("failed to invoke cairn binary")
}

/// `cairn compile` of the fixture's source for `edition` / `target`, with
/// the output directory and the lockfile inside the fixture's directory.
pub fn compile_as(fixture: &Fixture, edition: &str, target: &str) -> Output {
    compile(
        &fixture.source(),
        edition,
        target,
        &fixture.out(),
        Some(&fixture.lock()),
    )
}

/// What a run of the binary reported: its exit code and both streams as
/// text.
pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Run {
    fn from(out: Output) -> Self {
        Self {
            code: out.status.code(),
            stdout: String::from_utf8(out.stdout).expect("stdout utf-8"),
            stderr: String::from_utf8(out.stderr).expect("stderr utf-8"),
        }
    }
}

/// A run's stderr as text, for an assertion and its failure message.
pub fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The NBT inside a Java `.nbt` artifact. The writer gzips, so a search
/// over the file as written finds nothing whatever it is asked for.
pub fn nbt_body(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).expect("read artifact");
    let mut body = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut body)
        .expect("gzip decode");
    body
}

/// Whether `needle` occurs in `hay`. Both writers store a block id as the
/// bytes of a string tag, so a palette assertion is a byte search over the
/// NBT body.
pub fn bytes_contain(hay: &[u8], needle: impl AsRef<[u8]>) -> bool {
    let needle = needle.as_ref();
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A diagnostic reduced to what an assertion compares: where it points
/// (line, column), how bad it is, and which code it carries. The prose is
/// left out; the code, not the wording, is the contract.
pub type Reported = (u64, u64, String, String);

/// Diagnostics parsed back out of a command's stderr, in emission order.
/// Lines that are not `FILE:LINE:COL: SEVERITY[CODE]: ...` (notes,
/// `wrote ...`, plain `error:` messages) are skipped.
///
/// Parsing rather than substring-matching pins which code was reported,
/// where, and how often.
pub fn stderr_stream(stderr: &str) -> Vec<Reported> {
    stderr
        .lines()
        .filter_map(|line| {
            // The file path may itself contain `:` (`C:\...`), so anchor on
            // the severity marker and walk backwards from there.
            let (head, tail) = ["error[", "warning["].iter().find_map(|marker| {
                let needle = format!(": {marker}");
                line.find(&needle)
                    .map(|i| (&line[..i], &line[i + ": ".len()..]))
            })?;
            let (severity, tail) = tail.split_once('[')?;
            let (code, _) = tail.split_once(']')?;
            let (head, col) = head.rsplit_once(':')?;
            let (_, line_no) = head.rsplit_once(':')?;
            Some((
                line_no.parse().ok()?,
                col.parse().ok()?,
                severity.to_owned(),
                code.to_owned(),
            ))
        })
        .collect()
}
