//! `cairn compile` against a floor above the last target that declares a block the build paints:
//! `E_REQUIRES_CONFLICT`, per `spec/versioning-editions` "A floor is weighed against the registry".

use crate::common::{CHAIN_UNDER_THREE_FLOORS, Fixture, Run, assert_stream, compile_as};

/// Paints `@chain`, which Java declares last at 1.21.4 and Bedrock last at 1.21.60.
const CHAIN: &str = "theme t:\n  slot wall -> @chain\n\nstruct s size=2x2\n  floor mat_slot=wall\n";

fn compiled(source: &str, edition: &str, target: &str) -> (Fixture, Run) {
    let fixture = Fixture::new(source);
    let run = Run::from(compile_as(&fixture, edition, target));
    (fixture, run)
}

#[test]
fn a_floor_above_the_last_target_declaring_a_painted_id_is_refused_whatever_the_target() {
    let source = format!("@requires java version>=26.3\n{CHAIN}");
    let (fixture, at_floor) = compiled(&source, "java", "26.3");
    assert_eq!(at_floor.code, Some(1), "{}", at_floor.stderr);
    assert_stream(
        &at_floor.stderr,
        &[
            (3, 16, "error", "E_UNKNOWN_ID"),
            (1, 1, "error", "E_REQUIRES_CONFLICT"),
        ],
    );
    assert_eq!(fixture.artifacts(), Vec::<String>::new());

    let (fixture, below) = compiled(&source, "java", "1.21.4");
    assert_eq!(below.code, Some(1), "{}", below.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        below.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires java version>=26.3, but \
             no java target it admits declares `minecraft:chain`, which this build paints.\n  \
             note: {path}:3:16: `minecraft:chain` is last declared at java 1.21.4\n  fix: lower \
             the `@requires` floor to 1.21.4 or below, or replace `minecraft:chain` with a block \
             a java target at or above 26.3 declares\n",
        ),
        "the conflict stops the build ahead of `E_VERSION_CAP`, whose fix it would contradict",
    );
    assert_eq!(fixture.artifacts(), Vec::<String>::new());
}

#[test]
fn a_floor_a_part_declares_names_the_part_and_its_own_line() {
    let source = "theme t:\n  requires version>=26.3\n  slot wall -> @chain\n\nstruct s size=2x2\n  \
                  floor mat_slot=wall\n";
    let (fixture, run) = compiled(source, "java", "1.21.4");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:2:3: `theme t` requires version>=26.3, but no \
             java target it admits declares `minecraft:chain`, which this build paints.\n  \
             `theme t` declares it, and every scope that binds the theme inherits it\n  note: \
             {path}:3:16: `minecraft:chain` is last declared at java 1.21.4\n  fix: lower the \
             `requires` floor to 1.21.4 or below, or replace `minecraft:chain` with a block a java \
             target at or above 26.3 declares\n",
        ),
    );
}

#[test]
fn a_part_placed_twice_conflicts_once_and_names_its_block_once() {
    let source = "theme t:\n  slot floor -> @chain\n\ndef cottage size=2x2:\n  requires java \
                  version>=26.3\n  floor mat_slot=floor\n\nsite hamlet:\n  place id=a \
                  use=cottage theme=t at=origin\n  place id=b use=cottage theme=t east_of=a\n";
    let (fixture, run) = compiled(source, "java", "1.21.4");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:5:3: `def cottage` requires java version>=26.3, but \
             no java target it admits declares `minecraft:chain`, which this build paints.\n  \
             `def cottage` declares it, and every `place use=cottage` inherits it\n  note: \
             {path}:2:17: `minecraft:chain` is last declared at java 1.21.4\n  fix: lower the \
             `requires` floor to 1.21.4 or below, or replace `minecraft:chain` with a block a java \
             target at or above 26.3 declares\n",
        ),
        "one finding for the part, naming the id both placements paint once",
    );
}

#[test]
fn each_floor_above_the_range_is_one_finding_in_source_order() {
    let (fixture, run) = compiled(CHAIN_UNDER_THREE_FLOORS, "java", "1.21.4");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:2:1: this file requires java version>=26.3, but \
             no java target it admits declares `minecraft:chain`, which this build paints.\n  \
             note: {path}:5:16: `minecraft:chain` is last declared at java 1.21.4\n  fix: lower \
             the `@requires` floor to 1.21.4 or below, or replace `minecraft:chain` with a block \
             a java target at or above 26.3 declares\n\
             error[E_REQUIRES_CONFLICT]: {path}:8:3: `def cottage` requires java version>=26.3, \
             but no java target it admits declares `minecraft:chain`, which this build paints.\n  \
             `def cottage` declares it, and every `place use=cottage` inherits it\n  note: \
             {path}:5:16: `minecraft:chain` is last declared at java 1.21.4\n  fix: lower the \
             `requires` floor to 1.21.4 or below, or replace `minecraft:chain` with a block a java \
             target at or above 26.3 declares\n",
        ),
        "the floor at or below 1.21.4 is no finding",
    );
}

#[test]
fn the_id_last_declared_earliest_bounds_the_conflict() {
    let source = "@requires bedrock version>=1.26.60\n\ntheme t:\n  slot wall -> @chain\n  slot \
                  floor -> @stonebrick\n\nstruct s size=3x3\n  floor mat_slot=floor\n  walls \
                  mat_slot=wall height=2\n";
    let (fixture, run) = compiled(source, "bedrock", "1.21.60");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires bedrock version>=1.26.60, \
             but no bedrock target it admits declares `minecraft:stonebrick`, which this build \
             paints.\n  note: {path}:5:17: `minecraft:stonebrick` is last declared at bedrock \
             1.21.0\n  note: {path}:4:16: `minecraft:chain` is last declared at bedrock 1.21.60\n  \
             fix: lower the `@requires` floor to 1.21.0 or below, or replace \
             `minecraft:stonebrick`, `minecraft:chain` with blocks a bedrock target at or above \
             1.26.60 declares\n",
        ),
    );
}

#[test]
fn a_floor_between_two_last_declared_versions_names_only_the_ids_below_it() {
    let source = "@requires bedrock version>=1.21.40\n\ntheme t:\n  slot wall -> @chain\n  slot \
                  floor -> @stonebrick\n\nstruct s size=3x3\n  floor mat_slot=floor\n  walls \
                  mat_slot=wall height=2\n";
    let (fixture, run) = compiled(source, "bedrock", "1.21.0");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires bedrock version>=1.21.40, \
             but no bedrock target it admits declares `minecraft:stonebrick`, which this build \
             paints.\n  note: {path}:5:17: `minecraft:stonebrick` is last declared at bedrock \
             1.21.0\n  fix: lower the `@requires` floor to 1.21.0 or below, or replace \
             `minecraft:stonebrick` with a block a bedrock target at or above 1.21.40 declares\n",
        ),
        "`minecraft:chain`, last declared at 1.21.60, is at or above the floor",
    );
}

#[test]
fn a_floor_at_or_below_the_last_target_declaring_a_painted_id_is_no_conflict() {
    for floor in ["1.21.4", "1.21"] {
        let source = format!("@requires version>={floor}\n{CHAIN}");
        let (fixture, run) = compiled(&source, "java", "1.21.4");
        assert_eq!(run.code, Some(0), "{floor}: {}", run.stderr);
        assert_eq!(run.stderr, "", "{floor}");
        assert_eq!(fixture.artifacts(), ["s.crn.lock", "s.nbt"], "{floor}");
    }
}

#[test]
fn a_floor_no_carried_version_meets_is_left_to_e_version_cap() {
    let source = format!("@requires version>=26.3\n{CHAIN}");
    let (fixture, run) = compiled(&source, "bedrock", "1.21.60");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_VERSION_CAP]: {path}:1:1: this file requires version>=26.3 (target \
             1.21.60).\n  no supported bedrock target satisfies it: 1.21.0, 1.21.40, 1.21.60, \
             1.26.60, latest\n  fix: lower the `@requires` floor, or build against another \
             edition\n",
        ),
        "no bedrock target meets the floor, whatever the build paints",
    );
}

#[test]
fn an_unorderable_floor_is_left_to_its_own_code() {
    let source = format!("@requires version>=1.21.4\n{CHAIN}");
    let (_, run) = compiled(&source, "bedrock", "1.21.60");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_stream(&run.stderr, &[(1, 1, "error", "E_REQUIRES_UNORDERABLE")]);
}

#[test]
fn a_floor_below_the_first_target_declaring_a_painted_id_is_no_conflict() {
    let source = "@requires version>=1.20.4\n\ntheme t:\n  slot floor -> @pale_moss_block\n\n\
                  struct s size=2x2\n  floor mat_slot=floor\n";
    let (fixture, run) = compiled(source, "java", "1.21.4");
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_stream(&run.stderr, &[]);
    assert_eq!(fixture.artifacts(), ["s.crn.lock", "s.nbt"]);
}

#[test]
fn a_floor_scoped_to_the_other_edition_weighs_nothing() {
    let source = format!("@requires java version>=26.3\n{CHAIN}");
    let (_, run) = compiled(&source, "bedrock", "1.26.60");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_stream(&run.stderr, &[(3, 16, "error", "E_UNKNOWN_ID")]);
}

#[test]
fn an_id_no_target_declares_bounds_no_floor() {
    let source = "@requires version>=1.21\n\ntheme t:\n  slot wall -> @no_such_block\n\nstruct s \
                  size=2x2\n  floor mat_slot=wall\n";
    let (_, run) = compiled(source, "java", "26.3");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_stream(&run.stderr, &[(4, 16, "error", "E_UNKNOWN_ID")]);
}
