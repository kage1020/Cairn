//! `cairn compile --target` against the version floors a source declares:
//! a target a floor rules out, or one the floor's numbering cannot place,
//! is refused before anything is written, since a lock recording a target
//! the source disowns certifies a build nobody checked. The refusal names
//! the floor that refused and a repair that builds.

use std::process::Command;

use crate::common::{Fixture, assert_stream, cargo_bin, compile_as};

/// A struct with one painted floor, so the compile has real work to do and
/// a failure cannot be mistaken for an empty build.
const BUILD: &str =
    "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";

#[test]
fn a_target_below_the_declared_floor_is_refused() {
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
}

/// `cairn check --target` is deliberately not held to the floors: a floor
/// bounds what a *lockfile* may certify, and a check writes none.
/// `enforce_version_floor`'s single caller is `run_compile`.
#[test]
fn a_pinned_check_is_not_held_to_the_floor_the_compile_is_held_to() {
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let refused = compile_as(&fixture, "java", "1.20.4");
    let stderr = String::from_utf8(refused.stderr).expect("utf-8");
    assert_eq!(refused.status.code(), Some(1), "premise: {stderr}");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);

    let checked = Command::new(cargo_bin())
        .args([
            "check",
            fixture.source().to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.20.4",
        ])
        .output()
        .expect("run cairn");
    let stderr = String::from_utf8(checked.stderr).expect("utf-8");
    assert_eq!(checked.status.code(), Some(0), "stderr={stderr}");
    assert_stream(&stderr, &[]);
}

/// The floor, the target that missed it, and the fix: the shape
/// `spec/versioning-editions` "Fail-loud and minimum-version inference" uses
/// for this code.
#[test]
fn the_refusal_names_the_floor_the_target_and_the_fix() {
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.20.4").stderr).expect("utf-8");
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

/// On line 3, since a diagnostic anchored at the start of the file would
/// pass on a one-line source.
#[test]
fn the_refusal_points_at_the_line_that_set_the_floor() {
    let source = format!("@cairn 2026.7\n\n@requires version>=1.21\n{BUILD}");
    let fixture = Fixture::new(&source);
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains(":3:1: this file requires"),
        "the `@requires` is the third line: {stderr}",
    );
}

/// `spec/lint` "Machine-readable payload" makes the candidates valid in the
/// target part of the message; `--target >=99.0` would only lead to an
/// unsupported-target error.
#[test]
fn an_unsatisfiable_floor_says_so_instead_of_suggesting_a_target() {
    let fixture = Fixture::new(&format!("@requires version>=99.0\n{BUILD}"));
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.21.4").stderr).expect("utf-8");
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
    let fixture = Fixture::new(&format!("@requires version>=1.21.4\n{BUILD}"));
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.20.4").stderr).expect("utf-8");
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
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(fixture.lock().exists(), "a passing compile writes its lock");
}

#[test]
fn a_target_above_the_floor_compiles() {
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
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
    let fixture = Fixture::new(&format!("@requires version>=99.0\n{BUILD}"));
    for target in ["1.20.4", "1.21", "1.21.4", "latest"] {
        let out = compile_as(&fixture, "java", target);
        assert_eq!(out.status.code(), Some(1), "{target}");
    }
}

#[test]
fn a_source_with_no_requirement_compiles_against_any_target() {
    let fixture = Fixture::new(BUILD);
    for target in ["1.20.4", "1.21", "1.21.4"] {
        let out = compile_as(&fixture, "java", target);
        assert!(
            out.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// A malformed requirement declares no floor, so it must not surface as
/// `E_VERSION_CAP`, which would blame the target; it still stops the
/// compile.
#[test]
fn a_malformed_requirement_is_reported_as_itself_not_as_a_cap() {
    let fixture = Fixture::new(&format!("@requires version<1.20\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_INVALID_REQUIRES")]);
}

/// The same, on a file that *does* declare a floor the target misses: with
/// only a malformed line there is no floor, so the test above cannot see
/// which check runs first.
#[test]
fn a_malformed_requirement_is_reported_before_a_cap_it_would_also_trigger() {
    let source = format!("@requires version<1.20\n@requires version>=1.21\n{BUILD}");
    let fixture = Fixture::new(&source);
    let out = compile_as(&fixture, "java", "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_INVALID_REQUIRES")]);
}

/// Two lines naming the same version leave the maximum undecided, and the
/// first one wins, so the diagnostic does not move when an equivalent line
/// is appended below it.
#[test]
fn equal_floors_report_at_the_first_of_them() {
    let source = format!("@requires version>=1.21\n@requires version>=1.21.0\n{BUILD}");
    let fixture = Fixture::new(&source);
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.20.4").stderr).expect("utf-8");
    assert!(
        stderr.contains(":1:1: this file requires"),
        "the first of two equal floors owns the diagnostic: {stderr}",
    );
}

/// Floors compose by intersection, so the offered target has to clear the
/// floor that is not reported too: `1.21` clears only the first here.
#[test]
fn the_offered_target_clears_every_floor_not_only_the_reported_one() {
    let source = format!("@requires version>=1.21\n@requires version>=1.21.4\n{BUILD}");
    let fixture = Fixture::new(&source);
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.20.4").stderr).expect("utf-8");
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
    let out = compile_as(&fixture, "java", "1.21.4");
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
    let satisfiable = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&satisfiable, "java", "latest");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let unreachable = Fixture::new(&format!("@requires version>=99.0\n{BUILD}"));
    let out = compile_as(&unreachable, "java", "latest");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("target 1.21.4"),
        "the refusal should name the version `latest` resolved to: {stderr}",
    );
}

// -- the two editions ------------------------------------------------------
//
// Bedrock numbers its releases `1.21.0 / 1.21.40 / 1.21.60` against Java's
// `1.20.4 / 1.21 / 1.21.4`, and an unscoped floor carries no edition.

/// The comparison works where the two schemes happen to agree.
#[test]
fn a_bedrock_target_is_held_to_the_floor_too() {
    let fixture = Fixture::new(&format!("@requires version>=99.0\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
    assert!(
        stderr.contains("no supported bedrock target satisfies it"),
        "the candidates have to be the target edition's: {stderr}",
    );
    // The other edition's releases would read as a closed set to choose
    // from.
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
    let fixture = Fixture::new(&format!("@requires version>=1.21\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.0");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A floor written in Java's numbering is not evaluated against Bedrock's.
///
/// Bedrock numbers its patch releases in tens, so Java's `1.21.4` names no
/// Bedrock release; compared as dotted decimals, `40 > 4` would lock
/// `verified: true` for a target the source rules out. It falls between two
/// Bedrock rows, so it has no `DataVersion` to order by either.
#[test]
fn a_java_shaped_floor_is_not_evaluated_against_bedrock_numbering() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.4\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    // Not a cap: `--target` is not the thing to change.
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a refused compile must leave no artifact and no lock",
    );
}

/// The repair is on the `@requires` line: name the edition the floor is
/// written in, or a release of the one being built. The advice is followed
/// to a build, not only matched as text.
#[test]
fn the_unorderable_refusal_offers_a_scope_that_works() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.4\n{BUILD}"));
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
    // The two releases it falls between, not the whole table.
    assert!(
        stderr.contains("1.21.0 and then 1.21.20"),
        "should name the releases it falls between: {stderr}",
    );

    let repaired = Fixture::new(&format!("@requires java version>=1.21.4\n{BUILD}"));
    let out = compile_as(&repaired, "bedrock", "1.21.40");
    assert!(
        out.status.success(),
        "the offered repair has to build: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The scope is only offered when the *other* edition names the label:
/// scoped to an edition that does not name it either, the floor goes inert
/// there and the build locks `verified: true`.
#[test]
fn a_label_no_edition_can_place_is_not_answered_with_a_scope() {
    let fixture = Fixture::new(&format!("@requires version>=24w14a\n{BUILD}"));
    let stderr =
        String::from_utf8(compile_as(&fixture, "bedrock", "1.21.60").stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert!(
        !stderr.contains("@requires java"),
        "no edition can place a snapshot label, so none may be recommended: {stderr}",
    );
    assert!(
        stderr.contains("fix: name a bedrock release"),
        "the one repair left has to be named: {stderr}",
    );
}

/// A Java build refusing a floor the Bedrock table orders without naming it,
/// asserting no Bedrock scope is offered and the one repair left is.
///
/// Checked whole, so a refusal printed ahead of a build that goes on anyway
/// fails.
fn assert_no_bedrock_scope_is_offered(label: &str) {
    let fixture = Fixture::new(&format!("@requires version>={label}\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "`{label}`: {stderr}");
    // The label is inside Java's span and names no Java release.
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert!(
        !stderr.contains("@requires bedrock") && !stderr.contains("is a bedrock release"),
        "`{label}` names no bedrock release, so no bedrock scope may be offered: {stderr}",
    );
    assert!(
        stderr.contains("fix: name a java release"),
        "`{label}`: the one repair left has to be named: {stderr}",
    );
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "`{label}`: a refused compile must leave no artifact and no lock",
    );
}

/// A label Bedrock's table places below every row is not one of its
/// releases, so it gets no Bedrock scope.
///
/// Scoped to Bedrock, the floor would be met by every Bedrock target and
/// lost from the Java builds it constrains. The label sits inside Java's
/// span, naming no Java release, and below every Bedrock row, which holds
/// because the Bedrock table only grows upward.
#[test]
fn a_label_below_every_row_of_the_other_edition_is_not_answered_with_a_scope() {
    assert_no_bedrock_scope_is_offered("1.14.5");
}

/// A label Bedrock's table places above every row is not one of its
/// releases either, so it gets no Bedrock scope.
///
/// Scoped to Bedrock, the floor would be met by no Bedrock target and lost
/// from the Java builds. The label sits inside Java's span, naming no Java
/// release, and far above Bedrock's newest row so a pack refresh does not
/// make it one; if this starts offering a scope, check that premise first.
#[test]
fn a_label_above_every_row_of_the_other_edition_is_not_answered_with_a_scope() {
    assert_no_bedrock_scope_is_offered("1.99");
}

/// A label the other edition's table carries as a row gets the scope, and
/// the line saying whose release it is: the positive half that keeps the
/// two tests above from passing against any rewording.
///
/// The row is one Bedrock's pack orders but cannot build for, so the offer
/// covers every row the table names. The advice is taken to a build.
#[test]
fn a_label_naming_a_row_of_the_other_edition_is_offered_its_scope() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.20\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert!(
        stderr.contains("`1.21.20` is a bedrock release"),
        "should say whose release the label is: {stderr}",
    );
    assert!(
        stderr.contains("fix: `@requires bedrock version>=1.21.20`, or name a java release"),
        "should offer the bedrock scope: {stderr}",
    );

    let repaired = Fixture::new(&format!("@requires bedrock version>=1.21.20\n{BUILD}"));
    let out = compile_as(&repaired, "bedrock", "1.21.40");
    assert!(
        out.status.success(),
        "the offered repair has to build: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The pre-release of a row the other edition names gets the scope too:
/// it is placed at that row, so it names that edition's numbering just as
/// the release does.
///
/// `1.19.30-rc1` is inside Java's span and names no Java release; Bedrock
/// carries `1.19.30`. Only the repair is asserted, not the line before it,
/// which calls the label a release where it is the candidate for one.
#[test]
fn a_pre_release_of_a_row_of_the_other_edition_is_offered_its_scope() {
    let fixture = Fixture::new(&format!("@requires version>=1.19.30-rc1\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert!(
        stderr.contains("fix: `@requires bedrock version>=1.19.30-rc1`, or name a java release"),
        "should offer the bedrock scope: {stderr}",
    );

    let repaired = Fixture::new(&format!("@requires bedrock version>=1.19.30-rc1\n{BUILD}"));
    let out = compile_as(&repaired, "bedrock", "1.21.40");
    assert!(
        out.status.success(),
        "the offered repair has to build: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A floor naming a real release of the edition being built is ordered
/// against it, even when the pack cannot build for that release.
///
/// `1.21.1` is a Java release the pack ships no block table for. Ordering
/// it and building for it are different questions, and answering the first
/// with the second refuses a build the floor allows.
#[test]
fn a_floor_naming_a_release_the_pack_cannot_build_for_still_orders() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.1\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
    assert!(
        out.status.success(),
        "1.21.4 is above 1.21.1 by DataVersion: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let below = compile_as(&fixture, "java", "1.21");
    assert_eq!(below.status.code(), Some(1));
    let stderr = String::from_utf8(below.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
}

/// A floor above every release the pack can *build* for, but below ones it
/// can order against, says no supported target satisfies it — it must not
/// offer a release as a `--target` the pack would then refuse.
#[test]
fn an_orderable_but_unbuildable_release_is_not_offered_as_a_target() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.11\n{BUILD}"));
    let stderr = String::from_utf8(compile_as(&fixture, "java", "1.21.4").stderr).expect("utf-8");
    assert!(
        stderr.contains("no supported java target satisfies it"),
        "{stderr}",
    );
    assert!(
        !stderr.contains("--target 1.21.11") && !stderr.contains("--target 26."),
        "a release the pack cannot build for is not a target: {stderr}",
    );
}

/// The scope is the repair: scoped to Java, the floor says nothing about
/// Bedrock.
#[test]
fn a_java_scoped_floor_is_inert_on_a_bedrock_build() {
    let fixture = Fixture::new(&format!("@requires java version>=1.21.4\n{BUILD}"));
    let out = compile_as(&fixture, "bedrock", "1.21.0");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let java = compile_as(&fixture, "java", "1.21");
    assert_eq!(java.status.code(), Some(1));
    let stderr = String::from_utf8(java.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
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
    let fixture = Fixture::new(&format!("@requires bedrock version>=1.21.5\n{BUILD}"));
    let stderr =
        String::from_utf8(compile_as(&fixture, "bedrock", "1.21.40").stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
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
    let fixture = Fixture::new(&source);
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
/// A pre-release sorts below the release it names. Compared as dotted
/// decimals, `4-rc1` is a component that is not a number and sorts *above*
/// every number, so `1.21.4-rc1` would sit above `1.21.4` and the newest
/// supported target would miss a floor its own release candidate set.
#[test]
fn a_pre_release_floor_is_met_by_the_release_it_names() {
    let fixture = Fixture::new(&format!("@requires version>=1.21.4-rc1\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.21.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let below = compile_as(&fixture, "java", "1.21");
    assert_eq!(below.status.code(), Some(1));
    let stderr = String::from_utf8(below.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
}

/// A build placing a `def` that declares a floor is held to it, and the
/// refusal names the part: the repair for a composite floor
/// (`spec/versioning-editions` "Fail-loud and minimum-version inference")
/// is at the other end of the `place use=` that inherited it.
#[test]
fn a_floor_a_def_declares_refuses_the_target_and_names_the_def() {
    let fixture = Fixture::new(
        "theme t:\n  slot floor -> @oak_planks\n\
         \ndef cottage size=2x2:\n  requires version>=1.21.4\n  floor mat_slot=floor\n\
         \nsite hamlet:\n  place id=a use=cottage theme=t at=origin\n",
    );
    let out = compile_as(&fixture, "java", "1.21");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(5, 3, "error", "E_VERSION_CAP")]);
    assert!(
        stderr.contains("`def cottage` requires version>=1.21.4"),
        "the primary has to name the part: {stderr}",
    );
    assert!(
        stderr.contains("`def cottage` declares it, and every `place use=cottage` inherits it"),
        "the refusal has to name the route: {stderr}",
    );
    // The floor is on a `requires` line, not the directive.
    assert!(stderr.contains("or lower the `requires` floor"), "{stderr}");
    assert!(
        fixture.artifacts().is_empty(),
        "a refused build writes nothing"
    );
}

/// A floor on a bound `theme` reaches the build the same way, and says so
/// in the theme's own terms.
#[test]
fn a_floor_a_bound_theme_declares_refuses_the_target() {
    let fixture = Fixture::new(
        "theme t:\n  requires version>=1.21.4\n  slot floor -> @oak_planks\n\
         \nstruct s size=2x2\n  floor mat_slot=floor\n",
    );
    let out = compile_as(&fixture, "java", "1.21");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("`theme t` declares it, and every scope that binds the theme inherits it"),
        "{stderr}",
    );
}

/// Where a header floor and an inherited one both apply and only the
/// inherited one refuses, the refusal reports the one that refused.
///
/// Floors compose by intersection, so naming the first one sends the
/// author to edit a floor every target already clears.
#[test]
fn the_refusal_names_the_floor_that_refused_not_the_first_one() {
    let fixture = Fixture::new(
        "@requires version>=1.20.4\ntheme t:\n  slot floor -> @oak_planks\n\
         \ndef cottage size=2x2:\n  requires version>=1.21.4\n  floor mat_slot=floor\n\
         \nsite hamlet:\n  place id=a use=cottage theme=t at=origin\n",
    );
    let out = compile_as(&fixture, "java", "1.21");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("`def cottage` requires version>=1.21.4"),
        "{stderr}",
    );
    assert!(
        !stderr.contains("version>=1.20.4 (target"),
        "the floor every target clears is not the news: {stderr}",
    );
}

/// A theme bound only through a `def` no `place` names refuses nothing.
///
/// The auto-pick binds the module's sole theme to every `struct` and `def`
/// scope, and the floor fold follows it only for the `struct`. Otherwise
/// this file and the one below — identical but for which part carries the
/// floor, neither building a voxel — would disagree about the same target.
#[test]
fn a_theme_bound_only_by_an_unplaced_def_refuses_nothing() {
    let fixture = Fixture::new(
        "theme t:\n  requires version>=1.21.4\n  slot floor -> @oak_planks\n\
         \ndef cottage size=2x2:\n  floor mat_slot=floor\n",
    );
    let out = compile_as(&fixture, "java", "1.20.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A `def` the file never places is not part of the build, so its floor
/// refuses nothing — the same reading that makes `W_UNUSED_DEF` a warning
/// rather than an error.
#[test]
fn a_floor_on_an_unplaced_def_refuses_nothing() {
    let fixture = Fixture::new(&format!(
        "def spare size=2x2:\n  requires version>=1.21.4\n  floor mat_slot=floor\n\n{BUILD}"
    ));
    let out = compile_as(&fixture, "java", "1.20.4");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A struct painted with pale moss under `floors`, one directive per line.
///
/// Of the pack's Java tables, 1.21.4's is the first to declare pale moss,
/// so it is the block a `version>=1.21.4` floor is there for. Java 1.20.4
/// and Bedrock 1.21.40 both lack it.
fn pale_moss_under(floors: &str) -> String {
    format!(
        "{floors}theme t:\n  slot floor -> @pale_moss_block\n\
         struct s size=2x2\n  floor mat_slot=floor\n"
    )
}

/// The block a floor's version first declares is the usual reason for the
/// floor, so a target below it lacks the block too. The cap is the finding
/// to act on, so the id check is suspended for that target, and a
/// suspended run is refused and writes nothing.
///
/// That 1.20.4 lacks pale moss is the premise, supplied by
/// `a_target_that_clears_the_floor_still_checks_the_blocks_it_lacks`.
#[test]
fn a_target_below_the_floor_is_capped_rather_than_refused_for_a_block_it_lacks() {
    let fixture = Fixture::new(&pale_moss_under("@requires version>=1.21.4\n"));
    let out = compile_as(&fixture, "java", "1.20.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(1, 1, "error", "E_VERSION_CAP")]);
    // Before the build below, which writes into this same fixture.
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a suspended run is refused and writes nothing",
    );

    let built = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8(built.stderr).expect("utf-8");
    assert_eq!(
        built.status.code(),
        Some(0),
        "premise: the floor's version declares the block, stderr={stderr}",
    );
}

/// The control for the test above, and its premise: 1.20.4 has no pale
/// moss.
///
/// The id check runs only at a `--target` that resolves and that every
/// floor lets through; this one clears the only floor.
#[test]
fn a_target_that_clears_the_floor_still_checks_the_blocks_it_lacks() {
    let fixture = Fixture::new(&pale_moss_under("@requires version>=1.20.4\n"));
    let out = compile_as(&fixture, "java", "1.20.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(3, 17, "error", "E_UNKNOWN_ID")]);
}

/// A floor the edition's table cannot place refuses every target, as
/// `E_REQUIRES_UNORDERABLE`, so it suspends the id check as a cap does.
///
/// An `E_UNKNOWN_ID` for the pale moss would point away from the
/// `@requires` line, which is the repair. The second half takes the
/// refusal's advice, and its `E_UNKNOWN_ID` is the premise the first half's
/// absence rests on.
#[test]
fn an_unplaceable_floor_suspends_the_block_id_check_too() {
    let fixture = Fixture::new(&pale_moss_under("@requires version>=1.21.4\n"));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a suspended run is refused and writes nothing",
    );

    let scoped = Fixture::new(&pale_moss_under("@requires java version>=1.21.4\n"));
    let out = compile_as(&scoped, "bedrock", "1.21.40");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    // Bedrock 1.21.40 has no pale moss, and a java-scoped floor is inert on
    // a bedrock build.
    assert_stream(&stderr, &[(3, 17, "error", "E_UNKNOWN_ID")]);
}

/// Compile `floors` (Java's `1.21.4`, which Bedrock's table cannot place,
/// and Bedrock's `1.21.60`, above the target) against Bedrock 1.21.40, and
/// require the unplaceable one, on `line`, to be what is reported.
///
/// The unplaceable floor is reported ahead of a below-target one wherever
/// each sits, so both orders are run: "the first floor in source order
/// that refuses" names the cap when it comes first.
fn assert_the_unplaceable_floor_is_reported_beside_a_cap(floors: &str, line: u64) {
    let fixture = Fixture::new(&pale_moss_under(floors));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    // No cap, since raising `--target` cannot clear it, and no unknown id,
    // since the floors refuse the target.
    assert_stream(&stderr, &[(line, 1, "error", "E_REQUIRES_UNORDERABLE")]);
    assert_eq!(
        fixture.artifacts(),
        Vec::<String>::new(),
        "a suspended run is refused and writes nothing",
    );
}

#[test]
fn an_unplaceable_floor_before_a_cap_is_reported_as_unplaceable() {
    assert_the_unplaceable_floor_is_reported_beside_a_cap(
        "@requires version>=1.21.4\n@requires version>=1.21.60\n",
        1,
    );
}

#[test]
fn an_unplaceable_floor_after_a_cap_is_reported_as_unplaceable() {
    assert_the_unplaceable_floor_is_reported_beside_a_cap(
        "@requires version>=1.21.60\n@requires version>=1.21.4\n",
        2,
    );
}
