//! `cairn check` without `--target`: the lowering findings every target that reads their line
//! raises, weighed as if every id some shipped version declares were declared, and none whose
//! verdict is a target's pack's to give.

use std::path::Path;

use crate::common::{Run, assert_stream, cairn, json_stream, reported, write_source};

/// A shed roof outside the stair family, and a door whose side is a number.
const SHED_ON_COBBLESTONE: &str = concat!(
    "@cairn 2026.06\n\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "  slot roof -> @cobblestone\n\n",
    "struct hut size=5x5\n",
    "  walls mat_slot=wall height=3\n",
    "  roof kind=shed slope_to=front mat_slot=roof\n",
    "  door side=3\n",
);

/// `cairn check FILE` with `flags`.
fn check(src: &Path, flags: &[&str]) -> Run {
    let mut args = vec![src.to_str().expect("utf-8 path")];
    args.extend_from_slice(flags);
    cairn("check", &args).into()
}

#[test]
fn a_bare_check_reports_the_lowering_findings_a_pinned_check_reports() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", SHED_ON_COBBLESTONE);
    let bare = check(&src, &[]);
    assert_eq!(bare.code, Some(1), "{}", bare.stderr);
    assert_stream(
        &bare.stderr,
        &[
            (5, 16, "error", "E_INCOMPATIBLE_MATERIAL"),
            (10, 3, "error", "E_INVALID_MEMBER"),
        ],
    );
    let headlines: Vec<&str> = bare
        .stderr
        .lines()
        .filter(|line| !line.starts_with("  note: "))
        .collect();
    let path = src.display();
    assert_eq!(
        headlines,
        [
            format!(
                "{path}:5:16: error[E_INCOMPATIBLE_MATERIAL]: `shed` roof derives `facing` / \
                 `half` / `shape` from the geometry and `minecraft:cobblestone` is not a stair, \
                 so those states have nowhere to go"
            ),
            format!(
                "{path}:10:3: error[E_INVALID_MEMBER]: `side=` must be one of front, back, \
                 left, right, not integer `3`"
            ),
        ],
    );

    for flags in [
        &["--edition", "java"][..],
        &["--edition", "bedrock"],
        &["--edition", "java", "--target", "1.21.4"],
        &["--edition", "bedrock", "--target", "1.21.60"],
    ] {
        let run = check(&src, flags);
        assert_eq!(run.code, Some(1), "{flags:?}: {}", run.stderr);
        assert_eq!(run.stderr, bare.stderr, "{flags:?}");
    }
}

#[test]
fn the_json_report_of_a_bare_check_is_the_pinned_one() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", SHED_ON_COBBLESTONE);
    let bare = check(&src, &["--format", "json"]);
    assert_eq!(bare.code, Some(1), "{}", bare.stderr);
    assert_eq!(bare.stderr, "");
    assert_eq!(
        json_stream(&bare.stdout),
        reported(&[
            (5, 16, "error", "E_INCOMPATIBLE_MATERIAL"),
            (10, 3, "error", "E_INVALID_MEMBER"),
        ]),
    );
    let pinned = check(
        &src,
        &[
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--format",
            "json",
        ],
    );
    assert_eq!(pinned.code, Some(1), "{}", pinned.stderr);
    assert_eq!(pinned.stdout, bare.stdout);
}

#[test]
fn a_finding_whose_verdict_is_a_targets_pack_is_left_to_a_pinned_check() {
    // An id no target declares, a misspelt abstract token, and an abstract token on a roof that
    // needs a stair.
    let source = concat!(
        "theme t:\n",
        "  slot floor -> @totally_not_a_block\n",
        "  slot wall -> @wall.stone.cobbel\n",
        "  slot roof -> @wall.stone.cobble\n\n",
        "struct s size=5x5\n",
        "  floor mat_slot=floor\n",
        "  walls mat_slot=wall height=3\n",
        "  roof kind=gable mat_slot=roof\n",
    );
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "s.crn", source);
    let bare = check(&src, &[]);
    assert_eq!(bare.code, Some(0), "{}", bare.stderr);
    assert_eq!(bare.stderr, "");

    let pinned = check(&src, &["--edition", "java", "--target", "1.21.4"]);
    assert_eq!(pinned.code, Some(1), "{}", pinned.stderr);
    assert_stream(
        &pinned.stderr,
        &[
            (2, 17, "error", "E_UNKNOWN_ID"),
            (3, 16, "error", "E_UNKNOWN_ABSTRACT_TOKEN"),
            (4, 16, "error", "E_INCOMPATIBLE_MATERIAL"),
        ],
    );
}

#[test]
fn a_bare_check_names_no_lost_scope() {
    let source = concat!(
        "theme t:\n",
        "  slot floor -> @oak_planks\n\n",
        "struct good size=2x2\n",
        "  floor mat_slot=floor\n\n",
        "struct bad\n",
        "  floor mat_slot=floor\n",
    );
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "s.crn", source);
    let bare = check(&src, &[]);
    assert_eq!(bare.code, Some(0), "{}", bare.stderr);
    assert_stream(&bare.stderr, &[(7, 1, "warning", "W_STRUCT_NO_SIZE")]);
}

#[test]
fn an_id_no_shipped_version_declares_is_left_to_a_pinned_check() {
    let source = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "  slot roof -> @oak_stair\n\n",
        "struct hut size=5x5\n",
        "  walls mat_slot=wall height=3\n",
        "  roof kind=gable mat_slot=roof\n",
    );
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", source);
    for flags in [&[][..], &["--edition", "java"], &["--edition", "bedrock"]] {
        let run = check(&src, flags);
        assert_eq!(run.code, Some(0), "{flags:?}: {}", run.stderr);
        assert_eq!(run.stderr, "", "{flags:?}");
    }
    for flags in [
        &["--edition", "java", "--target", "1.21.4"][..],
        &["--edition", "bedrock", "--target", "1.21.60"],
    ] {
        let run = check(&src, flags);
        assert_eq!(run.code, Some(1), "{flags:?}: {}", run.stderr);
        assert_stream(
            &run.stderr,
            &[
                (3, 16, "error", "E_UNKNOWN_ID"),
                (7, 3, "warning", "W_DEFERRED_MEMBER"),
            ],
        );
    }
}

/// `t_java` and `t_bedrock` binding the roof slot of a gable roof to `java` and `bedrock`.
fn variant_roofs(java: &str, bedrock: &str) -> String {
    format!(
        "@cairn 2026.06\n\ntheme t_java:\n  slot wall -> @stone\n  slot roof -> {java}\n\n\
         theme t_bedrock:\n  slot wall -> @stone\n  slot roof -> {bedrock}\n\n\
         struct s size=5x5\n  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof\n"
    )
}

#[test]
fn a_finding_each_edition_raises_on_its_own_variant_is_reported_for_each() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "s.crn",
        &variant_roofs("@cobblestone", "@cobblestone"),
    );
    let java = (5, 16, "error", "E_INCOMPATIBLE_MATERIAL");
    let bedrock = (9, 16, "error", "E_INCOMPATIBLE_MATERIAL");
    let bare = check(&src, &[]);
    assert_eq!(bare.code, Some(1), "{}", bare.stderr);
    assert_stream(&bare.stderr, &[java, bedrock]);
    for (edition, target, found) in [("java", "1.21.4", java), ("bedrock", "1.21.60", bedrock)] {
        let alone = check(&src, &["--edition", edition]);
        assert_eq!(alone.code, Some(1), "{edition}: {}", alone.stderr);
        assert_stream(&alone.stderr, &[found]);
        let pinned = check(&src, &["--edition", edition, "--target", target]);
        assert_eq!(pinned.stderr, alone.stderr, "{edition}");
    }
}

#[test]
fn a_finding_one_edition_variant_raises_fails_a_check_for_that_edition_only() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "s.crn",
        &variant_roofs("@oak_stairs", "@cobblestone"),
    );
    let bedrock = [(9, 16, "error", "E_INCOMPATIBLE_MATERIAL")];
    for flags in [&[][..], &["--edition", "bedrock"]] {
        let run = check(&src, flags);
        assert_eq!(run.code, Some(1), "{flags:?}: {}", run.stderr);
        assert_stream(&run.stderr, &bedrock);
    }
    let java = check(&src, &["--edition", "java"]);
    assert_eq!(java.code, Some(0), "{}", java.stderr);
    assert_eq!(java.stderr, "");
}
