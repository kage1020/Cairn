//! `cairn compile` against a floor no target declaring every block the build paints clears:
//! `E_REQUIRES_CONFLICT`, per `spec/versioning-editions` "A floor is weighed against the registry".

use crate::common::{Fixture, Run, assert_stream, compile_as};

/// Paints `@stonebrick`, which Bedrock declares last at 1.21.0 and no later version writes.
const STONEBRICK: &str =
    "theme t:\n  slot wall -> @stonebrick\n\nstruct s size=2x2\n  floor mat_slot=wall\n";

fn compiled(source: &str, edition: &str, target: &str) -> (Fixture, Run) {
    let fixture = Fixture::new(source);
    let run = Run::from(compile_as(&fixture, edition, target));
    (fixture, run)
}

#[test]
fn a_floor_above_the_last_target_declaring_a_painted_id_is_refused_whatever_the_target() {
    let source = format!("@requires bedrock version>=1.21.40\n{STONEBRICK}");
    let (fixture, at_floor) = compiled(&source, "bedrock", "1.21.40");
    assert_eq!(at_floor.code, Some(1), "{}", at_floor.stderr);
    assert_stream(
        &at_floor.stderr,
        &[
            (3, 16, "error", "E_UNKNOWN_ID"),
            (1, 1, "error", "E_REQUIRES_CONFLICT"),
        ],
    );
    assert_eq!(fixture.artifacts(), Vec::<String>::new());

    let (fixture, below) = compiled(&source, "bedrock", "1.21.0");
    assert_eq!(below.code, Some(1), "{}", below.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        below.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires bedrock version>=1.21.40, \
             but no bedrock target it admits declares `minecraft:stonebrick`, which this build \
             paints.\n  note: {path}:3:16: `minecraft:stonebrick` is last declared at bedrock \
             1.21.0\n  fix: lower the `@requires` floor to 1.21.0 or below, or replace \
             `minecraft:stonebrick` with a block a bedrock target at or above 1.21.40 declares\n",
        ),
        "the conflict stops the build ahead of `E_VERSION_CAP`, whose fix it would contradict",
    );
    assert_eq!(fixture.artifacts(), Vec::<String>::new());
}

#[test]
fn a_floor_a_part_declares_names_the_part_and_its_own_line() {
    let source = "theme t:\n  requires version>=1.21.40\n  slot wall -> @stonebrick\n\nstruct s \
                  size=2x2\n  floor mat_slot=wall\n";
    let (fixture, run) = compiled(source, "bedrock", "1.21.0");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:2:3: `theme t` requires version>=1.21.40, but no \
             bedrock target it admits declares `minecraft:stonebrick`, which this build paints.\n  \
             `theme t` declares it, and every scope that binds the theme inherits it\n  note: \
             {path}:3:16: `minecraft:stonebrick` is last declared at bedrock 1.21.0\n  fix: lower \
             the `requires` floor to 1.21.0 or below, or replace `minecraft:stonebrick` with a \
             block a bedrock target at or above 1.21.40 declares\n",
        ),
    );
}

#[test]
fn a_part_placed_twice_conflicts_once_and_names_its_block_once() {
    let source = "theme t:\n  slot floor -> @stonebrick\n\ndef cottage size=2x2:\n  requires \
                  bedrock version>=1.21.40\n  floor mat_slot=floor\n\nsite hamlet:\n  place id=a \
                  use=cottage theme=t at=origin\n  place id=b use=cottage theme=t east_of=a\n";
    let (_, run) = compiled(source, "bedrock", "1.21.0");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_stream(&run.stderr, &[(5, 3, "error", "E_REQUIRES_CONFLICT")]);
    assert_eq!(
        run.stderr.matches("is last declared at").count(),
        1,
        "and the id both placements paint is named once: {}",
        run.stderr,
    );
}

/// Bedrock's 1.21.40 flattening drops `stonebrick` and `monster_egg` together.
#[test]
fn ids_last_declared_at_one_target_are_named_in_source_order() {
    let source = "@requires bedrock version>=1.21.40\n\ntheme t:\n  slot wall -> @monster_egg\n  \
                  slot floor -> @stonebrick\n\nstruct s size=3x3\n  floor mat_slot=floor\n  walls \
                  mat_slot=wall height=2\n";
    let (fixture, run) = compiled(source, "bedrock", "1.21.0");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let path = fixture.source().display().to_string();
    assert_eq!(
        run.stderr,
        format!(
            "error[E_REQUIRES_CONFLICT]: {path}:1:1: this file requires bedrock version>=1.21.40, \
             but no bedrock target it admits declares `minecraft:monster_egg`, which this build \
             paints.\n  note: {path}:4:16: `minecraft:monster_egg` is last declared at bedrock \
             1.21.0\n  note: {path}:5:17: `minecraft:stonebrick` is last declared at bedrock \
             1.21.0\n  fix: lower the `@requires` floor to 1.21.0 or below, or replace \
             `minecraft:monster_egg`, `minecraft:stonebrick` with blocks a bedrock target at or \
             above 1.21.40 declares\n",
        ),
    );
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

/// Java 26.3 spells `chain` `iron_chain`, so no Java target lacks the block.
#[test]
fn a_renamed_id_bounds_no_floor() {
    let source = "@requires java version>=26.3\n\ntheme t:\n  slot floor -> @chain\n\nstruct s \
                  size=2x2\n  floor mat_slot=floor\n";
    let (fixture, run) = compiled(source, "java", "26.3");
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_stream(&run.stderr, &[]);
    assert_eq!(fixture.artifacts(), ["s.crn.lock", "s.nbt"]);
}

#[test]
fn a_floor_scoped_to_the_other_edition_weighs_nothing() {
    let source = format!("@requires java version>=26.3\n{STONEBRICK}");
    let (_, run) = compiled(&source, "bedrock", "1.21.40");
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
