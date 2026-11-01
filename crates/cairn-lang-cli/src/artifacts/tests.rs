use cairn_lang_core::Edition;

use crate::name_case::NameCase;
use crate::pipeline::lowered_probe;
use crate::target::resolve_target;

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
                &target,
                out_dir,
                NameCase::Sensitive
            )
            .is_err(),
            "scope `{key}` must be refused rather than written outside `out`",
        );
    }
    let plain = rekeyed_probe(&["site::s::hut"]);
    let prepared = prepare_artifacts(&plain, &target, out_dir, NameCase::Sensitive)
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
                &target,
                out_dir,
                case,
            )
            .is_ok(),
            "the control pair names two files where `--out` is {case:?}",
        );
        match prepare_artifacts(
            &rekeyed_probe(&["struct::hut", "site::s::hut"]),
            &target,
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
        &target,
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
        &target,
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
        let prepared = prepare_artifacts(&block_ir, &target, out_dir, case)
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
        let prepared = prepare_artifacts(&block_ir, &target, out_dir, case)
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
