use super::*;

/// A floor on line 2 refusing the target, with a note pointing at line 1.
fn version_cap(at: RunLevelAt) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_VERSION_CAP"),
        at,
        primary: "this file requires version>=1.21 (target 1.20.4).".to_owned(),
        notes: vec![
            RunLevelNote::At {
                at: 7,
                note: "`struct a` is what the floor holds".to_owned(),
            },
            RunLevelNote::Line("fix: --target 1.21, or lower the `@requires` floor".to_owned()),
        ],
    }
}

const SOURCE: &str = "struct a size=3x3\n@requires version>=1.21\n";

#[test]
fn a_message_prints_gcc_style_at_the_place_it_names() {
    let lines = LineStarts::new(SOURCE);
    assert_eq!(
        version_cap(RunLevelAt::Offset(18)).lines("hut.crn", SOURCE, &lines),
        [
            "error[E_VERSION_CAP]: hut.crn:2:1: this file requires version>=1.21 (target 1.20.4).",
            "  note: hut.crn:1:8: `struct a` is what the floor holds",
            "  fix: --target 1.21, or lower the `@requires` floor",
        ],
    );
    assert_eq!(
        version_cap(RunLevelAt::Source).lines("hut.crn", SOURCE, &lines)[0],
        "error[E_VERSION_CAP]: hut.crn: this file requires version>=1.21 (target 1.20.4).",
    );
    assert_eq!(
        unsupported_target(&UnsupportedTarget {
            edition: Edition::Java,
            requested: "9.9".to_owned(),
            suggestion: None,
            supported: "1.21".to_owned(),
        })
        .lines("hut.crn", SOURCE, &lines),
        ["error: unsupported java target `9.9`. supported targets: 1.21"],
    );
}

#[test]
fn a_message_renders_as_a_finding_does_with_a_position_only_where_it_points() {
    let lines = LineStarts::new(SOURCE);
    assert_eq!(
        serde_json::to_value(version_cap(RunLevelAt::Offset(18)).render(SOURCE, &lines))
            .expect("serialises"),
        serde_json::json!({
            "code": "E_VERSION_CAP",
            "severity": "error",
            "line": 2,
            "col": 1,
            "primary": "this file requires version>=1.21 (target 1.20.4).",
            "notes": [
                { "line": 1, "col": 8, "message": "note: `struct a` is what the floor holds" },
                { "message": "fix: --target 1.21, or lower the `@requires` floor" },
            ],
        }),
    );
    let nowhere = backend_refusal("struct::a", "too tall");
    assert_eq!(
        serde_json::to_value(nowhere.render(SOURCE, &lines)).expect("serialises"),
        serde_json::json!({
            "severity": "error",
            "primary": "checking `struct::a`: too tall",
        }),
    );
}

#[test]
fn a_partial_build_counts_what_was_asked_for_and_names_each_lost_scope() {
    assert_eq!(
        partial_build(
            &["struct::shed".to_owned()],
            1,
            "refusing to certify a partial build"
        ),
        RunLevelMessage {
            severity: Severity::Error,
            code: Some("E_PARTIAL_BUILD"),
            at: RunLevelAt::Source,
            primary: "1 of 2 requested scopes did not lower; refusing to certify a partial build"
                .to_owned(),
            notes: vec![RunLevelNote::Line(
                "note: `struct::shed` produced no voxels".to_owned()
            )],
        },
    );
}
