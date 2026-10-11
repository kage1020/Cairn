use super::*;
use crate::ast::Value;

fn token(inner: &str) -> ValueWithSpan {
    let span = 0..inner.len() + 1; // account for the leading `@`
    ValueWithSpan::from_value(Value::new(ValueKind::Token(inner.to_owned()), span))
}

fn ident(name: &str) -> ValueWithSpan {
    let span = 0..name.len();
    ValueWithSpan::from_value(Value::new(ValueKind::Ident(name.to_owned()), span))
}

/// An in-memory [`TargetRegistry`]; the real one is in
/// `cairn-lang-formats`, which depends on this crate.
struct FakeResolver {
    entries: Vec<(&'static str, &'static str)>,
    /// Sorted, fully namespaced ids the pinned target declares. Empty
    /// means "no target pinned", matching a `blocks`-less pack.
    ids: Vec<String>,
    /// One alias group: every spelling of one block. Empty means no alias
    /// table.
    group: Vec<String>,
}

impl FakeResolver {
    fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            entries,
            ids: Vec::new(),
            group: Vec::new(),
        }
    }

    /// Pin a target declaring exactly `ids`.
    fn pinned(mut self, ids: &[&str]) -> Self {
        self.ids = ids.iter().map(|id| (*id).to_owned()).collect();
        self.ids.sort();
        self
    }

    /// Declare one alias group, in the order a pack wrote it.
    fn aliasing(mut self, group: &[&str]) -> Self {
        self.group = group.iter().map(|id| (*id).to_owned()).collect();
        self
    }
}

impl TargetRegistry for FakeResolver {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.entries
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        (!self.ids.is_empty()).then(|| BlockIdSet::new("test 1.0", &self.ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        if !self.group.iter().any(|spelling| spelling == id) {
            return Vec::new();
        }
        self.group
            .iter()
            .filter(|spelling| self.ids.binary_search(spelling).is_ok())
            .cloned()
            .collect()
    }
}

#[test]
fn canonical_bare_token_becomes_minecraft_namespaced() {
    let bs = resolve_block_state(&token("cobblestone"), None).unwrap();
    assert_eq!(bs.id, "minecraft:cobblestone");
    assert!(bs.properties.is_empty());
}

#[test]
fn canonical_state_literal_populates_properties() {
    let bs = resolve_block_state(&token("oak_log[axis=x]"), None).unwrap();
    assert_eq!(bs.id, "minecraft:oak_log");
    assert_eq!(bs.properties.get("axis").map(String::as_str), Some("x"));
}

#[test]
fn canonical_state_literal_with_multiple_pairs() {
    let bs = resolve_block_state(&token("stairs[facing=north,half=top]"), None).unwrap();
    assert_eq!(bs.id, "minecraft:stairs");
    assert_eq!(bs.properties.len(), 2);
    assert_eq!(bs.properties.get("facing").unwrap(), "north");
    assert_eq!(bs.properties.get("half").unwrap(), "top");
}

#[test]
fn canonical_explicit_namespace_is_preserved() {
    let bs = resolve_block_state(&token("create:cogwheel"), None).unwrap();
    assert_eq!(bs.id, "create:cogwheel");
}

#[test]
fn abstract_token_without_resolver_defers() {
    let err = resolve_block_state(&token("floor.wood.broadleaf"), None).unwrap_err();
    assert_eq!(
        err,
        MaterialDeferred::Abstract("floor.wood.broadleaf".into())
    );
}

#[test]
fn abstract_token_lifts_through_resolver() {
    let resolver = FakeResolver::new(vec![
        ("floor.wood.broadleaf", "oak_planks"),
        ("wall.stone.cobble", "cobblestone"),
    ]);
    let bs = resolve_block_state(&token("floor.wood.broadleaf"), Some(&resolver)).unwrap();
    assert_eq!(bs.id, "minecraft:oak_planks");
}

#[test]
fn abstract_token_unknown_returns_suggestion() {
    let resolver = FakeResolver::new(vec![
        ("floor.wood.broadleaf", "oak_planks"),
        ("floor.wood.conifer", "spruce_planks"),
    ]);
    let err = resolve_block_state(&token("floor.wood.broadlef"), Some(&resolver)).unwrap_err();
    assert_eq!(
        err,
        MaterialDeferred::UnknownAbstract {
            token: "floor.wood.broadlef".into(),
            suggestion: Some("floor.wood.broadleaf".into()),
        },
    );
}

#[test]
fn abstract_token_unknown_without_close_candidate_has_no_suggestion() {
    let resolver = FakeResolver::new(vec![("floor.wood.broadleaf", "oak_planks")]);
    let err = resolve_block_state(&token("totally.different.tree"), Some(&resolver)).unwrap_err();
    match err {
        MaterialDeferred::UnknownAbstract { suggestion, .. } => assert!(suggestion.is_none()),
        other => panic!("expected UnknownAbstract, got {other:?}"),
    }
}

#[test]
fn non_token_value_is_already_diagnosed() {
    let err = resolve_block_state(&ident("plain"), None).unwrap_err();
    assert_eq!(err, MaterialDeferred::AlreadyDiagnosed);
}

#[test]
fn an_authored_id_the_target_lacks_is_refused_with_its_registry_named() {
    let registry = FakeResolver::new(vec![]).pinned(&["minecraft:oak_planks"]);
    let err = resolve_block_state(&token("oak_plank"), Some(&registry)).unwrap_err();
    assert_eq!(
        err,
        MaterialDeferred::UnknownId(UnknownId {
            id: "minecraft:oak_plank".into(),
            registry: "test 1.0".into(),
            origin: IdOrigin::Authored,
            suggestion: Some("minecraft:oak_planks".into()),
            aliases: Vec::new(),
        }),
    );
}

#[test]
fn a_rename_is_answered_from_the_alias_table_and_not_by_distance() {
    // Bedrock 1.21.0 spells Java's `light` as `light_block`, six edits
    // away and past `nearest_match`'s cap.
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:light_block"])
        .aliasing(&["minecraft:light", "minecraft:light_block"]);
    let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert_eq!(unknown.id, "minecraft:light");
            assert_eq!(unknown.aliases, ["minecraft:light_block".to_owned()]);
            assert!(
                unknown.suggestion.is_none(),
                "the distance search still has nothing to say about a rename",
            );
        }
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// Bedrock 1.21.40 has a `light_block_N` per light level; offering one
/// would choose the light level for the author.
#[test]
fn a_split_answers_with_every_spelling_the_target_has() {
    let registry = FakeResolver::new(vec![])
        .pinned(&[
            "minecraft:light_block_0",
            "minecraft:light_block_1",
            "minecraft:light_block_2",
        ])
        .aliasing(&[
            "minecraft:light",
            "minecraft:light_block_0",
            "minecraft:light_block_1",
            "minecraft:light_block_2",
        ]);
    let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => assert_eq!(
            unknown.aliases,
            [
                "minecraft:light_block_0".to_owned(),
                "minecraft:light_block_1".to_owned(),
                "minecraft:light_block_2".to_owned(),
            ],
        ),
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// An alias group spans versions, so it holds spellings this target
/// refuses too.
#[test]
fn an_alias_the_target_lacks_is_not_offered() {
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:cobblestone"])
        .aliasing(&["minecraft:light", "minecraft:light_block"]);
    let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => assert!(
            unknown.aliases.is_empty(),
            "`light_block` is no more declared here than `light` is",
        ),
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// `stone_bricks` is in the group and also two edits from `stonebrick`,
/// so each field is filled, by its own rule.
#[test]
fn a_renamed_id_near_its_replacement_carries_both_answers() {
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:stonebrick"])
        .aliasing(&["minecraft:stone_bricks", "minecraft:stonebrick"]);
    let err = resolve_block_state(&token("stone_bricks"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert_eq!(unknown.aliases, ["minecraft:stonebrick".to_owned()]);
            assert_eq!(unknown.suggestion.as_deref(), Some("minecraft:stonebrick"));
        }
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// `stone_brick` is one edit from the group's `stone_bricks`, but an
/// alias is group membership, not distance.
#[test]
fn a_misspelling_no_group_names_carries_no_alias() {
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:stonebrick"])
        .aliasing(&["minecraft:stone_bricks", "minecraft:stonebrick"]);
    let err = resolve_block_state(&token("stone_brick"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert!(unknown.aliases.is_empty());
            assert_eq!(unknown.suggestion.as_deref(), Some("minecraft:stonebrick"));
        }
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

#[test]
fn a_registry_with_no_alias_table_answers_no_alias() {
    let registry = FakeResolver::new(vec![]).pinned(&["minecraft:light_block"]);
    let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => assert!(unknown.aliases.is_empty()),
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// A path search across namespaces would find `cogwheels` and offer
/// `minecraft:cogwheels`, an id no registry has. Only a modded table
/// reaches this: both built-in packs are all `minecraft:`.
#[test]
fn a_suggestion_never_borrows_a_path_from_another_namespace() {
    let registry = FakeResolver::new(vec![]).pinned(&["create:cogwheels"]);
    let err = resolve_block_state(&token("cogwheel"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert_eq!(unknown.id, "minecraft:cogwheel");
            assert_eq!(
                unknown.suggestion, None,
                "`create:cogwheels` is not a candidate for a `minecraft:` id, and \
                 `minecraft:cogwheels` is not an id at all",
            );
        }
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

/// The control for the test above: the same table does suggest within
/// its own namespace.
#[test]
fn a_suggestion_is_still_found_inside_the_namespace_that_has_one() {
    let registry = FakeResolver::new(vec![]).pinned(&["create:cogwheels"]);
    let err = resolve_block_state(&token("create:cogwheel"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert_eq!(unknown.suggestion.as_deref(), Some("create:cogwheels"));
        }
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

#[test]
fn an_authored_id_the_target_declares_passes() {
    let registry = FakeResolver::new(vec![]).pinned(&["minecraft:light"]);
    let bs = resolve_block_state(&token("light"), Some(&registry)).unwrap();
    assert_eq!(bs.id, "minecraft:light");
}

#[test]
fn a_state_literal_is_checked_on_the_id_alone() {
    let registry = FakeResolver::new(vec![]).pinned(&["minecraft:oak_log"]);
    let bs = resolve_block_state(&token("oak_log[axis=x]"), Some(&registry)).unwrap();
    assert_eq!(bs.properties.get("axis").map(String::as_str), Some("x"));
}

#[test]
fn a_catalog_mapping_onto_a_missing_id_names_the_token_that_reached_it() {
    let registry = FakeResolver::new(vec![("floor.stone.smooth", "stone_bricks")])
        .pinned(&["minecraft:stonebrick"]);
    let err = resolve_block_state(&token("floor.stone.smooth"), Some(&registry)).unwrap_err();
    assert_eq!(
        err,
        MaterialDeferred::UnknownId(UnknownId {
            id: "minecraft:stone_bricks".into(),
            registry: "test 1.0".into(),
            origin: IdOrigin::Catalog {
                token: "floor.stone.smooth".into(),
            },
            suggestion: Some("minecraft:stonebrick".into()),
            aliases: Vec::new(),
        }),
    );
}

#[test]
fn an_unpinned_registry_refutes_nothing() {
    let registry = FakeResolver::new(vec![("floor.stone.smooth", "stone_bricks")]);
    assert!(registry.block_ids().is_none());
    let bs = resolve_block_state(&token("totally_not_a_block"), Some(&registry)).unwrap();
    assert_eq!(bs.id, "minecraft:totally_not_a_block");
}

#[test]
fn an_unknown_id_with_no_close_candidate_carries_no_suggestion() {
    let registry = FakeResolver::new(vec![]).pinned(&["minecraft:stone"]);
    let err = resolve_block_state(&token("totally_not_a_block"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => assert!(unknown.suggestion.is_none()),
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

#[test]
fn every_id_origin_has_a_wire_name_and_the_token_that_goes_with_it() {
    let cases = [
        (IdOrigin::Authored, "authored", None),
        (
            IdOrigin::Catalog {
                token: "floor.stone.smooth".to_owned(),
            },
            "catalog",
            Some("floor.stone.smooth"),
        ),
        (
            IdOrigin::Builtin {
                token: "pressure_plate.default",
            },
            "builtin",
            Some("pressure_plate.default"),
        ),
    ];
    for (origin, kind, token) in cases {
        assert_eq!(origin.kind(), kind);
        assert_eq!(origin.token(), token);
    }
}
