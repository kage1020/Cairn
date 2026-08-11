//! `cairn compile --target` against a floor the source declares.
//!
//! `@requires version>=1.21` is a claim about what the build needs. It was
//! rendered by `cairn info` and enforced nowhere, so compiling against
//! `--target 1.20.4` succeeded and wrote a lockfile saying `verified: true`
//! for a version the source itself disowns. A lock is the record of what
//! was checked; recording a target the source rules out is the one thing it
//! must not do.

use std::path::{Path, PathBuf};
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// A source plus the directory its artifacts would land in, both removed
/// when the test ends.
struct Fixture {
    /// Directory holding the source, the output, and the lock.
    dir: PathBuf,
}

impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("cairn-version-cap-{}-{label}", std::process::id()));
        // A leftover from an interrupted run would make `nothing_written`
        // read a stale artifact as a fresh one.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        std::fs::write(dir.join("s.crn"), source).expect("write source");
        Self { dir }
    }

    fn source(&self) -> PathBuf {
        self.dir.join("s.crn")
    }

    fn lock(&self) -> PathBuf {
        self.dir.join("s.crn.lock")
    }

    fn out(&self) -> PathBuf {
        self.dir.join("out")
    }

    /// Every file the compile could have produced.
    fn artifacts(&self) -> Vec<String> {
        fn walk(dir: &Path, into: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, into);
                } else if path.extension().and_then(|e| e.to_str()) != Some("crn") {
                    into.push(path.file_name().expect("named").to_string_lossy().into());
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.dir, &mut found);
        found.sort();
        found
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A struct with one painted floor, so the compile has real work to do and
/// a failure cannot be mistaken for an empty build.
const BUILD: &str =
    "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";

fn compile(fixture: &Fixture, target: &str) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", "java", "--target", target])
        .arg("--out")
        .arg(fixture.out())
        .arg("--lock")
        .arg(fixture.lock())
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn a_target_below_the_declared_floor_is_refused() {
    let fixture = Fixture::new("below", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile(&fixture, "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_VERSION_CAP"),
        "expected E_VERSION_CAP, got: {stderr}",
    );
}

/// The reported symptom. Exiting non-zero while leaving a lock behind would
/// still leave `verified: true` on disk for the next reader.
#[test]
fn a_refused_target_writes_nothing() {
    let fixture = Fixture::new("nothing", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile(&fixture, "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a refused compile must leave no artifact and no lock",
    );
}

/// The message has to carry all three of what is needed to act: the floor,
/// the target that missed it, and what to do — the shape spec §10.4 uses
/// for this code.
#[test]
fn the_refusal_names_the_floor_the_target_and_the_fix() {
    let fixture = Fixture::new("message", &format!("@requires version>=1.21\n{BUILD}"));
    let stderr = String::from_utf8(compile(&fixture, "1.20.4").stderr).expect("utf-8");
    assert!(stderr.contains("1.21"), "should name the floor: {stderr}");
    assert!(
        stderr.contains("1.20.4"),
        "should name the target: {stderr}",
    );
    assert!(
        stderr.contains("--target"),
        "should name the flag to change: {stderr}",
    );
}

#[test]
fn a_target_at_the_floor_compiles() {
    let fixture = Fixture::new("at", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile(&fixture, "1.21");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(fixture.lock().exists(), "a passing compile writes its lock");
}

#[test]
fn a_target_above_the_floor_compiles() {
    let fixture = Fixture::new("above", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile(&fixture, "1.21.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A floor above everything the registry pack knows is unsatisfiable, and
/// says so for every target rather than only the low ones.
#[test]
fn a_floor_above_every_supported_target_refuses_all_of_them() {
    let fixture = Fixture::new("unreachable", &format!("@requires version>=99.0\n{BUILD}"));
    for target in ["1.20.4", "1.21", "1.21.4", "latest"] {
        let out = compile(&fixture, target);
        assert_eq!(out.status.code(), Some(1), "{target}");
    }
}

/// The floor only applies when one is declared. Nothing about this check
/// may make an ordinary file harder to compile.
#[test]
fn a_source_with_no_requirement_compiles_against_any_target() {
    let fixture = Fixture::new("none", BUILD);
    for target in ["1.20.4", "1.21", "1.21.4"] {
        let out = compile(&fixture, target);
        assert!(
            out.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// A malformed requirement is named once, and as the thing it is.
///
/// It declares no floor, so it must not surface as `E_VERSION_CAP` — that
/// would tell the author their target is too low when the actual mistake is
/// on the `@requires` line. The compile still stops, because the
/// requirement is an error in its own right.
#[test]
fn a_malformed_requirement_is_reported_as_itself_not_as_a_cap() {
    let fixture = Fixture::new("malformed", &format!("@requires version<1.20\n{BUILD}"));
    let out = compile(&fixture, "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_INVALID_REQUIRES"),
        "expected the requirement itself to be named: {stderr}",
    );
    assert!(
        !stderr.contains("E_VERSION_CAP"),
        "a requirement that declares no floor cannot cap a target: {stderr}",
    );
}
