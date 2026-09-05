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

/// The target the refusal offers has to be one that builds, not one that
/// clears the floor being reported.
///
/// Floors compose by intersection, so a module may declare two and only
/// the first be reported. Offering the candidates of that one alone sends
/// the author to `--target 1.21`, where the second floor refuses them —
/// the second error the candidate list exists to prevent, one floor along.
#[test]
fn the_offered_target_clears_every_floor_not_only_the_reported_one() {
    let source = format!("@requires version>=1.21\n@requires version>=1.21.4\n{BUILD}");
    let fixture = Fixture::new("two_floors", &source);
    let stderr = String::from_utf8(compile(&fixture, "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains(":1:1:"),
        "the first floor in source order owns the diagnostic: {stderr}",
    );
    assert!(
        stderr.contains("valid java targets: 1.21.4"),
        "1.21 clears the reported floor and not the other one: {stderr}",
    );
    assert!(
        stderr.contains("fix: --target 1.21.4"),
        "and the fix has to name a target that builds: {stderr}",
    );
    // Following the advice has to end in a build, which is the assertion
    // the string match above is a proxy for.
    let out = compile(&fixture, "1.21.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
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

/// A floor written in Java's numbering is not evaluated against Bedrock's.
///
/// Bedrock `1.21.40` is two releases *below* the edition's latest, while
/// Java `1.21.4` is its latest — different scales entirely. The
/// dotted-decimal comparison this replaced saw `40 > 4` and let the build
/// through, so a file declaring `version>=1.21.4` compiled to a Bedrock
/// structure and a lock reading `verified: true` for a target the source
/// rules out.
///
/// Ordering by `DataVersion` has no answer to give here, and says so: Java's
/// newest release names no Bedrock row and sits between two that exist, so
/// it has no `DataVersion` in this edition's table and cannot be given one.
#[test]
fn a_java_shaped_floor_is_not_evaluated_against_bedrock_numbering() {
    let fixture = Fixture::new("cross", &format!("@requires version>=1.21.4\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_REQUIRES_UNORDERABLE"),
        "the floor is not a cap on this edition; it names no version of it: {stderr}",
    );
    assert!(
        !stderr.contains("E_VERSION_CAP"),
        "`--target` is not the thing to change: {stderr}",
    );
    // And nothing was written, so no lock can say `verified: true` for it.
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a refused compile must leave no artifact and no lock",
    );
}

/// The refusal has to be actionable, and the action is on the `@requires`
/// line: name the edition the floor is written in, or name a release of
/// the one being built.
///
/// The advice is followed all the way to a build rather than only matched
/// as text. A message that reads right and leads nowhere is the failure
/// mode this shape of test exists to catch.
#[test]
fn the_unorderable_refusal_offers_a_scope_that_works() {
    let fixture = Fixture::new("cross_msg", &format!("@requires version>=1.21.4\n{BUILD}"));
    let stderr =
        String::from_utf8(compile_as(&fixture, "bedrock", "1.21.40").stderr).expect("utf-8");
    assert!(
        stderr.contains("@requires java version>=1.21.4"),
        "should offer the edition scope: {stderr}",
    );
    assert!(
        stderr.contains(":1:1:"),
        "should point at the `@requires` line: {stderr}",
    );
    // The two releases it falls between, rather than the whole table: the
    // pack names every Bedrock release now, and dozens of them say no more
    // than these two do.
    assert!(
        stderr.contains("1.21.0 and then 1.21.20"),
        "should name the releases it falls between: {stderr}",
    );

    // Now take the advice, and check it builds.
    let repaired = Fixture::new(
        "cross_msg_fixed",
        &format!("@requires java version>=1.21.4\n{BUILD}"),
    );
    let out = compile_as(&repaired, "bedrock", "1.21.40");
    assert!(
        out.status.success(),
        "the offered repair has to build: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The scope is only offered when the *other* edition can place the label.
///
/// Offering it otherwise recommends a guess: scoped to an edition that
/// cannot place it either, the floor goes inert there and the constraint
/// the author wrote evaporates into a `verified: true` build.
#[test]
fn a_label_no_edition_can_place_is_not_answered_with_a_scope() {
    let fixture = Fixture::new(
        "snapshot_label",
        &format!("@requires version>=24w14a\n{BUILD}"),
    );
    let stderr =
        String::from_utf8(compile_as(&fixture, "bedrock", "1.21.60").stderr).expect("utf-8");
    assert!(stderr.contains("E_REQUIRES_UNORDERABLE"), "{stderr}");
    assert!(
        !stderr.contains("@requires java"),
        "no edition can place a snapshot label, so none may be recommended: {stderr}",
    );
    assert!(
        stderr.contains("fix: name a bedrock release"),
        "the one repair left has to be named: {stderr}",
    );
}

/// A floor naming a real release of the edition being built is ordered
/// against it, even when the pack cannot build for that release.
///
/// `1.21.1` is a Java release the pack ships no block table for. Ordering
/// it and building for it are different questions, and answering the first
/// with the second is what made this refuse a build that `canary` accepted.
#[test]
fn a_floor_naming_a_release_the_pack_cannot_build_for_still_orders() {
    let fixture = Fixture::new(
        "unbuildable_row",
        &format!("@requires version>=1.21.1\n{BUILD}"),
    );
    let out = compile(&fixture, "1.21.4");
    assert!(
        out.status.success(),
        "1.21.4 is above 1.21.1 by DataVersion: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    // And below it is still below it.
    let below = compile(&fixture, "1.21");
    assert_eq!(below.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&below.stderr).contains("E_VERSION_CAP"),
        "{}",
        String::from_utf8_lossy(&below.stderr),
    );
}

/// A floor above every release the pack can *build* for, but below ones it
/// can order against, says no supported target satisfies it — it must not
/// offer a release as a `--target` the pack would then refuse.
#[test]
fn an_orderable_but_unbuildable_release_is_not_offered_as_a_target() {
    let fixture = Fixture::new(
        "unbuildable_floor",
        &format!("@requires version>=1.21.11\n{BUILD}"),
    );
    let stderr = String::from_utf8(compile(&fixture, "1.21.4").stderr).expect("utf-8");
    assert!(
        stderr.contains("no supported java target satisfies it"),
        "{stderr}",
    );
    assert!(
        !stderr.contains("--target 1.21.11") && !stderr.contains("--target 26."),
        "a release the pack cannot build for is not a target: {stderr}",
    );
}

/// And the scope is the repair, so it has to work/// And the scope is the repair, so it has to work: the same floor, scoped
/// to Java, builds Bedrock because it says nothing about Bedrock.
#[test]
fn a_java_scoped_floor_is_inert_on_a_bedrock_build() {
    let fixture = Fixture::new(
        "scoped_java",
        &format!("@requires java version>=1.21.4\n{BUILD}"),
    );
    let out = compile_as(&fixture, "bedrock", "1.21.0");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    // And it is still enforced where it does apply.
    let java = compile(&fixture, "1.21");
    assert_eq!(java.status.code(), Some(1));
    let stderr = String::from_utf8(java.stderr).expect("utf-8");
    assert!(stderr.contains("E_VERSION_CAP"), "{stderr}");
    assert!(
        stderr.contains("java version>=1.21.4"),
        "the refusal should echo the floor as the author scoped it: {stderr}",
    );
}

/// A floor scoped to the edition being built, naming a version that
/// edition does not have, is still unorderable — and the message does not
/// then offer a scope it already carries.
#[test]
fn a_scoped_floor_naming_no_release_of_its_own_edition_is_refused_without_the_scope_advice() {
    let fixture = Fixture::new(
        "scoped_unorderable",
        &format!("@requires bedrock version>=1.21.5\n{BUILD}"),
    );
    let stderr =
        String::from_utf8(compile_as(&fixture, "bedrock", "1.21.40").stderr).expect("utf-8");
    assert!(stderr.contains("E_REQUIRES_UNORDERABLE"), "{stderr}");
    assert!(
        !stderr.contains("scope the floor"),
        "it is already scoped to the edition refusing it: {stderr}",
    );
    assert!(
        stderr.contains("fix: name a bedrock release"),
        "the one repair left has to be named: {stderr}",
    );
}

/// Two floors for two editions is the shape the scope exists to make
/// writable, and each edition is held to its own.
#[test]
fn a_floor_per_edition_builds_both() {
    let source =
        format!("@requires java version>=1.21\n@requires bedrock version>=1.21.40\n{BUILD}");
    let fixture = Fixture::new("both", &source);
    for (edition, target) in [("java", "1.21.4"), ("bedrock", "1.21.60")] {
        let out = compile_as(&fixture, edition, target);
        assert!(
            out.status.success(),
            "{edition} {target}: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
    for (edition, target) in [("java", "1.20.4"), ("bedrock", "1.21.0")] {
        let out = compile_as(&fixture, edition, target);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{edition} {target} is below that edition's floor",
        );
    }
}

/// **The ordering key is the `DataVersion`, not the label.**
///
/// A pre-release sorts below the release it names, where the
/// dotted-decimal comparison this replaced read `4-rc1` as a component
/// that is not a number and sorted it *above* every number — so
/// `1.21.4-rc1` was above `1.21.4`, and the newest supported target
/// missed a floor its own release candidate set. It was refused at the
/// directive rather than mis-ordered; now it is a floor, and it is met.
#[test]
fn a_pre_release_floor_is_met_by_the_release_it_names() {
    let fixture = Fixture::new(
        "prerelease",
        &format!("@requires version>=1.21.4-rc1\n{BUILD}"),
    );
    let out = compile(&fixture, "1.21.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    // And a target below the release it precedes still misses it.
    let below = compile(&fixture, "1.21");
    assert_eq!(below.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&below.stderr).contains("E_VERSION_CAP"),
        "{}",
        String::from_utf8_lossy(&below.stderr),
    );
}
