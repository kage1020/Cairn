//! `cairn compile --target` against the version floors a source declares: a target a floor rules
//! out is refused before anything is written, naming the floor and a repair.

use std::process::Command;

use crate::common::{Fixture, assert_stream, cargo_bin, compile_as};

/// A struct with one painted floor, so a failure cannot be mistaken for an empty build.
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

/// A floor bounds what a lockfile may certify, and a check writes none.
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

/// The shape `spec/versioning-editions` "Fail-loud and minimum-version inference" uses.
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

/// On line 3, since an anchor at the start of the file would pass on a one-line source.
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

#[test]
fn a_malformed_requirement_is_reported_as_itself_not_as_a_cap() {
    let fixture = Fixture::new(&format!("@requires version<1.20\n{BUILD}"));
    let out = compile_as(&fixture, "java", "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_INVALID_REQUIRES")]);
}

/// The same, on a file that also declares a floor the target misses.
#[test]
fn a_malformed_requirement_is_reported_before_a_cap_it_would_also_trigger() {
    let source = format!("@requires version<1.20\n@requires version>=1.21\n{BUILD}");
    let fixture = Fixture::new(&source);
    let out = compile_as(&fixture, "java", "1.20.4");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&stderr, &[(1, 1, "error", "E_INVALID_REQUIRES")]);
}

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

/// `1.21` clears only the first floor here.
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
// Bedrock numbers its releases `1.21.0 / 1.21.40 / 1.21.60`, Java `1.20.4 / 1.21 / 1.21.4`.

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
    assert!(
        stderr.contains("1.21.0, 1.21.40, 1.21.60"),
        "should list Bedrock's releases: {stderr}",
    );
    assert!(
        !stderr.contains("1.20.4"),
        "1.20.4 is a Java release and does not exist here: {stderr}",
    );
}

/// Bedrock's earliest supported release is `1.21.0`, the same version as a `1.21` floor.
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

/// Java's `1.21.4` names no Bedrock release; as dotted decimals, `1.21.40` would clear it.
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

/// The advice is followed to a build, not only matched as text.
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

/// A Java build refusing a floor Bedrock's table orders without naming, with no Bedrock scope.
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

/// The Bedrock table only grows upward, so the label stays below every Bedrock row.
#[test]
fn a_label_below_every_row_of_the_other_edition_is_not_answered_with_a_scope() {
    assert_no_bedrock_scope_is_offered("1.14.5");
}

/// The label is far above Bedrock's newest row, so a pack refresh does not make it one.
#[test]
fn a_label_above_every_row_of_the_other_edition_is_not_answered_with_a_scope() {
    assert_no_bedrock_scope_is_offered("1.99");
}

/// The row is one Bedrock's pack orders but cannot build for; the advice is taken to a build.
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

/// Bedrock carries `1.19.30`. Only the repair is asserted, since the line before it calls the
/// label a release.
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

/// `1.21.1` is a Java release the pack ships no block table for.
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

/// Identical to the test below but for which part carries the floor; neither builds a voxel.
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

/// A struct painted with pale moss under `floors`; Java 1.21.4 is the first to declare it.
fn pale_moss_under(floors: &str) -> String {
    format!(
        "{floors}theme t:\n  slot floor -> @pale_moss_block\n\
         struct s size=2x2\n  floor mat_slot=floor\n"
    )
}

/// That 1.20.4 lacks pale moss is pinned by the control below.
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

/// The control for the test above, and its premise: 1.20.4 has no pale moss.
#[test]
fn a_target_that_clears_the_floor_still_checks_the_blocks_it_lacks() {
    let fixture = Fixture::new(&pale_moss_under("@requires version>=1.20.4\n"));
    let out = compile_as(&fixture, "java", "1.20.4");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(3, 17, "error", "E_UNKNOWN_ID")]);
}

/// The second half takes the refusal's advice, and its `E_UNKNOWN_ID` is the first half's premise.
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
    // Bedrock 1.21.40 has no pale moss, and a Java-scoped floor is inert there.
    assert_stream(&stderr, &[(3, 17, "error", "E_UNKNOWN_ID")]);
}

/// Compile `floors` against Bedrock 1.21.40 and require the unplaceable one, on `line`, reported.
fn assert_the_unplaceable_floor_is_reported_beside_a_cap(floors: &str, line: u64) {
    let fixture = Fixture::new(&pale_moss_under(floors));
    let out = compile_as(&fixture, "bedrock", "1.21.40");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
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
