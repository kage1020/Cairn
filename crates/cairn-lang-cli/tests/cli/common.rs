//! Helpers the test modules share: running the binary, the examples, and temp directories.

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

/// The number of `.crn` files `examples/` ships; bump it with the next example.
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

/// Copy `examples/<name>` into a fresh temp dir, so its lockfile stays out of the repository.
pub fn example_in_tempdir(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join(name);
    fs::copy(examples_dir().join(name), &dst).expect("copy example");
    (tmp, dst)
}

/// Whether `dir` takes two names that differ only in letter case for one entry, found by a second
/// `create_new` under the other spelling; neither file is left behind.
pub fn folds_case(dir: &Path) -> bool {
    let create_new = |name: &str| {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
    };
    drop(create_new("case-oracle").expect("create the lower-case spelling"));
    let folds = match create_new("CASE-ORACLE") {
        Ok(file) => {
            drop(file);
            fs::remove_file(dir.join("CASE-ORACLE")).expect("remove the upper-case spelling");
            false
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => true,
        Err(err) => panic!("create the upper-case spelling: {err}"),
    };
    fs::remove_file(dir.join("case-oracle")).expect("remove the lower-case spelling");
    folds
}

/// Write `source` to `dir/name` and return the path.
pub fn write_source(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, source).expect("write source");
    path
}

/// Every `.crn` under `examples/`, sorted, refusing any count but [`SHIPPED_EXAMPLES`].
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

/// A source plus the directory its artifacts would land in, removed when the test ends.
pub struct Fixture {
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

    /// Every file the compile could have produced; an I/O error panics, never reads as none.
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

/// `cairn compile <src> --edition <edition> --target <target> --out <out> [--lock <lock>]`.
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

/// `cairn compile` of the fixture's source, writing into the fixture's directory.
pub fn compile_as(fixture: &Fixture, edition: &str, target: &str) -> Output {
    compile(
        &fixture.source(),
        edition,
        target,
        &fixture.out(),
        Some(&fixture.lock()),
    )
}

/// A run's exit code and both streams as text.
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

/// The gunzipped NBT inside a Java `.nbt` artifact.
pub fn nbt_body(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).expect("read artifact");
    let mut body = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut body)
        .expect("gzip decode");
    body
}

/// Whether `needle` occurs in `hay`; both writers store a block id as a string tag's bytes.
pub fn bytes_contain(hay: &[u8], needle: impl AsRef<[u8]>) -> bool {
    let needle = needle.as_ref();
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A diagnostic as an assertion compares it: line, column, severity and code, without the prose.
///
/// A finding printed without a position, such as `E_PARTIAL_BUILD`, has line 0 and column 0.
pub type Reported = (u64, u64, String, String);

/// `expected` in the shape [`stderr_stream`] and [`json_stream`] return.
pub fn reported(expected: &[(u64, u64, &str, &str)]) -> Vec<Reported> {
    expected
        .iter()
        .map(|&(line, col, severity, code)| (line, col, severity.to_owned(), code.to_owned()))
        .collect()
}

/// Diagnostics from a `--format json` array on stdout, in emission order.
pub fn json_stream(stdout: &str) -> Vec<Reported> {
    let parsed: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("stdout is not JSON: {err}\n{stdout}"));
    parsed
        .as_array()
        .unwrap_or_else(|| panic!("stdout is not a JSON array: {stdout}"))
        .iter()
        .map(|d| {
            (
                d["line"].as_u64().expect("line"),
                d["col"].as_u64().expect("col"),
                d["severity"].as_str().expect("severity").to_owned(),
                d["code"].as_str().expect("code").to_owned(),
            )
        })
        .collect()
}

/// Diagnostics parsed back out of a command's stderr, in emission order.
///
/// # Panics
///
/// On an `error[` or `warning[` line that is not a finding.
pub fn stderr_stream(stderr: &str) -> Vec<Reported> {
    stderr
        .lines()
        .filter(|line| line.contains("error[") || line.contains("warning["))
        .map(|line| {
            finding_line(line)
                .or_else(|| refusal_line(line))
                .or_else(|| unpositioned_line(line))
                .unwrap_or_else(|| panic!("cannot parse the finding {line:?}; stderr:\n{stderr}"))
        })
        .collect()
}

/// `assert_eq!` of [`stderr_stream`] against `expected`, printing the whole stderr on failure.
#[track_caller]
pub fn assert_stream(stderr: &str, expected: &[(u64, u64, &str, &str)]) {
    assert_eq!(
        stderr_stream(stderr),
        reported(expected),
        "stderr:\n{stderr}"
    );
}

/// `FILE:LINE:COL: SEVERITY[CODE]: ...`.
fn finding_line(line: &str) -> Option<Reported> {
    // A path may contain `:` (`C:\...`), so anchor on the severity marker.
    let (head, tail) = ["error[", "warning["].iter().find_map(|marker| {
        let needle = format!(": {marker}");
        line.find(&needle)
            .map(|i| (&line[..i], &line[i + ": ".len()..]))
    })?;
    let (severity, code) = severity_and_code(tail)?;
    let (head, col) = head.rsplit_once(':')?;
    let (_, line_no) = head.rsplit_once(':')?;
    Some((line_no.parse().ok()?, col.parse().ok()?, severity, code))
}

/// `SEVERITY[CODE]: FILE:LINE:COL: ...`.
fn refusal_line(line: &str) -> Option<Reported> {
    if !(line.starts_with("error[") || line.starts_with("warning[")) {
        return None;
    }
    let (severity, code) = severity_and_code(line)?;
    let (_, rest) = line.split_once("]: ")?;
    // The file path may contain `:` too: take the first `:LINE:COL: `.
    rest.match_indices(':').find_map(|(i, _)| {
        let mut parts = rest[i + 1..].splitn(3, ':');
        let line_no = parts.next()?.parse().ok()?;
        let col = parts.next()?.parse().ok()?;
        parts
            .next()?
            .starts_with(' ')
            .then(|| (line_no, col, severity.clone(), code.clone()))
    })
}

/// `SEVERITY[CODE]: ...` with no position, such as `E_PARTIAL_BUILD` naming only the file.
fn unpositioned_line(line: &str) -> Option<Reported> {
    if !(line.starts_with("error[") || line.starts_with("warning[")) {
        return None;
    }
    let (severity, code) = severity_and_code(line)?;
    Some((0, 0, severity, code))
}

/// `SEVERITY` and `CODE` out of text starting `SEVERITY[CODE]`.
fn severity_and_code(text: &str) -> Option<(String, String)> {
    let (severity, tail) = text.split_once('[')?;
    let (code, _) = tail.split_once(']')?;
    Some((severity.to_owned(), code.to_owned()))
}
