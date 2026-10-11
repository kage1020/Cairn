use super::*;

fn write_pack(tmp: &Path, manifest_json: &str, data_versions_json: &str) {
    std::fs::create_dir_all(tmp).expect("mkdir");
    std::fs::write(tmp.join("pack.json"), manifest_json).expect("write manifest");
    std::fs::write(tmp.join("data_versions.json"), data_versions_json)
        .expect("write data_versions");
}

fn write_pack_with_materials(
    tmp: &Path,
    manifest_json: &str,
    data_versions_json: &str,
    materials_json: &str,
) {
    write_pack(tmp, manifest_json, data_versions_json);
    std::fs::write(tmp.join("materials.json"), materials_json).expect("write materials");
}

fn good_manifest() -> &'static str {
    r#"{
        "schema_version": 1,
        "edition": "java",
        "name": "test",
        "description": "test",
        "files": { "data_versions": "data_versions.json" }
    }"#
}

fn good_data_versions() -> &'static str {
    r#"{
        "schema_version": 1,
        "latest": "1.21.4",
        "versions": [
            { "mc_version": "1.20.4", "data_version": 3700 },
            { "mc_version": "1.21.4", "data_version": 4189 }
        ]
    }"#
}

/// A pack carrying every component, so a test can vary one piece at a
/// time without re-spelling the other three.
fn write_full_pack(
    tmp: &Path,
    manifest_json: &str,
    data_versions_json: &str,
    materials_json: &str,
    blocks_json: &str,
) {
    write_pack_with_materials(tmp, manifest_json, data_versions_json, materials_json);
    std::fs::write(tmp.join("blocks.json"), blocks_json).expect("write blocks");
}

fn manifest_with_blocks() -> &'static str {
    r#"{
        "schema_version": 1,
        "edition": "java",
        "name": "test",
        "description": "test",
        "files": {
            "data_versions": "data_versions.json",
            "materials": "materials.json",
            "blocks": "blocks.json"
        }
    }"#
}

/// One id list per version `good_data_versions` declares, so the
/// coverage check passes and a test that wants to break it says so.
fn good_blocks() -> &'static str {
    r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.20.4", "blocks": ["oak_planks", "stone"] },
        "diffs": [
            { "mc_version": "1.21.4", "inherits": "1.20.4", "added": ["pale_oak_planks"] }
        ]
    }"#
}

/// A pack carrying every component including `aliases`, for the tests
/// that are about renames rather than about ids.
fn write_pack_with_aliases(
    tmp: &Path,
    data_versions_json: &str,
    blocks_json: &str,
    aliases_json: &str,
) {
    write_full_pack(
        tmp,
        manifest_with_aliases(),
        data_versions_json,
        &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
        blocks_json,
    );
    std::fs::write(tmp.join("aliases.json"), aliases_json).expect("write aliases");
}

fn manifest_with_aliases() -> &'static str {
    r#"{
        "schema_version": 1,
        "edition": "java",
        "name": "test",
        "description": "test",
        "files": {
            "data_versions": "data_versions.json",
            "materials": "materials.json",
            "blocks": "blocks.json",
            "aliases": "aliases.json"
        }
    }"#
}

/// One group over `good_blocks`: the id `1.21.4` added, beside a
/// spelling no version of this pack declares. That is the shape of a
/// cross-edition row, and it is answerable because one member is here.
fn good_aliases() -> &'static str {
    r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [{ "spellings": ["pale_oak_planks", "paleoak_planks"] }]
    }"#
}

fn one_material(body: &str) -> String {
    format!(r#"{{ "schema_version": 1, "namespace": "minecraft", "entries": [{body}] }}"#)
}

/// A fresh directory for one pack, removed when the guard drops.
fn pack_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create a temp dir")
}

#[test]
fn a_pack_without_a_blocks_component_loads_and_refutes_no_id() {
    // `blocks` is optional, so a `materials`-only pack must load.
    let pack = load_from_dir_or_die("no-blocks", |dir| {
        write_pack_with_materials(
            dir,
            good_manifest(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
        );
    });
    assert!(pack.blocks.is_empty());
    assert!(pack.view(Some("1.20.4")).block_ids().is_none());
}

#[test]
fn a_blocks_component_folds_one_table_per_version() {
    let pack = load_from_dir_or_die("folded", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            good_blocks(),
        );
    });
    assert_eq!(
        pack.blocks.versions().collect::<Vec<_>>(),
        ["1.20.4", "1.21.4"]
    );
    let old = pack.view(Some("1.20.4"));
    let new = pack.view(Some("1.21.4"));
    let old_ids = old.block_ids().expect("1.20.4 table");
    let new_ids = new.block_ids().expect("1.21.4 table");
    assert!(!old_ids.contains("minecraft:pale_oak_planks"));
    assert!(new_ids.contains("minecraft:pale_oak_planks"));
    assert_eq!(new_ids.label(), "java 1.21.4");
}

#[test]
fn a_view_with_no_version_pinned_carries_no_id_table() {
    let pack = load_from_dir_or_die("unpinned", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            good_blocks(),
        );
    });
    let view = pack.view(None);
    assert!(view.block_ids().is_none());
    // The catalog still resolves — only the id check is off.
    assert!(TargetRegistry::lookup(&view, "a.b").is_some());
}

#[test]
fn a_supported_version_with_no_id_table_is_refused() {
    let err = load_from_dir_err("missing-version", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "base": { "mc_version": "1.20.4", "blocks": ["oak_planks"] }
            }"#,
        );
    });
    let RegistryError::BlocksVersionMismatch { missing, extra } = err else {
        panic!("expected BlocksVersionMismatch, got {err:?}");
    };
    assert_eq!(missing, "1.21.4");
    assert_eq!(extra, "");
}

#[test]
fn an_id_table_for_an_unsupported_version_is_refused() {
    let err = load_from_dir_err("extra-version", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "base": { "mc_version": "1.20.4", "blocks": ["oak_planks"] },
                "diffs": [
                    { "mc_version": "1.21.4", "inherits": "1.20.4", "added": ["stone"] },
                    { "mc_version": "1.99", "inherits": "1.21.4", "added": ["dirt"] }
                ]
            }"#,
        );
    });
    let RegistryError::BlocksVersionMismatch { missing, extra } = err else {
        panic!("expected BlocksVersionMismatch, got {err:?}");
    };
    assert_eq!(missing, "");
    assert_eq!(extra, "1.99");
}

#[test]
fn a_malformed_blocks_table_is_refused_with_its_own_error() {
    let err = load_from_dir_err("bad-fold", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "base": { "mc_version": "1.20.4", "blocks": ["oak_planks"] },
                "diffs": [
                    { "mc_version": "1.21.4", "inherits": "1.20.4", "removed": ["granite"] }
                ]
            }"#,
        );
    });
    assert!(
        matches!(
            err,
            RegistryError::Blocks {
                source: super::super::blocks::BlocksError::RemovedNotInParent { .. }
            }
        ),
        "expected the fold error to surface intact, got {err:?}",
    );
}

#[test]
fn a_material_override_for_an_unsupported_version_is_refused() {
    let err = load_from_dir_err("bad-override", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(
                r#"{ "token": "a.b", "block": "oak_planks",
                     "overrides": [{ "mc_version": "1.21.5", "block": "stone" }] }"#,
            ),
            good_blocks(),
        );
    });
    let RegistryError::MaterialOverrideUnknownVersion { token, mc_version } = err else {
        panic!("expected MaterialOverrideUnknownVersion, got {err:?}");
    };
    assert_eq!(token, "a.b");
    assert_eq!(mc_version, "1.21.5");
}

#[test]
fn a_material_override_changes_the_id_for_that_version_only() {
    let pack = load_from_dir_or_die("override", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(
                r#"{ "token": "a.b", "block": "oak_planks",
                     "overrides": [{ "mc_version": "1.20.4", "block": "stone" }] }"#,
            ),
            good_blocks(),
        );
    });
    assert_eq!(
        TargetRegistry::lookup(&pack.view(Some("1.20.4")), "a.b").map(|s| s.id),
        Some("minecraft:stone".to_owned()),
    );
    assert_eq!(
        TargetRegistry::lookup(&pack.view(Some("1.21.4")), "a.b").map(|s| s.id),
        Some("minecraft:oak_planks".to_owned()),
    );
}

#[test]
fn a_pack_without_an_aliases_component_loads_and_names_no_rename() {
    let pack = load_from_dir_or_die("no-aliases", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#),
            good_blocks(),
        );
    });
    assert!(pack.aliases.is_empty());
    assert!(
        pack.view(Some("1.21.4"))
            .aliases_for("minecraft:paleoak_planks")
            .is_empty()
    );
}

#[test]
fn an_alias_answers_with_the_spellings_the_pinned_version_declares() {
    let pack = load_from_dir_or_die("aliases", |dir| {
        write_pack_with_aliases(dir, good_data_versions(), good_blocks(), good_aliases());
    });
    assert_eq!(pack.aliases.len(), 1);
    // `1.21.4` is the version that added `pale_oak_planks`, so the same
    // group answers there and stays silent on `1.20.4` — the version
    // scoping lives in the block tables, not in the row.
    assert_eq!(
        pack.view(Some("1.21.4"))
            .aliases_for("minecraft:paleoak_planks"),
        ["minecraft:pale_oak_planks".to_owned()],
    );
    assert!(
        pack.view(Some("1.20.4"))
            .aliases_for("minecraft:paleoak_planks")
            .is_empty(),
        "1.20.4 has neither spelling, so the group has nothing to offer it",
    );
}

#[test]
fn a_view_with_no_version_pinned_names_no_rename() {
    let pack = load_from_dir_or_die("aliases-unpinned", |dir| {
        write_pack_with_aliases(dir, good_data_versions(), good_blocks(), good_aliases());
    });
    assert!(
        pack.view(None)
            .aliases_for("minecraft:paleoak_planks")
            .is_empty()
    );
}

#[test]
fn an_alias_group_no_version_declares_is_refused() {
    let err = load_from_dir_err("aliases-dead", |dir| {
        write_pack_with_aliases(
            dir,
            good_data_versions(),
            good_blocks(),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "groups": [{ "spellings": ["standing_sign", "oak_sign"] }]
            }"#,
        );
    });
    let RegistryError::AliasGroupUnanswerable { spellings } = err else {
        panic!("expected AliasGroupUnanswerable, got {err:?}");
    };
    assert_eq!(spellings, "minecraft:standing_sign, minecraft:oak_sign");
}

#[test]
fn a_malformed_alias_catalog_is_refused_with_its_own_error() {
    let err = load_from_dir_err("aliases-bad", |dir| {
        write_pack_with_aliases(
            dir,
            good_data_versions(),
            good_blocks(),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "groups": [{ "spellings": ["pale_oak_planks"] }]
            }"#,
        );
    });
    assert!(
        matches!(
            err,
            RegistryError::Aliases {
                source: super::super::aliases::AliasError::GroupTooSmall { .. }
            }
        ),
        "expected the catalog error to surface intact, got {err:?}",
    );
}

#[test]
fn the_aliases_bytes_reach_the_pack_hash() {
    // The manifest is shared, so only the `aliases` bytes can separate
    // the digests.
    let one = load_from_dir_or_die("alias-hash-one", |dir| {
        write_pack_with_aliases(dir, good_data_versions(), good_blocks(), good_aliases());
    });
    let other = load_from_dir_or_die("alias-hash-other", |dir| {
        write_pack_with_aliases(
            dir,
            good_data_versions(),
            good_blocks(),
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "groups": [{ "spellings": ["pale_oak_planks", "paleoak_plank"] }]
            }"#,
        );
    });
    assert_ne!(one.bytes_hash, other.bytes_hash);
}

#[test]
fn the_blocks_bytes_reach_the_pack_hash() {
    // The manifest is shared, so only the `blocks` bytes can separate
    // the digests; varying the manifest would pass even with `blocks`
    // left out of the hash.
    let materials = one_material(r#"{ "token": "a.b", "block": "oak_planks" }"#);
    let one = load_from_dir_or_die("hash-one", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &materials,
            good_blocks(),
        );
    });
    let other = load_from_dir_or_die("hash-other", |dir| {
        write_full_pack(
            dir,
            manifest_with_blocks(),
            good_data_versions(),
            &materials,
            r#"{
                "schema_version": 1,
                "namespace": "minecraft",
                "base": { "mc_version": "1.20.4", "blocks": ["oak_planks", "dirt"] },
                "diffs": [
                    { "mc_version": "1.21.4", "inherits": "1.20.4", "added": ["stone"] }
                ]
            }"#,
        );
    });
    assert_ne!(one.bytes_hash, other.bytes_hash);
}

/// Write a pack into a fresh directory and load it, panicking on any
/// failure.
fn load_from_dir_or_die(label: &str, write: impl FnOnce(&Path)) -> RegistryPack {
    let dir = pack_dir();
    write(dir.path());
    load_from_dir(dir.path()).unwrap_or_else(|err| panic!("load {label}: {err}"))
}

/// The mirror of [`load_from_dir_or_die`] for the packs that must be
/// refused: panics when the load unexpectedly succeeds.
fn load_from_dir_err(label: &str, write: impl FnOnce(&Path)) -> RegistryError {
    let dir = pack_dir();
    write(dir.path());
    match load_from_dir(dir.path()) {
        Ok(_) => panic!("load {label} was expected to be refused"),
        Err(err) => err,
    }
}

#[test]
fn the_builtin_java_pack_loads() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    assert_eq!(pack.manifest.edition, Edition::Java);
    assert_eq!(pack.manifest.name, "cairn-builtin-java");
    assert_eq!(pack.source, PackSource::Builtin);
    assert!(!pack.data_versions.versions.is_empty());
}

#[test]
fn builtin_pack_includes_materials_catalog() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    assert!(
        !pack.materials.is_empty(),
        "built-in pack ships a non-empty materials catalog",
    );
    assert_eq!(
        pack.materials.lookup_id("floor.wood.broadleaf"),
        Some("minecraft:oak_planks"),
    );
    assert_eq!(
        pack.materials.lookup_id("wall.stone.cobble"),
        Some("minecraft:cobblestone"),
    );
    assert_eq!(
        pack.materials.lookup_id("wood.dark"),
        Some("minecraft:dark_oak_planks"),
    );
    assert_eq!(
        pack.materials.lookup_id("roof.dark_wood"),
        Some("minecraft:dark_oak_stairs"),
    );
}

#[test]
fn pack_without_materials_component_loads_with_empty_index() {
    let dir = pack_dir();
    let tmp = dir.path();
    // `good_manifest()` omits the `materials` key.
    write_pack(tmp, good_manifest(), good_data_versions());
    let pack = load_from_dir(tmp).expect("load");
    assert!(pack.materials.is_empty());
}

#[test]
fn pack_with_materials_component_lifts_tokens() {
    let dir = pack_dir();
    let tmp = dir.path();
    let manifest = r#"{
        "schema_version": 1,
        "edition": "java",
        "name": "test",
        "description": "test",
        "files": {
            "data_versions": "data_versions.json",
            "materials": "materials.json"
        }
    }"#;
    let materials = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "wall.stone.cobble", "block": "cobblestone" }
        ]
    }"#;
    write_pack_with_materials(tmp, manifest, good_data_versions(), materials);
    let pack = load_from_dir(tmp).expect("load");
    assert_eq!(
        pack.materials.lookup_id("wall.stone.cobble"),
        Some("minecraft:cobblestone"),
    );
}

#[test]
fn pack_with_duplicate_materials_token_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    let manifest = r#"{
        "schema_version": 1,
        "edition": "java",
        "name": "test",
        "description": "test",
        "files": {
            "data_versions": "data_versions.json",
            "materials": "materials.json"
        }
    }"#;
    let materials = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [
            { "token": "x.y", "block": "stone" },
            { "token": "x.y", "block": "dirt" }
        ]
    }"#;
    write_pack_with_materials(tmp, manifest, good_data_versions(), materials);
    let err = load_from_dir(tmp).expect_err("duplicate token");
    assert!(matches!(
        err,
        RegistryError::Materials {
            source: MaterialsError::DuplicateMaterialEntry { ref token },
        } if token == "x.y"
    ));
}

#[test]
fn targetable_rows_are_the_buildable_ones_in_data_version_order() {
    let java: Vec<&str> = builtin_pack(Edition::Java)
        .targetable()
        .into_iter()
        .map(|e| e.mc_version.as_str())
        .collect();
    assert_eq!(java, ["1.20.4", "1.21", "1.21.4"]);
    let bedrock: Vec<&str> = builtin_pack(Edition::Bedrock)
        .targetable()
        .into_iter()
        .map(|e| e.mc_version.as_str())
        .collect();
    assert_eq!(bedrock, ["1.21.0", "1.21.40", "1.21.60"]);
}

#[test]
fn targetable_rows_sort_by_data_version_whatever_the_table_order() {
    let mut pack = load_builtin_pack(Edition::Java).expect("builtin java");
    pack.data_versions.versions.reverse();
    let rows: Vec<i32> = pack
        .targetable()
        .into_iter()
        .map(|e| e.data_version)
        .collect();
    assert!(rows.is_sorted(), "{rows:?}");
    assert_eq!(rows.len(), 3);
}

#[test]
fn version_order_carries_every_row_including_the_untargetable_ones() {
    for pack in [builtin_pack(Edition::Java), builtin_pack(Edition::Bedrock)] {
        let order = pack.version_order();
        assert_eq!(order.rows().count(), pack.data_versions.versions.len());
        assert!(pack.data_versions.versions.iter().any(|e| !e.targetable));
        for entry in &pack.data_versions.versions {
            assert_eq!(
                order.key_of(&entry.mc_version),
                Some(i64::from(entry.data_version)),
                "{}",
                entry.mc_version,
            );
        }
    }
}

#[test]
fn builtin_pack_resolves_known_targets() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    let t = pack.resolve_java_target("1.20.4").expect("1.20.4");
    assert_eq!(t.mc_version, "1.20.4");
    assert_eq!(t.data_version, 3700);
    let t = pack.resolve_java_target("1.21").expect("1.21");
    assert_eq!(t.data_version, 3953);
    let t = pack.resolve_java_target("1.21.4").expect("1.21.4");
    assert_eq!(t.data_version, 4189);
}

#[test]
fn builtin_pack_resolves_latest_alias() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    let latest = pack.resolve_java_target("latest").expect("latest");
    assert_eq!(latest.mc_version, pack.data_versions.latest);
}

#[test]
fn builtin_pack_rejects_unknown_target() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    let err = pack
        .resolve_java_target("0.0.0")
        .expect_err("unknown target");
    assert_eq!(err.requested, "0.0.0");
    assert!(err.supported.contains("1.20.4"));
    assert!(err.supported.contains("latest"));
    // `0.0.0` is far from every supported version.
    assert!(err.suggestion.is_none());
}

#[test]
fn builtin_pack_suggests_near_target() {
    // `1.21.5` is one substitution away from `1.21.4`.
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    let err = pack
        .resolve_java_target("1.21.5")
        .expect_err("unknown target");
    assert_eq!(err.suggestion.as_deref(), Some("1.21.4"));
}

/// `--target` reads a version label the way `@requires` and
/// `@intended_targets` do, and the target carries the row's own label.
/// Both editions are covered, since they spell the same release in
/// opposite ways (`1.21` on Java, `1.21.0` on Bedrock).
#[test]
fn either_spelling_of_a_version_names_its_row() {
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    let named = java
        .resolve_java_target("1.21")
        .expect("the row's own label");
    let padded = java
        .resolve_java_target("1.21.0")
        .expect("the padded spelling");
    assert_eq!(padded, named);
    assert_eq!(padded.mc_version, "1.21");

    let bedrock = load_builtin_pack(Edition::Bedrock).expect("bedrock pack");
    let named = bedrock
        .resolve_bedrock_target("1.21.0")
        .expect("the row's own label");
    let trimmed = bedrock
        .resolve_bedrock_target("1.21")
        .expect("the trimmed spelling");
    assert_eq!(trimmed, named);
    assert_eq!(trimmed.mc_version, "1.21.0");
}

/// A leading zero in a numeric component is padding, as the comparison
/// reads it, so `1.021` names Java's `1.21`.
#[test]
fn a_leading_zero_in_a_target_is_padding() {
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    let padded = java.resolve_java_target("1.021").expect("1.021 is 1.21");
    assert_eq!(padded, java.resolve_java_target("1.21").expect("1.21"));
    assert_eq!(padded.mc_version, "1.21");
}

/// A non-zero trailing component is a different version, and a zero
/// digit at the end of a component is part of its number. Bedrock
/// numbers its patch releases in tens, so a rule that stripped trailing
/// `0` characters before comparing would build Bedrock's `1.21.40` for
/// `--target 1.21.4`, another release's palette version.
#[test]
fn a_nonzero_trailing_component_names_a_different_version() {
    let bedrock = load_builtin_pack(Edition::Bedrock).expect("bedrock pack");
    bedrock
        .resolve_bedrock_target("1.21.4")
        .expect_err("1.21.4 is not 1.21.40");
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    java.resolve_java_target("1.21.40")
        .expect_err("1.21.40 is not 1.21.4");
    java.resolve_java_target("1.2")
        .expect_err("1.2 is neither 1.20.4 nor 1.21");
}

/// A row the pack only orders against is not a target in any spelling:
/// resolving `--target` to one would pin a compile to a version with no
/// id table. The premise is asserted first, so the test cannot pass on
/// a pack that dropped the row or made it targetable.
#[test]
fn an_ordering_only_row_is_not_a_target_in_any_spelling() {
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    let bedrock = load_builtin_pack(Edition::Bedrock).expect("bedrock pack");
    for (pack, label) in [(&java, "1.21.1"), (&bedrock, "1.21.20")] {
        assert!(
            pack.data_versions
                .versions
                .iter()
                .any(|e| e.mc_version == label && !e.targetable),
            "`{label}` must be an ordering-only row",
        );
    }
    for spelling in ["1.21.1", "1.21.1.0"] {
        let err = java
            .resolve_java_target(spelling)
            .expect_err("an ordering-only java row");
        assert_eq!(err.requested, spelling);
    }
    for spelling in ["1.21.20", "1.21.20.0"] {
        let err = bedrock
            .resolve_bedrock_target(spelling)
            .expect_err("an ordering-only bedrock row");
        assert_eq!(err.requested, spelling);
    }
}

/// A pre-release is not its release, and an empty component is not a
/// zero one: both stay unequal to the `1.21` row under the comparison.
#[test]
fn a_pre_release_or_an_empty_component_is_not_the_release() {
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    java.resolve_java_target("1.21.0-rc1")
        .expect_err("a pre-release of 1.21");
    java.resolve_java_target("1.21.")
        .expect_err("an empty trailing component");
}

#[test]
fn pack_hash_is_deterministic() {
    let a = load_builtin_pack(Edition::Java).expect("a");
    let b = load_builtin_pack(Edition::Java).expect("b");
    assert_eq!(a.bytes_hash, b.bytes_hash);
}

#[test]
fn the_builtin_bedrock_pack_loads_with_every_java_token() {
    let pack = load_builtin_pack(Edition::Bedrock).expect("builtin pack");
    assert_eq!(pack.manifest.edition, Edition::Bedrock);
    assert_eq!(pack.manifest.name, "cairn-builtin-bedrock");
    assert_eq!(pack.source, PackSource::Builtin);
    assert!(!pack.data_versions.versions.is_empty());
    // The materials catalog must cover the same abstract tokens the
    // Java pack lifts, or a `.crn` that compiles for Java would fail
    // resolution for Bedrock on vocabulary rather than parity.
    let java = load_builtin_pack(Edition::Java).expect("java pack");
    for token in java.materials.tokens() {
        assert!(
            pack.materials.lookup_id(token).is_some(),
            "bedrock pack is missing token `{token}`",
        );
    }
}

#[test]
fn builtin_bedrock_resolves_known_targets_and_latest() {
    // The block-palette `version` integer packs (major, minor, patch,
    // revision) one byte each, major high. Deriving the expected value
    // from the parts pins the JSON table against the documented
    // packing rather than restating the packed integer.
    let version = |major: i32, minor: i32, patch: i32, revision: i32| {
        (major << 24) | (minor << 16) | (patch << 8) | revision
    };
    let pack = load_builtin_pack(Edition::Bedrock).expect("builtin pack");
    // Each targetable row is the block-state version that release
    // writes into a palette (PocketMine-MP's
    // `BlockStateData::CURRENT_VERSION` in the release supporting it),
    // whose revision need not be the client build's (1.21.0.3, 1.21.40.3,
    // 1.21.60.10). The targetable set is pinned with the integers, so a
    // row added or a table regenerated with revision 0 or from build
    // numbers fails here rather than in a user's lock.
    let expected = [
        ("1.21.0", version(1, 21, 0, 3)),
        ("1.21.40", version(1, 21, 40, 1)),
        ("1.21.60", version(1, 21, 60, 33)),
    ];
    let targetable: Vec<&str> = pack
        .data_versions
        .versions
        .iter()
        .filter(|row| row.targetable)
        .map(|row| row.mc_version.as_str())
        .collect();
    assert_eq!(targetable, expected.map(|(mc_version, _)| mc_version));
    // An ordering row never reaches an artifact, so the built-in pack
    // gives it the release's base integer with revision 0. The schema
    // does not require that of a pack; this pins it for this one.
    for row in pack.data_versions.versions.iter().filter(|r| !r.targetable) {
        assert_eq!(row.data_version & 0xFF, 0, "{}", row.mc_version);
    }
    for (mc_version, block_version) in expected {
        let t = pack.resolve_bedrock_target(mc_version).expect(mc_version);
        assert_eq!(t.mc_version, mc_version);
        assert_eq!(t.block_version, block_version, "{mc_version}");
    }
    let latest = pack.resolve_bedrock_target("latest").expect("latest");
    assert_eq!(latest.mc_version, pack.data_versions.latest);
    assert_eq!(latest.block_version, version(1, 21, 60, 33));
}

#[test]
fn builtin_bedrock_unknown_target_names_bedrock_edition() {
    let pack = load_builtin_pack(Edition::Bedrock).expect("builtin pack");
    let err = pack
        .resolve_bedrock_target("1.21.61")
        .expect_err("unknown target");
    assert_eq!(err.edition, Edition::Bedrock);
    assert_eq!(err.suggestion.as_deref(), Some("1.21.60"));
    assert!(err.supported.contains("1.21.60"));
    assert!(
        err.to_string().starts_with("unsupported bedrock target"),
        "error must name the bedrock vocabulary, got: {err}",
    );
}

#[test]
fn bedrock_pack_hash_snapshot() {
    let pack = load_builtin_pack(Edition::Bedrock).expect("builtin pack");
    insta::assert_snapshot!("builtin_bedrock_pack_hash", pack.bytes_hash.as_str());
}

#[test]
fn load_from_dir_reports_missing_manifest() {
    let dir = pack_dir();
    let err = load_from_dir(dir.path()).expect_err("missing manifest");
    assert!(matches!(err, RegistryError::Io { .. }));
}

#[test]
fn load_from_dir_reads_a_complete_pack() {
    let dir = pack_dir();
    let tmp = dir.path();
    write_pack(tmp, good_manifest(), good_data_versions());
    let pack = load_from_dir(tmp).expect("load");
    assert!(matches!(pack.source, PackSource::Path(ref p) if p == tmp));
    let t = pack.resolve_java_target("1.21.4").expect("resolve");
    assert_eq!(t.data_version, 4189);
}

/// Row order in `data_versions.json` is part of the schema, as the
/// `DataVersionTable::versions` doc says: a table whose keys or labels
/// do not ascend row by row is refused at load. Two labels that compare
/// equal do not ascend, which is what lets `targetable_row_for` find at
/// most one row.
#[test]
fn data_versions_rows_out_of_order_are_refused() {
    let keys_descend = r#"{
        "schema_version": 1,
        "latest": "1.21.4",
        "versions": [
            { "mc_version": "1.21.4", "data_version": 4189 },
            { "mc_version": "1.20.4", "data_version": 3700 }
        ]
    }"#;
    let err = load_from_dir_err("order-keys", |dir| {
        write_pack(dir, good_manifest(), keys_descend);
    });
    let RegistryError::VersionOrderBroken { reason } = &err else {
        panic!("expected VersionOrderBroken, got {err}");
    };
    assert!(reason.contains("keys must ascend"), "{reason}");
    let labels_descend = r#"{
        "schema_version": 1,
        "latest": "1.21.4",
        "versions": [
            { "mc_version": "1.21.4", "data_version": 3700 },
            { "mc_version": "1.20.4", "data_version": 4189 }
        ]
    }"#;
    let err = load_from_dir_err("order-labels", |dir| {
        write_pack(dir, good_manifest(), labels_descend);
    });
    let RegistryError::VersionOrderBroken { reason } = &err else {
        panic!("expected VersionOrderBroken, got {err}");
    };
    assert!(reason.contains("does not sort above"), "{reason}");
    // Two spellings of one version. The keys are distinct and ascend, so
    // the key check passes and the label check is the one that refuses:
    // equal keys would trip the key check first.
    let labels_equal = r#"{
        "schema_version": 1,
        "latest": "1.21",
        "versions": [
            { "mc_version": "1.21", "data_version": 3953 },
            { "mc_version": "1.21.0", "data_version": 3954 }
        ]
    }"#;
    let err = load_from_dir_err("order-labels-equal", |dir| {
        write_pack(dir, good_manifest(), labels_equal);
    });
    let RegistryError::VersionOrderBroken { reason } = &err else {
        panic!("expected VersionOrderBroken, got {err}");
    };
    assert!(reason.contains("does not sort above"), "{reason}");
}

#[test]
fn unsupported_manifest_schema_version_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    let bad = r#"{
        "schema_version": 999,
        "edition": "java",
        "name": "x",
        "description": "x",
        "files": { "data_versions": "data_versions.json" }
    }"#;
    write_pack(tmp, bad, good_data_versions());
    let err = load_from_dir(tmp).expect_err("bad schema");
    assert!(matches!(
        err,
        RegistryError::UnsupportedSchemaVersion {
            kind: "manifest",
            got: 999,
            ..
        }
    ));
}

#[test]
fn unsupported_data_versions_schema_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    let bad = r#"{
        "schema_version": 999,
        "latest": "1.21.4",
        "versions": [ { "mc_version": "1.21.4", "data_version": 4189 } ]
    }"#;
    write_pack(tmp, good_manifest(), bad);
    let err = load_from_dir(tmp).expect_err("bad schema");
    assert!(matches!(
        err,
        RegistryError::UnsupportedSchemaVersion {
            kind: "data_versions",
            got: 999,
            ..
        }
    ));
}

#[test]
fn empty_version_table_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    let empty = r#"{ "schema_version": 1, "latest": "1.21.4", "versions": [] }"#;
    write_pack(tmp, good_manifest(), empty);
    let err = load_from_dir(tmp).expect_err("empty");
    assert!(matches!(err, RegistryError::EmptyVersionTable));
}

#[test]
fn latest_not_in_table_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    let dangling = r#"{
        "schema_version": 1,
        "latest": "9.9.9",
        "versions": [ { "mc_version": "1.21.4", "data_version": 4189 } ]
    }"#;
    write_pack(tmp, good_manifest(), dangling);
    let err = load_from_dir(tmp).expect_err("dangling latest");
    assert!(matches!(err, RegistryError::LatestNotInTable { ref latest } if latest == "9.9.9"));
}

#[test]
fn bedrock_manifest_into_java_loader_is_edition_mismatch() {
    let dir = pack_dir();
    let tmp = dir.path();
    let bedrock = r#"{
        "schema_version": 1,
        "edition": "bedrock",
        "name": "x",
        "description": "x",
        "files": { "data_versions": "data_versions.json" }
    }"#;
    write_pack(tmp, bedrock, good_data_versions());
    let err = load_from_dir(tmp).expect_err("mismatch");
    assert!(matches!(
        err,
        RegistryError::EditionMismatch {
            expected: Edition::Java,
            got: Edition::Bedrock,
        }
    ));
}

#[test]
fn malformed_manifest_json_is_error() {
    let dir = pack_dir();
    let tmp = dir.path();
    write_pack(tmp, "{ not json", good_data_versions());
    let err = load_from_dir(tmp).expect_err("bad json");
    assert!(matches!(err, RegistryError::Manifest { .. }));
}

#[test]
fn pack_hash_snapshot() {
    let pack = load_builtin_pack(Edition::Java).expect("builtin pack");
    insta::assert_snapshot!("builtin_java_pack_hash", pack.bytes_hash.as_str());
}
