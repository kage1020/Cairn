use cairn_lang_core::Edition;
use cairn_lang_formats::data_version::resolve_target;

use crate::name_case::NameCase;
use crate::pipeline::lowered_probe;

use super::*;

#[test]
fn an_artifact_file_name_that_is_not_plain_is_refused() {
    let out_dir = Path::new("out");
    for (name, refusal) in [
        ("/abs/hut.nbt", ArtifactNameRefusal::Separator('/')),
        ("sub/hut.nbt", ArtifactNameRefusal::Separator('/')),
        ("a\\b.nbt", ArtifactNameRefusal::Separator('\\')),
        ("C:hut.nbt", ArtifactNameRefusal::Separator(':')),
        ("hut.nbt:ads", ArtifactNameRefusal::Separator(':')),
        ("..", ArtifactNameRefusal::NotAFile),
        (".", ArtifactNameRefusal::NotAFile),
        ("", ArtifactNameRefusal::NotAFile),
    ] {
        assert_eq!(
            artifact_name_refusal(name),
            Some(refusal),
            "`{name}` would not be written directly into `out`, so it must be refused",
        );
        assert!(artifact_path(out_dir, "site::s::probe", name).is_err());
    }
    assert_eq!(artifact_name_refusal("home1.nbt"), None);
    assert_eq!(
        artifact_path(out_dir, "site::s::home1", "home1.nbt").ok(),
        Some(out_dir.join("home1.nbt")),
    );
}

/// The resolver refuses such ids with `E_INVALID_PLACE_ID`, so a real lowering is rekeyed.
#[test]
fn a_scope_whose_file_name_leaves_out_dir_writes_nothing() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");

    for key in ["site::s::/abs/hut", "site::s::sub/hut", "site::s::a\\b"] {
        assert!(
            prepare_artifacts(
                &rekeyed_probe(&[key]),
                Backend::PerArray(&target),
                out_dir,
                NameCase::Sensitive
            )
            .is_err(),
            "scope `{key}` must be refused rather than written outside `out`",
        );
    }
    let plain = rekeyed_probe(&["site::s::hut"]);
    let prepared = prepare_artifacts(
        &plain,
        Backend::PerArray(&target),
        out_dir,
        NameCase::Sensitive,
    )
    .unwrap_or_else(|_| panic!("a plain id prepares"));
    let paths: Vec<&Path> = prepared.iter().map(|(path, _)| path.as_path()).collect();
    assert_eq!(paths, [out_dir.join("hut.nbt").as_path()]);
}

/// The probe's one lowered structure under each of `keys`.
fn rekeyed_probe(keys: &[&str]) -> BlockArrayIr {
    let mut block_ir = lowered_probe(Edition::Java);
    let array = block_ir
        .structures
        .values()
        .next()
        .expect("the probe lowers to one structure")
        .clone();
    block_ir.structures = keys
        .iter()
        .map(|key| ((*key).to_owned(), array.clone()))
        .collect();
    block_ir
}

/// `result` without the structures, which would bury a failure message.
fn prepared_paths(
    result: Result<Vec<(PathBuf, PreparedStructure<'_>)>, PrepareRefusal>,
) -> Result<Vec<PathBuf>, PrepareRefusal> {
    result.map(|prepared| prepared.into_iter().map(|(path, _)| path).collect())
}

/// The resolver refuses such a pair with `E_OUTPUT_NAME_COLLISION`, so a real lowering is rekeyed.
#[test]
fn two_scopes_that_name_one_file_are_refused_before_any_io() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    for case in [NameCase::Sensitive, NameCase::Insensitive] {
        assert!(
            prepare_artifacts(
                &rekeyed_probe(&["struct::hut", "site::s::barn"]),
                Backend::PerArray(&target),
                out_dir,
                case,
            )
            .is_ok(),
            "the control pair names two files where `--out` is {case:?}",
        );
        match prepare_artifacts(
            &rekeyed_probe(&["struct::hut", "site::s::hut"]),
            Backend::PerArray(&target),
            out_dir,
            case,
        ) {
            Err(PrepareRefusal::Collision(collision)) => {
                assert_eq!(
                    (collision.first.as_str(), collision.scope.as_str()),
                    ("struct::hut", "site::s::hut")
                );
            }
            other => panic!(
                "`struct::hut` and `site::s::hut` both write `hut.nbt`, so the collision check \
                 must refuse them where `--out` is {case:?}, got {:?}",
                prepared_paths(other),
            ),
        }
    }
}

#[test]
fn two_scopes_whose_names_differ_only_in_case_collide_where_out_folds_case() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    match prepare_artifacts(
        &rekeyed_probe(&["struct::hut", "struct::HUT"]),
        Backend::PerArray(&target),
        out_dir,
        NameCase::Insensitive,
    ) {
        Err(PrepareRefusal::Collision(collision)) => assert_eq!(
            (
                collision.first.as_str(),
                collision.first_path.as_path(),
                collision.scope.as_str(),
                collision.path.as_path(),
            ),
            (
                "struct::hut",
                out_dir.join("hut.nbt").as_path(),
                "struct::HUT",
                out_dir.join("HUT.nbt").as_path(),
            ),
        ),
        other => panic!(
            "`hut.nbt` and `HUT.nbt` are one file where `--out` folds case, got {:?}",
            prepared_paths(other),
        ),
    }
}

#[test]
fn two_scopes_whose_names_differ_only_in_case_are_two_files_where_out_keeps_case() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    let paths = prepared_paths(prepare_artifacts(
        &rekeyed_probe(&["struct::hut", "struct::HUT"]),
        Backend::PerArray(&target),
        out_dir,
        NameCase::Sensitive,
    ))
    .unwrap_or_else(|refusal| {
        panic!("`hut.nbt` and `HUT.nbt` are two files where `--out` keeps case, got {refusal:?}")
    });
    assert_eq!(paths, [out_dir.join("hut.nbt"), out_dir.join("HUT.nbt")]);
}

/// The lock spellings that are `hut.nbt`, or one of its scratch names, only once case is folded.
const LOCKS_DIFFERING_ONLY_IN_CASE: [&str; 3] = ["HUT.nbt", "Hut.NBT.tmp", "hut.nbt.BAK"];

#[test]
fn a_lock_differing_from_an_artifact_only_in_case_is_refused_only_where_out_folds_case() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    let block_ir = rekeyed_probe(&["struct::hut"]);
    for (case, expected) in [
        (NameCase::Insensitive, Err(ExitCode::from(1))),
        (NameCase::Sensitive, Ok(())),
    ] {
        let prepared = prepare_artifacts(&block_ir, Backend::PerArray(&target), out_dir, case)
            .unwrap_or_else(|_| panic!("one struct prepares"));
        for lock in LOCKS_DIFFERING_ONLY_IN_CASE {
            assert_eq!(
                check_lock_path_is_free(&prepared, &out_dir.join(lock), case),
                expected,
                "`--lock out/{lock}` where `--out` is {case:?}",
            );
        }
        assert_eq!(
            check_lock_path_is_free(&prepared, &out_dir.join("hut.nbt"), case),
            Err(ExitCode::from(1)),
            "the artifact's own spelling is refused where `--out` is {case:?}",
        );
    }
}

#[test]
fn a_lower_case_lock_beside_an_upper_case_artifact_is_refused_where_out_folds_case() {
    let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    let block_ir = rekeyed_probe(&["struct::HUT"]);
    for (case, expected) in [
        (NameCase::Insensitive, Err(ExitCode::from(1))),
        (NameCase::Sensitive, Ok(())),
    ] {
        let prepared = prepare_artifacts(&block_ir, Backend::PerArray(&target), out_dir, case)
            .unwrap_or_else(|_| panic!("one struct prepares"));
        for lock in ["hut.nbt", "hut.nbt.tmp", "Hut.nbt.bak"] {
            assert_eq!(
                check_lock_path_is_free(&prepared, &out_dir.join(lock), case),
                expected,
                "`--lock out/{lock}` beside `HUT.nbt` where `--out` is {case:?}",
            );
        }
    }
}

/// The Java target `latest` resolves to.
fn latest_java() -> ResolvedTarget {
    resolve_target(Edition::Java, "latest").expect("the latest target resolves")
}

fn litematic(target: &ResolvedTarget) -> Backend<'_> {
    let ResolvedTarget::Java(java) = target else {
        panic!("a Java target");
    };
    Backend::Litematic(java)
}

#[test]
fn a_litematic_whose_file_name_leaves_out_dir_writes_nothing() {
    let target = latest_java();
    let out_dir = Path::new("out");
    for key in ["struct::/abs/hut", "struct::sub/hut", "struct::a\\b"] {
        assert!(
            prepare_artifacts(
                &rekeyed_probe(&[key]),
                litematic(&target),
                out_dir,
                NameCase::Sensitive
            )
            .is_err(),
            "scope `{key}` must be refused rather than written outside `out`",
        );
    }
    let paths = prepared_paths(prepare_artifacts(
        &rekeyed_probe(&["struct::hut"]),
        litematic(&target),
        out_dir,
        NameCase::Sensitive,
    ))
    .unwrap_or_else(|refusal| panic!("a plain name prepares, got {refusal:?}"));
    assert_eq!(paths, [out_dir.join("hut.litematic")]);
}

#[test]
fn two_litematics_whose_names_differ_only_in_case_collide_on_every_host() {
    let target = latest_java();
    let out_dir = Path::new("out");
    let block_ir = rekeyed_probe(&["struct::hut", "struct::HUT"]);
    for case in [NameCase::Insensitive, NameCase::Sensitive] {
        match prepare_artifacts(&block_ir, litematic(&target), out_dir, case) {
            Err(PrepareRefusal::Collision(collision)) => assert_eq!(
                (
                    collision.first.as_str(),
                    collision.first_path.as_path(),
                    collision.scope.as_str(),
                    collision.path.as_path(),
                    collision.litematic,
                ),
                (
                    "struct::hut",
                    out_dir.join("hut.litematic").as_path(),
                    "struct::HUT",
                    out_dir.join("HUT.litematic").as_path(),
                    true,
                ),
            ),
            other => panic!(
                "`hut.litematic` and `HUT.litematic` collide whatever `--out` folds, got {:?}",
                prepared_paths(other),
            ),
        }
    }
}

#[test]
fn a_site_a_litematic_cannot_hold_exits_1_before_any_io() {
    let target = latest_java();
    let mut block_ir = rekeyed_probe(&["site::s::far"]);
    let dims = block_ir.structures["site::s::far"].dims;
    block_ir.placements.insert(
        "site::s::far".to_owned(),
        cairn_lang_core::block_array::Placement {
            site: cairn_lang_core::SiteName::new("s").expect("a site name"),
            place_id: cairn_lang_core::PlaceId::new("far").expect("a place id"),
            source_def: "d".to_owned(),
            theme: "t".to_owned(),
            origin: (i32::MAX, 0, 0),
            dims,
        },
    );
    match prepare_artifacts(
        &block_ir,
        litematic(&target),
        Path::new("out"),
        NameCase::Sensitive,
    ) {
        Err(PrepareRefusal::Other(code)) => assert_eq!(code, ExitCode::from(1)),
        other => panic!(
            "a region past i32 is refused with exit 1, got {:?}",
            prepared_paths(other)
        ),
    }
}

#[test]
fn a_litematic_refusal_another_format_avoids_says_so() {
    let corner = LitematicError::CornerOverflow {
        region: "far".to_owned(),
        axis: "x",
        value: i64::from(i32::MAX) + 1,
    };
    let metadata = LitematicError::MetadataOverflow {
        field: "TotalVolume",
        value: i64::from(i32::MAX) + 1,
    };
    let region = LitematicError::Region {
        region: "hut".to_owned(),
        source: Box::new(
            cairn_lang_formats::java_structure::JavaStructureError::AbstractPaletteEntry {
                id: "oak_planks".to_owned(),
            },
        ),
    };
    assert_eq!(
        [corner, metadata, region].map(|refusal| litematic_fix(&refusal)),
        [
            Some(
                "move the placement nearer the site's origin, or compile with \
                 `--structure-format nbt`, which records only where each file's minimum corner \
                 sits"
            ),
            Some(
                "split the site into smaller ones, or compile with `--structure-format nbt`, \
                 which writes a file per placement and walkway"
            ),
            None,
        ]
    );
}
