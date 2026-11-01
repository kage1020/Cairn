//! `cairn compile` against block ids the target does not have, per `spec/versioning-editions`
//! "Fail-loud and minimum-version inference".

use std::process::Command;

use crate::common::{Fixture, assert_stream, cairn_argv, cargo_bin, compile_as, stderr_of};

/// A struct with one painted floor whose `slot floor` binds `{id}`.
fn source_binding(id: &str) -> String {
    format!("theme t:\n  slot floor -> @{id}\nstruct s size=2x2\n  floor mat_slot=floor\n")
}

#[test]
fn an_id_no_edition_has_is_refused_rather_than_written() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = compile_as(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert_stream(&stderr, &[(2, 17, "error", "E_UNKNOWN_ID")]);
    assert!(
        stderr.contains("minecraft:totally_not_a_block"),
        "the message must name the id it refused, got: {stderr}",
    );
}

#[test]
fn a_java_only_block_is_refused_on_bedrock_and_compiles_on_java() {
    // Bedrock spells Java's `light` `light_block_0` … `light_block_15`.
    let bedrock = Fixture::new(&source_binding("light"));
    let out = compile_as(&bedrock, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert_stream(&stderr, &[(2, 17, "error", "E_UNKNOWN_ID")]);
    assert!(
        stderr.contains("bedrock 1.21.60"),
        "the message must name the registry it checked, got: {stderr}",
    );

    let java = Fixture::new(&source_binding("light"));
    let out = compile_as(&java, "java", "1.21.4");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the same source must compile on the edition that has the block; stderr: {}",
        stderr_of(&out),
    );
}

#[test]
fn a_typo_is_answered_with_the_id_the_target_does_spell() {
    let fixture = Fixture::new(&source_binding("oak_plank"));
    let out = compile_as(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("minecraft:oak_planks"),
        "a one-letter typo must be answered with the real id, got: {stderr}",
    );
}

#[test]
fn a_rename_is_answered_with_the_name_the_target_uses() {
    let fixture = Fixture::new(&source_binding("light"));
    let out = compile_as(&fixture, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("minecraft:light_block_0") && stderr.contains("alias table"),
        "expected the alias answer, got: {stderr}",
    );
    assert!(
        !stderr.contains("is near enough to suggest"),
        "the alias answer stands in place of the no-candidate note, got: {stderr}",
    );
}

#[test]
fn the_same_group_answers_from_either_edition() {
    let on_bedrock = Fixture::new(&source_binding("oak_sign"));
    let out = compile_as(&on_bedrock, "bedrock", "1.21.60");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:standing_sign"),
        "bedrock spells the oak sign `standing_sign`, got: {}",
        stderr_of(&out),
    );

    let on_java = Fixture::new(&source_binding("standing_sign"));
    let out = compile_as(&on_java, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:oak_sign"),
        "java spells the same block `oak_sign`, got: {}",
        stderr_of(&out),
    );
}

#[test]
fn an_id_no_group_names_still_says_it_has_no_candidate() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = compile_as(&fixture, "bedrock", "1.21.60");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is near enough to suggest") && !stderr.contains("alias table"),
        "expected the no-candidate note, got: {stderr}",
    );
}

/// Bedrock 1.21.0 spells `stone_bricks` `stonebrick`.
#[test]
fn an_id_is_checked_against_the_version_and_not_the_edition() {
    let old = Fixture::new(&source_binding("stone_bricks"));
    let out = compile_as(&old, "bedrock", "1.21.0");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("minecraft:stonebrick"),
        "1.21.0 must point at its own spelling, got: {}",
        stderr_of(&out),
    );

    let new = Fixture::new(&source_binding("stone_bricks"));
    let out = compile_as(&new, "bedrock", "1.21.60");
    assert_eq!(
        out.status.code(),
        Some(0),
        "1.21.60 does declare stone_bricks; stderr: {}",
        stderr_of(&out),
    );
}

/// `@floor.stone.smooth` is valid on both versions, and the pack respells it per version.
#[test]
fn a_material_token_follows_the_rename_across_its_editions_range() {
    for target in ["1.21.0", "1.21.40", "1.21.60"] {
        let fixture = Fixture::new(
            "theme t:\n  slot floor -> @floor.stone.smooth\nstruct s size=2x2\n  floor mat_slot=floor\n",
        );
        let out = compile_as(&fixture, "bedrock", target);
        assert_eq!(
            out.status.code(),
            Some(0),
            "@floor.stone.smooth must resolve on bedrock {target}; stderr: {}",
            stderr_of(&out),
        );
    }
}

#[test]
fn check_stays_silent_about_ids_because_it_does_not_lower() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("check")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = stderr_of(&out);
    assert_stream(&stderr, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
}

#[test]
fn a_walkway_path_id_is_checked_too() {
    let fixture = Fixture::new(&two_huts_joined_by("totally_not_a_block"));
    let out = compile_as(&fixture, "java", "1.21.4");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert_stream(&stderr, &[(13, 35, "error", "E_UNKNOWN_ID")]);
    assert!(
        stderr.contains("minecraft:totally_not_a_block"),
        "expected the path material to be refused, got: {stderr}",
    );

    // The control: the same shape with a real block still builds.
    let ok = Fixture::new(&two_huts_joined_by("gravel"));
    let out = compile_as(&ok, "java", "1.21.4");
    assert_eq!(
        out.status.code(),
        Some(0),
        "a walkway of a real block must compile; stderr: {}",
        stderr_of(&out),
    );
}

/// The slot tests above are the control: the same id on a `slot` is `E_UNKNOWN_ID`.
#[test]
fn a_theme_selector_binding_id_does_not_refuse_the_build() {
    let fixture = Fixture::new(&window_bound_to("totally_not_a_block"));
    let out = compile_as(&fixture, "java", "1.21.4");
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert_stream(&stderr, &[(4, 32, "warning", "W_IGNORED_ARGUMENT")]);
    assert!(
        fixture
            .artifacts()
            .iter()
            .any(|name| std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nbt"))),
        "the structure is written: {:?}",
        fixture.artifacts(),
    );
}

#[test]
fn a_binding_in_a_def_nothing_places_is_not_an_error() {
    let fixture = Fixture::new(
        "theme medieval:\n  slot glass -> @glass_pane\n  slot bad   -> @no_such_slot_block\n  \
         window[class=small] -> frame=@no_such_binding_block\n\n\
         def shed size=9x7:\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=bad\n\n\
         struct cottage size=9x7\n  floor mat_slot=glass\n",
    );
    let source = fixture.source();
    let out = cairn_argv(&[
        "check",
        source.to_str().expect("utf-8 path"),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert_stream(
        &stderr,
        &[
            (4, 32, "warning", "W_IGNORED_ARGUMENT"),
            (6, 1, "warning", "W_UNUSED_DEF"),
        ],
    );
    assert!(
        !stderr.contains("no_such_binding_block"),
        "the binding's value is judged by nothing: {stderr}",
    );
}

/// The `buildable targets:` row `cairn info` prints for the fixture's source.
fn buildable_row(fixture: &Fixture) -> String {
    let source = fixture.source();
    let out = cairn_argv(&["info", source.to_str().expect("utf-8 path")]);
    let stdout = String::from_utf8(out.stdout.clone()).expect("utf-8 stdout");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {}",
        stderr_of(&out),
    );
    stdout
        .lines()
        .find(|line| line.starts_with("buildable targets:"))
        .unwrap_or_else(|| panic!("the row is printed: {stdout}"))
        .to_owned()
}

#[test]
fn a_bad_binding_id_leaves_every_target_buildable() {
    let with = Fixture::new(&window_bound_to("totally_not_a_block"));
    let without = Fixture::new(
        &window_bound_to("totally_not_a_block")
            .replace("  window[class=small] -> frame=@totally_not_a_block\n", ""),
    );
    let (with, without) = (buildable_row(&with), buildable_row(&without));
    assert!(
        !without.contains("none"),
        "the control has buildable targets in both editions: {without}",
    );
    assert_eq!(with, without);
}

#[test]
fn check_pinned_check_and_info_agree_on_a_misspelled_binding_token() {
    let fixture = Fixture::new(
        "theme medieval:\n  slot wall -> @wall.stone.cobble\n  \
         walls[class=outer] -> trim=@wall.stone.cobbl\n\n\
         struct cottage size=9x7\n  walls class=outer mat_slot=wall height=3\n",
    );
    let source = fixture.source();
    let source = source.to_str().expect("utf-8 path");
    let runs: [&[&str]; 3] = [
        &["check", source, "--edition", "java"],
        &["check", source, "--edition", "java", "--target", "1.21.4"],
        &["info", source],
    ];
    for argv in runs {
        let out = cairn_argv(argv);
        let stderr = stderr_of(&out);
        assert_eq!(out.status.code(), Some(0), "{argv:?}: {stderr}");
        assert_stream(&stderr, &[(3, 30, "warning", "W_IGNORED_ARGUMENT")]);
    }
}

/// The README's cottage window, its theme selector row binding `frame=@{id}`, in walls that paint.
fn window_bound_to(id: &str) -> String {
    format!(
        "theme t:\n  slot glass -> @glass_pane\n  slot wall -> @cobblestone\n  \
         window[class=small] -> frame=@{id}\n\n\
         struct s size=9x7\n  \
         walls mat_slot=wall height=5\n  \
         window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass\n"
    )
}

/// Two placed huts and a walkway between them paved with `{path}`.
fn two_huts_joined_by(path: &str) -> String {
    format!(
        "theme t:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\n\
         def hut size=3x3:\n  floor id=f mat_slot=floor\n  \
         walls id=w mat_slot=wall height=2\n  door id=entry side=front at=center\n\n\
         site duo:\n  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n  \
         connect a.entry to b.entry path=@{path}\n"
    )
}

#[test]
fn lower_stays_silent_about_ids_because_it_takes_no_target() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("lower")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = stderr_of(&out);
    assert_stream(&stderr, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("E_UNKNOWN_ID"), "got: {stdout}");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr_of(&out));
}

#[test]
fn info_blames_a_named_target_or_says_nothing_about_ids() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = Command::new(cargo_bin())
        .arg("info")
        .arg(fixture.source())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = stderr_of(&out);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !stdout.contains("E_UNKNOWN_ID"),
        "the rows are about the source and carry no id verdict, got: {stdout}",
    );
    let findings = stderr.matches("E_UNKNOWN_ID").count();
    let attributions = stderr.matches("refuses this source").count();
    assert!(findings > 0, "the fixture's id is in no version: {stderr}");
    assert_eq!(
        findings, attributions,
        "each id finding should sit under the target that raised it: {stderr}",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "info reports; it does not refuse: {stderr}",
    );
}

#[test]
fn an_unresolvable_target_is_still_reported_as_a_target_problem() {
    let fixture = Fixture::new(&source_binding("oak_planks"));
    let out = compile_as(&fixture, "java", "1.99");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("1.99"),
        "expected the unsupported-target message, got: {stderr}",
    );
    assert_stream(&stderr, &[]);
}

/// The slot sits on line 2 of `source_binding`, its `@id` at column 17.
#[test]
fn the_refusal_points_at_the_line_that_names_the_material() {
    let fixture = Fixture::new(&source_binding("totally_not_a_block"));
    let out = compile_as(&fixture, "java", "1.21.4");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("s.crn:2:17: error[E_UNKNOWN_ID]"),
        "expected the slot line and column, got: {stderr}",
    );
}
