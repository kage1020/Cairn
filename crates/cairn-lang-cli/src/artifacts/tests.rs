use cairn_lang_core::Edition;

use crate::pipeline::lowered_probe;
use crate::target::resolve_target;

use super::*;

/// Every file name that would leave `--out` is refused, and a plain one
/// joins onto it unchanged.
///
/// The refused rows are the shapes a separator gives a name: absolute
/// (which `Path::join` lets replace `out_dir`), relative with a
/// directory, Windows-style, a drive prefix (on Windows `C:hut.nbt`
/// joins to `C:hut.nbt` and drops `out_dir`) and an alternate data
/// stream, then the names that are not a file at all. Each row pins the
/// reason, not just the refusal, and every host takes the same one: on
/// Unix the `\` and `:` rows are single `Normal` components that only
/// the textual check refuses.
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

/// `prepare_artifacts` routes every scope through [`artifact_path`], so a
/// scope key whose id carries a separator is refused before any I/O.
///
/// No source reaches this: the resolver refuses such an id with
/// `E_INVALID_PLACE_ID` and the lowering pass skips it. Rekeying a real
/// lowering is the only way to ask what the compiler does if a future
/// source of scope keys skips that gate.
///
/// `prepare_artifacts` has three other refusals: the tag build, the
/// directory-entry lookup and the collision check. The plain key below
/// runs the same structure through all three and passes, and the
/// lookup falls back to `std::path::absolute` for a directory that does
/// not exist, so the key's file name is the only thing left to refuse.
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

/// The probe's one lowered structure under each of `keys`, ready to
/// hand to `prepare_artifacts`. Rekeying a real lowering is the only
/// way to reach a refusal no source reaches.
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

/// Two scopes that name one file are refused before any I/O.
///
/// No source reaches this either: the resolver refuses the pair with
/// `E_OUTPUT_NAME_COLLISION`, so `cairn compile` stops before it lowers.
/// The check here is what is left if a future source of scope keys
/// skips that one. The two keys are the shape the resolver's own
/// finding is about, a struct and a placement of one name, and the
/// same structure under two names that do not meet prepares, so the
/// refusal is the collision's.
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

/// Where the file system folds case, two keys whose stems differ only in
/// case are one directory entry, and the guard compares entries, not
/// spellings. On Linux `entry_location` does not fold, so the two are
/// two files there and this is not asserted.
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
