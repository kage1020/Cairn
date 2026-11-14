//! The `inferred range` row: per edition, the versions whose block tables declare every id the build
//! paints, per `spec/versioning-editions` "The `inferred range` row".

use crate::common::{Fixture, Run, assert_stream, cairn_argv};

/// A struct whose floor and walls paint the theme's two slots.
fn painting(floor: &str, wall: &str) -> String {
    format!(
        "theme t:\n  slot floor -> @{floor}\n  slot wall -> @{wall}\n\nstruct s size=3x3\n  floor \
         mat_slot=floor\n  walls mat_slot=wall height=2\n"
    )
}

/// `cairn info` of the fixture's source for `editions`, with `extra` arguments.
fn info(fixture: &Fixture, editions: &str, extra: &[&str]) -> Run {
    let source = fixture.source();
    let mut argv = vec![
        "info",
        source.to_str().expect("utf-8 path"),
        "--editions",
        editions,
    ];
    argv.extend_from_slice(extra);
    Run::from(cairn_argv(&argv))
}

/// The text after the `inferred range:` label.
fn inferred_row(run: &Run) -> &str {
    let rows: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("inferred range:          "))
        .collect();
    let [row] = rows.as_slice() else {
        panic!("one `inferred range` row expected, got: {}", run.stdout);
    };
    row
}

/// The `note:` lines about the inferred range, with the fixture's path cut to `s.crn`.
fn range_notes(fixture: &Fixture, run: &Run) -> Vec<String> {
    let path = fixture.source().display().to_string();
    run.stderr
        .lines()
        .filter(|line| line.contains("inferred range"))
        .map(|line| line.replace(&path, "s.crn"))
        .collect()
}

#[test]
fn a_build_every_carried_version_declares_has_an_open_range() {
    let fixture = Fixture::new(&painting("cobblestone", "oak_planks"));
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_row(&run),
        "Java: 0.0 .. latest   Bedrock: 0.0 .. latest"
    );
    assert_eq!(range_notes(&fixture, &run), Vec::<String>::new());
}

#[test]
fn an_id_a_later_version_adds_starts_the_range_there() {
    let fixture = Fixture::new(&painting("pale_moss_block", "cobblestone"));
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_row(&run),
        "Java: 1.21.4 .. latest   Bedrock: 1.21.60 .. latest"
    );
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: java's inferred range starts at 1.21.4, the first java target \
             that declares `minecraft:pale_moss_block`",
            "note: s.crn:2:17: bedrock's inferred range starts at 1.21.60, the first bedrock \
             target that declares `minecraft:pale_moss_block`",
        ],
    );
}

/// Bedrock 1.21.40 splits `stonebrick` four ways, so no later version writes it.
#[test]
fn an_id_a_later_version_drops_ends_the_range_there() {
    let fixture = Fixture::new(&painting("cobblestone", "stonebrick"));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Bedrock: 0.0 .. 1.21.0");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: bedrock's inferred range ends at 1.21.0, the last bedrock target \
             that declares `minecraft:stonebrick`"
        ],
    );
}

/// Java 26.3 and Bedrock 1.26.60 spell `chain` `iron_chain`.
#[test]
fn a_renamed_id_bounds_no_range() {
    let fixture = Fixture::new(&painting("chain", "cobblestone"));
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_row(&run),
        "Java: 0.0 .. latest   Bedrock: 0.0 .. latest"
    );
    assert_eq!(range_notes(&fixture, &run), Vec::<String>::new());
}

/// Java adds `pale_moss_block` at 1.21.4 and `leaf_litter` at 26.3.
#[test]
fn the_latest_start_bounds_the_range() {
    let fixture = Fixture::new(&painting("pale_moss_block", "leaf_litter"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(inferred_row(&run), "Java: 26.3 .. latest");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: java's inferred range starts at 26.3, the first java target that \
          declares `minecraft:leaf_litter`"
        ],
    );
}

#[test]
fn edges_that_cross_leave_no_range() {
    let fixture = Fixture::new(&painting("pale_moss_block", "stonebrick"));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Bedrock: none");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: bedrock's inferred range starts at 1.21.60, the first bedrock \
             target that declares `minecraft:pale_moss_block`",
            "note: s.crn:3:16: bedrock's inferred range ends at 1.21.0, the last bedrock target \
             that declares `minecraft:stonebrick`",
            "note: bedrock has no inferred range: it would start at 1.21.60 and end at 1.21.0",
        ],
    );
}

#[test]
fn an_id_no_carried_version_declares_leaves_no_range() {
    let fixture = Fixture::new(&painting("cobblestone", "no_such_block"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Java: none");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: java has no inferred range: no java target declares \
          `minecraft:no_such_block`"
        ],
    );
}

#[test]
fn an_id_painted_twice_is_named_once_where_it_is_first_painted() {
    let fixture = Fixture::new(&painting("pale_moss_block", "pale_moss_block"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(inferred_row(&run), "Java: 1.21.4 .. latest");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: java's inferred range starts at 1.21.4, the first java target that \
          declares `minecraft:pale_moss_block`"
        ],
    );
}

/// Bedrock's catalog maps `@floor.stone.smooth` onto `stone_bricks`, and its state map writes
/// `@stone_bricks` at 1.21.0 as `stonebrick`.
#[test]
fn a_catalog_or_canonical_token_counts_as_each_pin_spells_it() {
    let fixture = Fixture::new(&painting("floor.stone.smooth", "cobblestone"));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Bedrock: 0.0 .. latest");
    assert_eq!(range_notes(&fixture, &run), Vec::<String>::new());

    let authored = Fixture::new(&painting("stone_bricks", "cobblestone"));
    let run = info(&authored, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Bedrock: 0.0 .. latest");
    assert_eq!(range_notes(&authored, &run), Vec::<String>::new());
}

#[test]
fn a_floor_below_the_range_is_no_conflict() {
    let fixture = Fixture::new(&format!(
        "@requires version>=1.20.4\n{}",
        painting("pale_moss_block", "cobblestone")
    ));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stdout
            .starts_with("registry compatibility:  1.20.4 .. latest\ninferred range:          Java: 1.21.4 .. latest\n"),
        "the declared row and the inferred row side by side: {}",
        run.stdout,
    );
    assert_stream(
        &run.stderr,
        &[
            (3, 17, "error", "E_UNKNOWN_ID"),
            (3, 17, "error", "E_UNKNOWN_ID"),
        ],
    );
}

#[test]
fn a_floor_above_the_range_is_reported_and_the_report_completes() {
    let fixture = Fixture::new(&format!(
        "@requires bedrock version>=1.21.40\n{}",
        painting("cobblestone", "stonebrick")
    ));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(
        run.code,
        Some(0),
        "a report is not a refusal: {}",
        run.stderr
    );
    assert_eq!(inferred_row(&run), "Bedrock: 0.0 .. 1.21.0");
    assert_stream(
        &run.stderr,
        &[
            (4, 16, "error", "E_UNKNOWN_ID"),
            (4, 16, "error", "E_UNKNOWN_ID"),
            (4, 16, "error", "E_UNKNOWN_ID"),
            (1, 1, "error", "E_REQUIRES_CONFLICT"),
        ],
    );
}

#[test]
fn json_carries_each_editions_range_with_its_ids_and_conflicts() {
    let fixture = Fixture::new(&format!(
        "@requires bedrock version>=1.21.40\n{}",
        painting("pale_moss_block", "stonebrick")
    ));
    let run = info(&fixture, "java,bedrock", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["inferred_range"],
        serde_json::json!([
            {
                "edition": "java",
                "min": "1.21.4",
                "max": "latest",
                "empty": true,
                "min_ids": [{ "id": "minecraft:pale_moss_block", "line": 3, "col": 17 }],
                "undeclared": [{ "id": "minecraft:stonebrick", "line": 4, "col": 16 }],
            },
            {
                "edition": "bedrock",
                "min": "1.21.60",
                "max": "1.21.0",
                "empty": true,
                "min_ids": [{ "id": "minecraft:pale_moss_block", "line": 3, "col": 17 }],
                "max_ids": [{ "id": "minecraft:stonebrick", "line": 4, "col": 16 }],
                "conflicts": [{
                    "floor": { "declared": "bedrock version>=1.21.40", "line": 1, "col": 1 },
                    "ids": [{
                        "id": "minecraft:stonebrick",
                        "last_declared": "1.21.0",
                        "line": 4,
                        "col": 16,
                    }],
                }],
            },
        ]),
    );
}

#[test]
fn json_marks_a_range_no_version_can_lie_in_as_empty() {
    let fixture = Fixture::new(&painting("cobblestone", "no_such_block"));
    let run = info(&fixture, "java", &["--format", "json"]);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["inferred_range"],
        serde_json::json!([{
            "edition": "java",
            "min": "0.0",
            "max": "latest",
            "empty": true,
            "undeclared": [{ "id": "minecraft:no_such_block", "line": 3, "col": 16 }],
        }]),
    );
}
