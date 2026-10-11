use cairn_lang_core::Edition;

use crate::pipeline::lowered_probe;
use crate::target::resolve_target;

use super::*;

/// Each row pins the reason, which every host shares: on Unix the `\` and
/// `:` rows are single `Normal` components only the textual check refuses.
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

/// No source reaches this, since the resolver refuses such an id with
/// `E_INVALID_PLACE_ID`, so a real lowering is rekeyed. The plain key
/// passes `prepare_artifacts`' other refusals (tag build, directory-entry
/// lookup, which falls back to `std::path::absolute` for a missing `out`,
/// and collision check), so the file name is the only thing left to refuse.
#[test]
fn a_scope_whose_file_name_leaves_out_dir_writes_nothing() {
    let target = resolve_target(EditionArg::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");

    for key in ["site::s::/abs/hut", "site::s::sub/hut", "site::s::a\\b"] {
        assert!(
            prepare_artifacts(&rekeyed_probe(&[key]), &target, out_dir).is_err(),
            "scope `{key}` must be refused rather than written outside `out`",
        );
    }
    let plain = rekeyed_probe(&["site::s::hut"]);
    let prepared = prepare_artifacts(&plain, &target, out_dir)
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

/// The paths `prepare_artifacts` would write, or its refusal, without
/// the tag trees that make a failure message unreadable.
fn prepared_paths(
    result: Result<Vec<(PathBuf, PreparedStructure<'_>)>, PrepareRefusal>,
) -> Result<Vec<PathBuf>, PrepareRefusal> {
    result.map(|prepared| prepared.into_iter().map(|(path, _)| path).collect())
}

/// No source reaches this: the resolver refuses the pair with
/// `E_OUTPUT_NAME_COLLISION`. The control pair shows the same structure
/// prepares under two names that do not meet, so the refusal is the
/// collision's.
#[test]
fn two_scopes_that_name_one_file_are_refused_before_any_io() {
    let target = resolve_target(EditionArg::Java, "latest").expect("the latest target resolves");
    let out_dir = Path::new("out");
    assert!(
        prepare_artifacts(
            &rekeyed_probe(&["struct::hut", "site::s::barn"]),
            &target,
            out_dir
        )
        .is_ok(),
        "the control pair names two files",
    );
    match prepare_artifacts(
        &rekeyed_probe(&["struct::hut", "site::s::hut"]),
        &target,
        out_dir,
    ) {
        Err(PrepareRefusal::Collision(collision)) => {
            assert_eq!(
                (collision.first.as_str(), collision.scope.as_str()),
                ("struct::hut", "site::s::hut")
            );
        }
        other => panic!(
            "`struct::hut` and `site::s::hut` both write `hut.nbt`, so the collision check \
             must refuse them, got {:?}",
            prepared_paths(other),
        ),
    }
}

#[cfg(any(target_os = "macos", windows))]
#[test]
fn two_scopes_whose_names_differ_only_in_case_are_refused_where_case_is_folded() {
    let target = resolve_target(EditionArg::Java, "latest").expect("the latest target resolves");
    match prepare_artifacts(
        &rekeyed_probe(&["struct::hut", "struct::HUT"]),
        &target,
        Path::new("out"),
    ) {
        Err(PrepareRefusal::Collision(collision)) => {
            assert_eq!(
                (collision.first.as_str(), collision.scope.as_str()),
                ("struct::hut", "struct::HUT")
            );
        }
        other => panic!(
            "`hut.nbt` and `HUT.nbt` are one file here, got {:?}",
            prepared_paths(other),
        ),
    }
}
