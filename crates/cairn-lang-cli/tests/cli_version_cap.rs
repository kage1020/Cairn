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
        // A leftover from an interrupted run would make
        // `a_refused_target_writes_nothing` read a stale artifact as a
        // fresh one, so the removal has to have happened — "it was not
        // there" is the only other acceptable outcome.
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("cannot clear {}: {err}", dir.display()),
        }
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
    ///
    /// Every I/O failure panics rather than reading as "nothing there".
    /// The absence of artifacts is the observation this file exists to
    /// make, so a walk that quietly gives up folds the whole suite toward
    /// passing.
    fn artifacts(&self) -> Vec<String> {
        fn walk(dir: &Path, into: &mut Vec<String>) {
            let entries = std::fs::read_dir(dir)
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
    compile_as(fixture, "java", target)
}

fn compile_as(fixture: &Fixture, edition: &str, target: &str) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", edition, "--target", target])
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

/// Exiting non-zero while leaving a lock behind would still leave
/// `verified: true` on disk for the next reader.
///
/// The empty output directory counts. `--out` is created before the
/// structure tags are built, so a check placed even one step later than it
/// is leaves that directory behind — a file count alone cannot tell the
/// two orderings apart, and the ordering is the whole point of running
/// before any of it.
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
    assert!(
        !fixture.out().exists(),
        "a refused compile must not create its output directory either",
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

/// And it has to point at the `@requires` line, not at the file.
///
/// The floor's span is the only thing carrying that; a diagnostic anchored
/// at the start of the file reads the same on a one-line source and sends
/// a reader to the wrong place on every other one.
#[test]
fn the_refusal_points_at_the_line_that_set_the_floor() {
    let source = format!("@cairn 2026.7\n\n@requires version>=1.21\n{BUILD}");
    let fixture = Fixture::new("position", &source);
    let stderr = String::from_utf8(compile(&fixture, "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains(":3:1: this file requires"),
        "the `@requires` is the third line: {stderr}",
    );
}

/// The suggestion has to be a target that exists.
///
/// `spec/lint.md` §11.2 makes the candidates valid in the target part of
/// the message. Echoing the floor back as `--target >=99.0` sends the
/// author to an unsupported-target error, which is a second failure and no
/// closer to a build.
#[test]
fn an_unsatisfiable_floor_says_so_instead_of_suggesting_a_target() {
    let fixture = Fixture::new("unsat_msg", &format!("@requires version>=99.0\n{BUILD}"));
    let stderr = String::from_utf8(compile(&fixture, "1.21.4").stderr).expect("utf-8");
    assert!(
        stderr.contains("no supported java target satisfies it"),
        "should say the floor cannot be met: {stderr}",
    );
    assert!(
        stderr.contains("1.20.4, 1.21, 1.21.4"),
        "should list what the pack does support: {stderr}",
    );
    assert!(
        !stderr.contains("--target 99.0") && !stderr.contains("--target >=99.0"),
        "must not suggest a target that does not exist: {stderr}",
    );
}

/// A satisfiable floor names one that works, and only ones that work.
#[test]
fn a_satisfiable_floor_names_a_target_that_meets_it() {
    let fixture = Fixture::new("sat_msg", &format!("@requires version>=1.21.4\n{BUILD}"));
    let stderr = String::from_utf8(compile(&fixture, "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains("--target 1.21.4"),
        "should suggest the target that meets the floor: {stderr}",
    );
    // 1.20.4 and 1.21 are both below the floor, so neither may be offered.
    assert!(
        !stderr.contains("valid java targets: 1.20.4"),
        "must not offer a target below the floor: {stderr}",
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

/// The same, on a file that *does* declare a floor the target misses.
///
/// The test above cannot see the ordering it is named for: with only a
/// malformed line the module has no floor at all, so the enforcement
/// returns before it compares anything and `E_VERSION_CAP` is absent for a
/// reason that has nothing to do with which check runs first. Adding a
/// well-formed line below makes the cap genuinely available, and the file
/// still has to hear about the mistake it can act on.
#[test]
fn a_malformed_requirement_is_reported_before_a_cap_it_would_also_trigger() {
    let source = format!("@requires version<1.20\n@requires version>=1.21\n{BUILD}");
    let fixture = Fixture::new("malformed_and_capped", &source);
    let out = compile(&fixture, "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_INVALID_REQUIRES"),
        "the mistake in the file comes first: {stderr}",
    );
    assert!(
        !stderr.contains("E_VERSION_CAP"),
        "the target is a consequence, not the thing to fix: {stderr}",
    );
}

/// Two lines naming the same version leave the maximum undecided, and the
/// first one wins, so the diagnostic does not move when an equivalent line
/// is appended below it.
#[test]
fn equal_floors_report_at_the_first_of_them() {
    let source = format!("@requires version>=1.21\n@requires version>=1.21.0\n{BUILD}");
    let fixture = Fixture::new("equal_floors", &source);
    let stderr = String::from_utf8(compile(&fixture, "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains(":1:1: this file requires"),
        "the first of two equal floors owns the diagnostic: {stderr}",
    );
}

/// `latest` is the one target whose version the source does not choose, so
/// it is the one where a floor and a target can drift apart without anyone
/// editing either.
#[test]
fn latest_is_held_to_the_floor_like_any_other_target() {
    let satisfiable = Fixture::new("latest_ok", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile(&satisfiable, "latest");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let unreachable = Fixture::new("latest_cap", &format!("@requires version>=99.0\n{BUILD}"));
    let out = compile(&unreachable, "latest");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("target 1.21.4"),
        "the refusal should name the version `latest` resolved to: {stderr}",
    );
}

// -- the two editions ------------------------------------------------------
//
// Everything above compiles Java. Bedrock numbers its releases differently
// — `1.21.0 / 1.21.40 / 1.21.60` against Java's `1.20.4 / 1.21 / 1.21.4` —
// and a floor carries no edition, so the comparison runs across two
// unrelated schemes.

/// The comparison works where the two schemes happen to agree.
#[test]
fn a_bedrock_target_is_held_to_the_floor_too() {
    let fixture = Fixture::new("bedrock_cap", &format!("@requires version>=99.0\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(stderr.contains("E_VERSION_CAP"), "{stderr}");
    assert!(
        stderr.contains("no supported bedrock target satisfies it"),
        "the candidates have to be the target edition's: {stderr}",
    );
    // The versions, not only the word: naming the edition while listing
    // the other one's releases is worse than listing neither, because it
    // reads as a closed set the author can choose from.
    assert!(
        stderr.contains("1.21.0, 1.21.40, 1.21.60"),
        "should list Bedrock's releases: {stderr}",
    );
    assert!(
        !stderr.contains("1.20.4"),
        "1.20.4 is a Java release and does not exist here: {stderr}",
    );
}

/// The trailing-zero rule decides a real build here, not just a unit
/// assertion: Bedrock's earliest supported release is `1.21.0`, and a floor
/// of `1.21` is the same version by the comparison's own convention.
#[test]
fn a_bedrock_target_equal_to_the_floor_but_for_a_trailing_zero_compiles() {
    let fixture = Fixture::new("bedrock_zero", &format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.0");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A floor written in Java's numbering is not evaluated against Bedrock's,
/// and this is where that shows.
///
/// Bedrock `1.21.40` is two releases *below* the edition's latest, while
/// Java `1.21.4` is its latest — different scales entirely. The comparison
/// sees `40 > 4` and lets the build through, so a file declaring
/// `version>=1.21.4` compiles to a Bedrock structure and a lock reading
/// `verified: true`.
///
/// Pinned rather than left implicit: this is the same shape of defect the
/// floor enforcement exists to remove, one edition to the left, and it
/// cannot be fixed without deciding whether `@requires` is edition-neutral
/// — a language question, not an oversight. When that is settled, this test
/// fails, which is the signal to replace it with the real assertion.
#[test]
fn a_java_shaped_floor_is_not_evaluated_against_bedrock_numbering() {
    let fixture = Fixture::new("cross", &format!("@requires version>=1.21.4\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    assert!(
        out.status.success(),
        "the cross-edition comparison started refusing this; the divergence \
         this pins is gone: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    // And the lock says the build was verified, which is what makes the
    // divergence worth pinning rather than merely noting.
    let lock = std::fs::read_to_string(fixture.lock()).expect("a lock was written");
    assert!(lock.contains("verified: true"), "{lock}");
}
