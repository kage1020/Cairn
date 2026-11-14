//! The `respelled` row: per edition, the canonical tokens a version writes under another id, per
//! `spec/versioning-editions` "The `respelled` row".

use crate::common::{Fixture, Run, cairn_argv};

/// A struct whose floor, walls and roof paint the theme's three slots.
fn painting(floor: &str, wall: &str, roof: &str) -> String {
    format!(
        "theme t:\n  slot floor -> @{floor}\n  slot wall -> @{wall}\n  slot roof -> @{roof}\n\n\
         struct s size=3x3\n  floor mat_slot=floor\n  walls mat_slot=wall height=2\n  roof \
         kind=flat mat_slot=roof\n"
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

/// The text after the `respelled:` label.
fn respelled_row(run: &Run) -> &str {
    let rows: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("respelled:               "))
        .collect();
    let [row] = rows.as_slice() else {
        panic!("one `respelled` row expected, got: {}", run.stdout);
    };
    row
}

/// The `note:` lines about a respelled token, with the fixture's path cut to `s.crn`.
fn respelled_notes(fixture: &Fixture, run: &Run) -> Vec<String> {
    let path = fixture.source().display().to_string();
    run.stderr
        .lines()
        .filter(|line| line.contains(" is written `"))
        .map(|line| line.replace(&path, "s.crn"))
        .collect()
}

#[test]
fn tokens_every_version_spells_as_written_leave_the_row_empty() {
    let fixture = Fixture::new(&painting("cobblestone", "oak_planks", "stone"));
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(respelled_row(&run), "Java: none   Bedrock: none");
    assert_eq!(respelled_notes(&fixture, &run), Vec::<String>::new());
}

#[test]
fn each_respelled_token_is_named_with_the_table_that_maps_it() {
    let fixture = Fixture::new(&painting("chain", "oak_sign", "cauldron[fluid=water]"));
    let run = info(&fixture, "java,bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        respelled_row(&run),
        "Java: minecraft:chain, minecraft:cauldron   Bedrock: minecraft:chain, minecraft:oak_sign"
    );
    assert_eq!(
        respelled_notes(&fixture, &run),
        [
            "note: s.crn:2:17: `minecraft:chain` is written `minecraft:iron_chain` at java 26.3, \
             per the registry pack's alias table",
            "note: s.crn:4:16: `minecraft:cauldron` is written `minecraft:water_cauldron` at java \
             1.20.4, 1.21, 1.21.4, 26.3, per the registry pack's meaning table",
            "note: s.crn:2:17: `minecraft:chain` is written `minecraft:iron_chain` at bedrock \
             1.26.60, per the registry pack's alias table",
            "note: s.crn:3:16: `minecraft:oak_sign` is written `minecraft:standing_sign` at \
             bedrock 1.21.0, 1.21.40, 1.21.60, 1.26.60, per the registry pack's state map",
        ],
    );
}

#[test]
fn only_the_versions_the_floors_admit_are_weighed() {
    let fixture = Fixture::new(&format!(
        "@requires bedrock version>=1.21.40\n{}",
        painting("stone_bricks", "oak_sign", "stone")
    ));
    let run = info(&fixture, "bedrock", &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        respelled_notes(&fixture, &run),
        [
            "note: s.crn:4:16: `minecraft:oak_sign` is written `minecraft:standing_sign` at bedrock \
          1.21.40, 1.21.60, 1.26.60, per the registry pack's state map"
        ],
        "1.21.0, which writes `stone_bricks` as `stonebrick`, is below the floor",
    );
}

#[test]
fn json_lists_each_version_with_the_id_it_writes() {
    let fixture = Fixture::new(&painting("chain", "cobblestone", "stone"));
    let run = info(&fixture, "java,bedrock", &["--format", "json"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let report: serde_json::Value = serde_json::from_str(&run.stdout).expect("valid JSON");
    assert_eq!(
        report["respelled"],
        serde_json::json!([
            {
                "edition": "java",
                "tokens": [{
                    "id": "minecraft:chain",
                    "line": 2,
                    "col": 17,
                    "written": [
                        { "version": "26.3", "id": "minecraft:iron_chain", "through": "aliases" },
                    ],
                }],
            },
            {
                "edition": "bedrock",
                "tokens": [{
                    "id": "minecraft:chain",
                    "line": 2,
                    "col": 17,
                    "written": [
                        {
                            "version": "1.26.60",
                            "id": "minecraft:iron_chain",
                            "through": "aliases",
                        },
                    ],
                }],
            },
        ]),
    );
}
