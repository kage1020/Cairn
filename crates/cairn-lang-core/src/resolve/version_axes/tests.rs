use super::*;
use crate::parse;

/// Reports with only the portable count set.
fn synthetic_portability(entries: &[(Edition, u32)]) -> Vec<EditionReport> {
    entries
        .iter()
        .map(|&(edition, portable)| EditionReport {
            edition,
            portable,
            unsupported_entries: Vec::new(),
            degraded_entries: Vec::new(),
            buildable: Vec::new(),
            considered: Vec::new(),
            refusal: None,
            inferred_range: open_range(edition),
        })
        .collect()
}

/// A range no id bounds.
fn open_range(edition: Edition) -> InferredRange {
    InferredRange {
        edition,
        min: "0.0".to_owned(),
        max: "latest".to_owned(),
        empty: false,
        min_ids: Vec::new(),
        max_ids: Vec::new(),
        undeclared: Vec::new(),
        conflicts: Vec::new(),
    }
}

#[test]
fn registry_compat_min_from_requires_header() {
    let src = "@requires version>=1.20\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "1.20");
    assert_eq!(axes.registry_compat.max, "latest");
}

#[test]
fn registry_compat_takes_max_when_multiple_requires_present() {
    let src = "@requires version>=1.20\n@requires version>=1.21\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "1.21");
}

#[test]
fn registry_compat_defaults_when_requires_absent() {
    let src = "struct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "0.0");
}

#[test]
fn edition_portability_is_forwarded_verbatim_from_caller() {
    let src = "struct s size=4x4\n  walls height=3\n  floor\n  roof kind=gable\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(
        &m,
        vec![
            EditionReport {
                edition: Edition::Java,
                portable: 3,
                unsupported_entries: Vec::new(),
                degraded_entries: Vec::new(),
                buildable: vec!["1.20.4".to_owned()],
                considered: vec!["1.20.4".to_owned()],
                refusal: None,
                inferred_range: open_range(Edition::Java),
            },
            EditionReport {
                edition: Edition::Bedrock,
                portable: 1,
                unsupported_entries: one_entry_per_reason(),
                degraded_entries: vec![DegradedEntry {
                    id: "minecraft:spruce_stairs".to_owned(),
                    states: "facing=north,shape=outer_left".to_owned(),
                    dropped: vec![DroppedIntent {
                        key: "shape".to_owned(),
                        value: "outer_left".to_owned(),
                        mc_versions: vec!["1.21.0".to_owned()],
                        reason: "no stair shape".to_owned(),
                    }],
                }],
                buildable: vec!["1.21.0".to_owned()],
                considered: vec!["1.21.0".to_owned(), "1.21.40".to_owned()],
                refusal: None,
                inferred_range: open_range(Edition::Bedrock),
            },
        ],
    );
    assert_eq!(axes.edition_portability.len(), 2);
    assert_eq!(axes.edition_portability[0].edition, Edition::Java);
    assert_eq!(axes.edition_portability[0].portable, 3);
    assert_eq!(axes.edition_portability[0].degraded, 0);
    assert_eq!(axes.edition_portability[0].unsupported, 0);
    assert_eq!(axes.edition_portability[1].edition, Edition::Bedrock);
    assert_eq!(axes.edition_portability[1].portable, 1);
    assert_eq!(axes.edition_portability[1].degraded, 1);
    assert_eq!(axes.edition_portability[1].unsupported, 3);
    assert_eq!(
        axes.edition_portability[1].degraded as usize,
        axes.edition_portability[1].degraded_entries.len(),
    );
    assert_eq!(
        axes.edition_portability[1].unsupported as usize,
        axes.edition_portability[1].unsupported_entries.len(),
    );
    let json = serde_json::to_string(&axes).unwrap();
    assert!(json.contains(r#""edition":"java""#), "got: {json}");
    assert!(json.contains(r#""edition":"bedrock""#), "got: {json}");
    assert_eq!(axes.buildable_targets.len(), axes.edition_portability.len());
    let editions: Vec<Edition> = axes
        .buildable_targets
        .iter()
        .map(|row| row.edition)
        .collect();
    assert_eq!(
        editions,
        axes.edition_portability
            .iter()
            .map(|row| row.edition)
            .collect::<Vec<_>>(),
    );
    assert_eq!(axes.buildable_targets[1].buildable, ["1.21.0"]);
    assert_eq!(axes.buildable_targets[1].considered, ["1.21.0", "1.21.40"]);
    assert!(json.contains(r#""buildable":["1.21.0"]"#), "got: {json}");
    assert!(
        json.contains(r#""considered":["1.21.0","1.21.40"]"#),
        "got: {json}",
    );
    assert_eq!(
        axes.edition_portability[1].unsupported_entries,
        one_entry_per_reason(),
    );
    assert_eq!(axes.edition_portability[0].unsupported_entries, Vec::new());
}

/// One [`UnsupportedEntry`] per [`UnsupportedReason`] variant.
fn one_entry_per_reason() -> Vec<UnsupportedEntry> {
    vec![
        UnsupportedEntry {
            id: "minecraft:oak_sign".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_log".to_owned()),
                aliases: vec!["minecraft:standing_sign".to_owned()],
            },
        },
        UnsupportedEntry {
            id: "minecraft:border_block".to_owned(),
            reason: UnsupportedReason::NoCounterpart {
                detail: "the border block, which Bedrock uses to wall players in; Java has no \
                         such block"
                    .to_owned(),
            },
        },
        UnsupportedEntry {
            id: "minecraft:camera".to_owned(),
            reason: UnsupportedReason::StatesUnmapped {
                states: "facing_direction=1".to_owned(),
                detail: "no Java block maps onto `minecraft:camera` there".to_owned(),
            },
        },
    ]
}

/// One of these reasons is unreachable from a `.crn`, so no CLI test covers its JSON.
#[test]
fn every_unsupported_reason_carries_its_own_wire_shape() {
    let entries = serde_json::to_value(one_entry_per_reason()).expect("the entries serialize");
    assert_eq!(
        entries,
        serde_json::json!([
            {
                "id": "minecraft:oak_sign",
                "reason": "absent_from_edition",
                "suggestion": "minecraft:oak_log",
                "aliases": ["minecraft:standing_sign"],
            },
            {
                "id": "minecraft:border_block",
                "reason": "no_counterpart",
                "detail": "the border block, which Bedrock uses to wall players in; Java has no \
                           such block",
            },
            {
                "id": "minecraft:camera",
                "reason": "states_unmapped",
                "states": "facing_direction=1",
                "detail": "no Java block maps onto `minecraft:camera` there",
            },
        ]),
    );
    let absent = serde_json::to_value(UnsupportedEntry {
        id: "minecraft:nothing_like_this".to_owned(),
        reason: UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    })
    .expect("the entry serializes");
    assert_eq!(
        absent,
        serde_json::json!({
            "id": "minecraft:nothing_like_this",
            "reason": "absent_from_edition",
            "suggestion": null,
            "aliases": [],
        }),
    );
}

#[test]
fn semantic_sensitive_is_empty_without_catalog() {
    let src = "struct s size=4x4\n  walls height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.semantic_sensitive, [] as [SemanticSensitiveFinding; 0]);
}

#[test]
fn floor_part_spells_the_surface_keyword() {
    for (part, kind) in [
        (FloorPart::Def, ItemKind::Def),
        (FloorPart::Theme, ItemKind::Theme),
    ] {
        assert_eq!(
            serde_json::to_value(part).expect("serialize"),
            serde_json::Value::String(kind.keyword().to_owned()),
        );
    }
    for origin in [
        FloorOrigin::Module,
        FloorOrigin::Def("d".to_owned()),
        FloorOrigin::Theme("t".to_owned()),
    ] {
        let Some((keyword, _)) = origin.part() else {
            continue;
        };
        assert!(
            [FloorPart::Def, FloorPart::Theme]
                .iter()
                .any(|part| serde_json::to_value(part).expect("serialize") == keyword),
            "`{keyword}` can declare a floor but `FloorPart` has no variant for it",
        );
    }
}

#[test]
fn a_dropped_intent_serializes_its_pieces_and_not_its_sentence() {
    let dropped = DroppedIntent {
        key: "shape".to_owned(),
        value: "outer_left".to_owned(),
        mc_versions: vec!["1.21.0".to_owned(), "1.21.60".to_owned()],
        reason: "no stair shape".to_owned(),
    };
    assert_eq!(
        serde_json::to_value(dropped).expect("the intent serializes"),
        serde_json::json!({
            "key": "shape",
            "value": "outer_left",
            "mc_versions": ["1.21.0", "1.21.60"],
        }),
    );
}

#[test]
fn each_editions_inferred_range_is_forwarded_in_the_callers_order() {
    let m = parse("struct s size=4x4\n  walls height=3\n").expect("parse");
    let mut reports = synthetic_portability(&[(Edition::Bedrock, 0), (Edition::Java, 0)]);
    reports[0].inferred_range.min = "1.21.40".to_owned();
    let axes = compute_axes(&m, reports);
    let ranges: Vec<(Edition, &str)> = axes
        .inferred_range
        .iter()
        .map(|range| (range.edition, range.min.as_str()))
        .collect();
    assert_eq!(
        ranges,
        [(Edition::Bedrock, "1.21.40"), (Edition::Java, "0.0")]
    );
}

#[test]
fn an_open_range_serializes_its_edges_and_nothing_it_lacks() {
    assert_eq!(
        serde_json::to_value(open_range(Edition::Java)).expect("the range serializes"),
        serde_json::json!({ "edition": "java", "min": "0.0", "max": "latest", "empty": false }),
    );
}

#[test]
fn a_bounded_range_names_its_ids_and_the_floors_above_it() {
    let at = |line: u32, col: u32| Position {
        line: std::num::NonZeroU32::new(line).expect("1-based"),
        col: std::num::NonZeroU32::new(col).expect("1-based"),
    };
    let range = InferredRange {
        edition: Edition::Java,
        min: "1.21.4".to_owned(),
        max: "1.21.4".to_owned(),
        empty: false,
        min_ids: vec![PaintedId {
            id: "minecraft:pale_moss_block".to_owned(),
            position: at(4, 17),
        }],
        max_ids: vec![PaintedId {
            id: "minecraft:chain".to_owned(),
            position: at(5, 17),
        }],
        undeclared: vec![PaintedId {
            id: "minecraft:nonesuch".to_owned(),
            position: at(6, 17),
        }],
        conflicts: vec![RequiresConflict {
            floor: DeclaredFloor {
                declared: "java version>=26.3".to_owned(),
                position: at(1, 1),
                declared_by: None,
            },
            ids: vec![LastDeclaredId {
                id: "minecraft:chain".to_owned(),
                last_declared: "1.21.4".to_owned(),
                position: at(5, 17),
            }],
        }],
    };
    assert_eq!(
        serde_json::to_value(range).expect("the range serializes"),
        serde_json::json!({
            "edition": "java",
            "min": "1.21.4",
            "max": "1.21.4",
            "empty": false,
            "min_ids": [{ "id": "minecraft:pale_moss_block", "line": 4, "col": 17 }],
            "max_ids": [{ "id": "minecraft:chain", "line": 5, "col": 17 }],
            "undeclared": [{ "id": "minecraft:nonesuch", "line": 6, "col": 17 }],
            "conflicts": [{
                "floor": { "declared": "java version>=26.3", "line": 1, "col": 1 },
                "ids": [{
                    "id": "minecraft:chain",
                    "last_declared": "1.21.4",
                    "line": 5,
                    "col": 17,
                }],
            }],
        }),
    );
}
