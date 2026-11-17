//! The `inferred range` row: per edition, the interval from the latest version that first declares
//! an id the build paints to the earliest that last declares one, per `spec/versioning-editions`
//! "The `inferred range` row".

use crate::common::{
    CHAIN_UNDER_THREE_FLOORS, Fixture, Run, WALL_LEVER, assert_stream, cairn_argv,
};

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

/// The run's stderr, with the fixture's path cut to `s.crn`.
fn stderr(fixture: &Fixture, run: &Run) -> String {
    run.stderr
        .replace(&fixture.source().display().to_string(), "s.crn")
}

/// The `note:` lines about the inferred range, with the fixture's path cut to `s.crn`.
fn range_notes(fixture: &Fixture, run: &Run) -> Vec<String> {
    stderr(fixture, run)
        .lines()
        .filter(|line| line.contains("inferred range"))
        .map(str::to_owned)
        .collect()
}

/// The `inferred_range` field of a `--format json` run.
fn inferred_json(run: &Run) -> serde_json::Value {
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    report["inferred_range"].clone()
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

#[test]
fn an_id_a_later_version_drops_ends_the_range_there() {
    let fixture = Fixture::new(&painting("cobblestone", "chain"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Java: 0.0 .. 1.21.4");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: java's inferred range ends at 1.21.4, the last java target that \
          declares `minecraft:chain`"
        ],
    );
}

/// Java adds `pale_moss_block` at 1.21.4 and `leaf_litter` at 26.3; Bedrock drops `stonebrick`
/// after 1.21.0 and `chain` after 1.21.60.
#[test]
fn the_latest_start_and_the_earliest_end_bound_the_range() {
    let fixture = Fixture::new(&painting("pale_moss_block", "leaf_litter"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Java: 26.3 .. latest");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: java's inferred range starts at 26.3, the first java target that \
          declares `minecraft:leaf_litter`"
        ],
    );

    let fixture = Fixture::new(&painting("chain", "stonebrick"));
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
fn a_block_only_the_other_edition_has_leaves_no_range() {
    let fixture = Fixture::new(&painting("cobblestone", "test_block"));
    let run = info(&fixture, "bedrock", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_json(&run),
        serde_json::json!([{
            "edition": "bedrock",
            "min": "0.0",
            "max": "latest",
            "empty": true,
            "undeclared": [{ "id": "minecraft:test_block", "line": 3, "col": 16 }],
        }]),
    );
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:3:16: bedrock has no inferred range: no bedrock target declares \
             `minecraft:test_block`"
        ],
    );
}

#[test]
fn two_ids_that_set_one_edge_are_each_named() {
    let fixture = Fixture::new(&painting("pale_moss_block", "pale_oak_planks"));
    let run = info(&fixture, "java,bedrock", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let edge = serde_json::json!([
        { "id": "minecraft:pale_moss_block", "line": 2, "col": 17 },
        { "id": "minecraft:pale_oak_planks", "line": 3, "col": 16 },
    ]);
    assert_eq!(
        inferred_json(&run),
        serde_json::json!([
            { "edition": "java", "min": "1.21.4", "max": "latest", "empty": false, "min_ids": edge },
            { "edition": "bedrock", "min": "1.21.60", "max": "latest", "empty": false, "min_ids": edge },
        ]),
    );
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: java's inferred range starts at 1.21.4, the first java target \
             that declares `minecraft:pale_moss_block`",
            "note: s.crn:3:16: java's inferred range starts at 1.21.4, the first java target \
             that declares `minecraft:pale_oak_planks`",
            "note: s.crn:2:17: bedrock's inferred range starts at 1.21.60, the first bedrock \
             target that declares `minecraft:pale_moss_block`",
            "note: s.crn:3:16: bedrock's inferred range starts at 1.21.60, the first bedrock \
             target that declares `minecraft:pale_oak_planks`",
        ],
    );
}

#[test]
fn each_edition_counts_the_ids_its_own_theme_variant_paints() {
    let fixture = Fixture::new(
        "theme t:\n  slot floor -> @pale_moss_block\n\ntheme t_bedrock:\n  slot floor -> \
         @cobblestone\n\nstruct s size=2x2\n  floor mat_slot=floor\n",
    );
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_row(&run),
        "Java: 1.21.4 .. latest   Bedrock: 0.0 .. latest"
    );
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: java's inferred range starts at 1.21.4, the first java target \
             that declares `minecraft:pale_moss_block`"
        ],
    );
}

#[test]
fn an_id_painted_twice_is_named_once_where_the_source_first_names_it() {
    let fixture = Fixture::new(&painting("pale_moss_block", "pale_moss_block"));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Java: 1.21.4 .. latest");
    assert_eq!(
        range_notes(&fixture, &run),
        [
            "note: s.crn:2:17: java's inferred range starts at 1.21.4, the first java target that \
          declares `minecraft:pale_moss_block`"
        ],
    );
}

/// Bedrock's catalog maps `@floor.stone.smooth` onto `stone_bricks`, which 1.21.0 spells
/// `stonebrick`, and says so with an override.
#[test]
fn a_catalog_token_counts_as_each_pin_spells_it() {
    let fixture = Fixture::new(&painting("floor.stone.smooth", "cobblestone"));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(inferred_row(&run), "Bedrock: 0.0 .. latest");
    assert_eq!(range_notes(&fixture, &run), Vec::<String>::new());

    let authored = Fixture::new(&painting("stone_bricks", "cobblestone"));
    let run = info(&authored, "bedrock", &[]);
    assert_eq!(
        inferred_row(&run),
        "Bedrock: 1.21.40 .. latest",
        "the id the token reaches at 1.21.40 and later, written as an id, has no override",
    );
}

#[test]
fn a_floor_below_the_range_is_no_conflict() {
    let fixture = Fixture::new(&format!(
        "@requires version>=1.20.4\n{}",
        painting("pale_moss_block", "cobblestone")
    ));
    let run = info(&fixture, "java", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "registry compatibility:  1.20.4 .. latest\n\
         inferred range:          Java: 1.21.4 .. latest\n\
         edition portability:     Java: portable: 2  degraded: 0  unsupported: 0\n\
         buildable targets:       Java: 1.21.4, 26.3 (1.20.4, 1.21 refuse)\n\
         intended targets:        (none declared)\n\
         semantic-sensitive:      (none)\n\
         verified target:         (no lock)\n",
        "the declared row and the inferred row side by side",
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
        "@requires java version>=26.3\n{}",
        painting("cobblestone", "chain")
    ));
    let run = info(&fixture, "java", &[]);
    assert_eq!(
        run.code,
        Some(0),
        "a report is not a refusal: {}",
        run.stderr
    );
    assert_eq!(
        run.stdout,
        "registry compatibility:  0.0 .. latest\n\
         inferred range:          Java: 0.0 .. 1.21.4\n\
         edition portability:     Java: portable: 2  degraded: 0  unsupported: 0\n\
         buildable targets:       Java: none (1.20.4, 1.21, 1.21.4, 26.3 all refuse)\n\
         intended targets:        (none declared)\n\
         semantic-sensitive:      (none)\n\
         verified target:         (no lock)\n",
    );
    assert_eq!(
        stderr(&fixture, &run),
        "note: s.crn:1:1: `java version>=26.3` is scoped to one edition, so it is not part of the \
         `registry compatibility` row, which is edition-neutral; `buildable targets` is where it \
         is weighed\n\
         note: java 26.3 refuses this source\n\
         s.crn:4:16: error[E_UNKNOWN_ID]: `minecraft:chain` is not a block in `java 26.3`\n  \
         note: `java 26.3` spells this block `minecraft:iron_chain`, per the registry pack's alias \
         table\n  \
         note: the last target that declares `minecraft:chain` is java 1.21.4: build with \
         `--target 1.21.4` or another target that declares it, or pick a block `java 26.3` has\n\
         note: s.crn:1:1: java 1.20.4, 1.21, 1.21.4 are below the `java version>=26.3` this file \
         declares\n\
         note: s.crn:4:16: java's inferred range ends at 1.21.4, the last java target that \
         declares `minecraft:chain`\n\
         error[E_REQUIRES_CONFLICT]: s.crn:1:1: this file requires java version>=26.3, but no \
         java target it admits declares `minecraft:chain`, which this build paints.\n  \
         note: s.crn:4:16: `minecraft:chain` is last declared at java 1.21.4\n  \
         fix: lower the `@requires` floor to 1.21.4 or below, or replace `minecraft:chain` with \
         a block a java target at or above 26.3 declares\n\
         note: no lockfile at s.crn.lock, so no target is verified; `cairn compile` writes one\n",
        "the refused versions, the floor's note, the range's note, then the conflict",
    );
}

#[test]
fn a_floor_at_or_below_the_upper_edge_is_no_conflict() {
    for floor in ["1.21.4", "1.21"] {
        let fixture = Fixture::new(&format!(
            "@requires version>={floor}\n{}",
            painting("cobblestone", "chain")
        ));
        let run = info(&fixture, "java", &["--format", "json"]);
        assert_eq!(run.code, Some(0), "{floor}: {}", run.stderr);
        assert_eq!(
            inferred_json(&run),
            serde_json::json!([{
                "edition": "java",
                "min": "0.0",
                "max": "1.21.4",
                "empty": false,
                "max_ids": [{ "id": "minecraft:chain", "line": 4, "col": 16 }],
            }]),
            "{floor}",
        );
        assert_stream(&run.stderr, &[(4, 16, "error", "E_UNKNOWN_ID")]);
    }
}

#[test]
fn a_floor_no_carried_version_meets_is_not_weighed() {
    let fixture = Fixture::new(&format!(
        "@requires version>=26.3\n{}",
        painting("cobblestone", "chain")
    ));
    let run = info(&fixture, "bedrock", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_json(&run),
        serde_json::json!([{
            "edition": "bedrock",
            "min": "0.0",
            "max": "1.21.60",
            "empty": false,
            "max_ids": [{ "id": "minecraft:chain", "line": 4, "col": 16 }],
        }]),
    );
    assert_stream(&run.stderr, &[]);
}

#[test]
fn a_run_an_edition_refuses_still_reports_the_conflict_and_its_json_carries_only_the_refusal() {
    let fixture = Fixture::new(&format!(
        "@requires java version>=26.3\n{WALL_LEVER}  raw_block mat=minecraft:chain at=2,1,2\n"
    ));
    let notes = "note: java 26.3 refuses this source\n\
         s.crn:12:17: error[E_UNKNOWN_ID]: `minecraft:chain` is not a block in `java 26.3`\n  \
         note: `java 26.3` spells this block `minecraft:iron_chain`, per the registry pack's alias \
         table\n  \
         note: the last target that declares `minecraft:chain` is java 1.21.4: build with \
         `--target 1.21.4` or another target that declares it, or pick a block `java 26.3` has\n\
         note: s.crn:1:1: java 1.20.4, 1.21, 1.21.4 are below the `java version>=26.3` this file \
         declares\n\
         note: s.crn:12:17: java's inferred range ends at 1.21.4, the last java target that \
         declares `minecraft:chain`\n\
         error[E_REQUIRES_CONFLICT]: s.crn:1:1: this file requires java version>=26.3, but no \
         java target it admits declares `minecraft:chain`, which this build paints.\n  \
         note: s.crn:12:17: `minecraft:chain` is last declared at java 1.21.4\n  \
         fix: lower the `@requires` floor to 1.21.4 or below, or replace `minecraft:chain` with \
         a block a java target at or above 26.3 declares\n";
    let scoped = "note: s.crn:1:1: `java version>=26.3` is scoped to one edition, so it is not part \
         of the `registry compatibility` row, which is edition-neutral; `buildable targets` is \
         where it is weighed\n";

    let text = info(&fixture, "java", &[]);
    assert_eq!(text.code, Some(1), "{}", text.stderr);
    assert_eq!(text.stdout, "");
    assert_eq!(
        stderr(&fixture, &text),
        format!(
            "{scoped}s.crn:9:3: error[E_ROUTE_UNREACHABLE_COMPONENT]: struct=room: the lever `l` \
             at y=2 cannot reach the circuit under the floor: a lever or button reaches the layer \
             under the floor only from the floor row `y = 0`, the row over it\n  \
             note: Fix: set `y=0` on it\n{notes}"
        ),
    );

    let json = info(&fixture, "java", &["--format", "json"]);
    assert_eq!(json.code, Some(1), "{}", json.stderr);
    assert_eq!(stderr(&fixture, &json), format!("{scoped}{notes}"));
    let document: serde_json::Value = serde_json::from_str(&json.stdout).expect("valid JSON");
    let refusals: Vec<(&str, u64, u64)> = document["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .map(|d| {
            (
                d["code"].as_str().expect("code is a string"),
                d["line"].as_u64().expect("line is a number"),
                d["col"].as_u64().expect("col is a number"),
            )
        })
        .collect();
    assert_eq!(refusals, [("E_ROUTE_UNREACHABLE_COMPONENT", 9, 3)]);
}

#[test]
fn json_carries_each_editions_range_with_its_ids_and_conflicts() {
    let fixture = Fixture::new(&format!(
        "@requires java version>=26.3\n{}",
        painting("pale_moss_block", "chain")
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
                "max": "1.21.4",
                "empty": false,
                "min_ids": [{ "id": "minecraft:pale_moss_block", "line": 3, "col": 17 }],
                "max_ids": [{ "id": "minecraft:chain", "line": 4, "col": 16 }],
                "conflicts": [{
                    "floor": { "declared": "java version>=26.3", "line": 1, "col": 1 },
                    "ids": [{
                        "id": "minecraft:chain",
                        "last_declared": "1.21.4",
                        "line": 4,
                        "col": 16,
                    }],
                }],
            },
            {
                "edition": "bedrock",
                "min": "1.21.60",
                "max": "1.21.60",
                "empty": false,
                "min_ids": [{ "id": "minecraft:pale_moss_block", "line": 3, "col": 17 }],
                "max_ids": [{ "id": "minecraft:chain", "line": 4, "col": 16 }],
            },
        ]),
    );
}

#[test]
fn json_names_the_part_that_declares_a_conflicting_floor() {
    let fixture = Fixture::new(CHAIN_UNDER_THREE_FLOORS);
    let run = info(&fixture, "java", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let chain = serde_json::json!([{
        "id": "minecraft:chain",
        "last_declared": "1.21.4",
        "line": 5,
        "col": 16,
    }]);
    assert_eq!(
        inferred_json(&run),
        serde_json::json!([{
            "edition": "java",
            "min": "0.0",
            "max": "1.21.4",
            "empty": false,
            "max_ids": [{ "id": "minecraft:chain", "line": 5, "col": 16 }],
            "conflicts": [
                {
                    "floor": { "declared": "java version>=26.3", "line": 2, "col": 1 },
                    "ids": chain,
                },
                {
                    "floor": {
                        "declared": "java version>=26.3",
                        "line": 8,
                        "col": 3,
                        "declared_by": { "keyword": "def", "name": "cottage" },
                    },
                    "ids": chain,
                },
            ],
        }]),
    );
}

#[test]
fn json_marks_a_range_no_version_can_lie_in_as_empty() {
    let fixture = Fixture::new(&painting("cobblestone", "no_such_block"));
    let run = info(&fixture, "java", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        inferred_json(&run),
        serde_json::json!([{
            "edition": "java",
            "min": "0.0",
            "max": "latest",
            "empty": true,
            "undeclared": [{ "id": "minecraft:no_such_block", "line": 3, "col": 16 }],
        }]),
    );
}
