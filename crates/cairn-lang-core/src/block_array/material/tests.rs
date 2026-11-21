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

struct FakeResolver {
    entries: Vec<(&'static str, &'static str)>,
    /// Empty means no target pinned.
    ids: Vec<String>,
    /// One alias group; empty means no alias table.
    group: Vec<String>,
    /// The ids a later version of the edition, `2.0`, adds.
    later: Vec<String>,
}

impl FakeResolver {
    fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            entries,
            ids: Vec::new(),
            group: Vec::new(),
            later: Vec::new(),
        }
    }

    /// Pin a target declaring exactly `ids`.
    fn pinned(mut self, ids: &[&str]) -> Self {
        self.ids = ids.iter().map(|id| (*id).to_owned()).collect();
        self.ids.sort();
        self
    }

    /// Have version `2.0` add `ids`.
    fn later(mut self, ids: &[&str]) -> Self {
        self.later = ids.iter().map(|id| (*id).to_owned()).collect();
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

    fn declared_elsewhere(&self, id: &str) -> Option<DeclaredElsewhere> {
        if self.later.iter().any(|later| later == id) {
            DeclaredElsewhere::new(Edition::Java, None, Some("2.0".to_owned()))
        } else {
            None
        }
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
        MaterialDeferred::UnknownId(Box::new(UnknownId {
            id: "minecraft:oak_plank".into(),
            registry: "test 1.0".into(),
            origin: IdOrigin::Authored,
            suggestion: Some("minecraft:oak_planks".into()),
            aliases: Vec::new(),
            elsewhere: None,
            absent: None,
        })),
    );
}

#[test]
fn a_rename_is_answered_from_the_alias_table_and_not_by_distance() {
    // `light_block` is six edits from `light`, past `nearest_match`'s cap.
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

#[test]
fn a_misspelling_no_group_names_carries_no_alias() {
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:stonebrick"])
        .aliasing(&["minecraft:stone_bricks", "minecraft:stonebrick"]);
    let err = resolve_block_state(&token("stone_brick"), Some(&registry)).unwrap_err();
    match err {
        MaterialDeferred::UnknownId(unknown) => {
            assert_eq!(unknown.aliases, [] as [String; 0]);
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
        MaterialDeferred::UnknownId(unknown) => assert_eq!(unknown.aliases, [] as [String; 0]),
        other => panic!("expected UnknownId, got {other:?}"),
    }
}

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
fn an_id_a_later_version_declares_is_answered_with_that_version_rather_than_a_spelling() {
    // One edit from a declared id, so only the suppression keeps the spelling out.
    let registry = FakeResolver::new(vec![])
        .pinned(&["minecraft:oak_planks"])
        .later(&["minecraft:oak_plank"]);
    let err = resolve_block_state(&token("oak_plank"), Some(&registry)).unwrap_err();
    assert_eq!(
        err,
        MaterialDeferred::UnknownId(Box::new(UnknownId {
            id: "minecraft:oak_plank".into(),
            registry: "test 1.0".into(),
            origin: IdOrigin::Authored,
            suggestion: None,
            aliases: Vec::new(),
            elsewhere: DeclaredElsewhere::new(Edition::Java, None, Some("2.0".into())),
            absent: None,
        })),
    );
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
        MaterialDeferred::UnknownId(Box::new(UnknownId {
            id: "minecraft:stone_bricks".into(),
            registry: "test 1.0".into(),
            origin: IdOrigin::Catalog {
                token: "floor.stone.smooth".into(),
            },
            suggestion: Some("minecraft:stonebrick".into()),
            aliases: Vec::new(),
            elsewhere: None,
            absent: None,
        })),
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

/// A target declaring `iron_chain` and `water_cauldron`, which spells `chain` `iron_chain`, splits
/// `cauldron` by `fluid`, and whose catalog maps `@chain.link` onto `chain`.
struct Respeller {
    ids: Vec<String>,
}

impl Respeller {
    fn new() -> Self {
        Self {
            ids: vec![
                "minecraft:iron_chain".to_owned(),
                "minecraft:water_cauldron".to_owned(),
            ],
        }
    }
}

impl TargetRegistry for Respeller {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == "chain.link").then(|| BlockState::bare("minecraft:chain"))
    }

    fn known_tokens(&self) -> Vec<String> {
        vec!["chain.link".to_owned()]
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("test 2.0", &self.ids))
    }

    fn meant(&self, state: &BlockState) -> Option<BlockState> {
        let fluid = state.properties.get("fluid")?;
        (state.id == "minecraft:cauldron" && fluid == "water")
            .then(|| BlockState::bare("minecraft:water_cauldron"))
    }

    fn respelled(&self, state: &BlockState) -> Option<BlockState> {
        // Answers for the declared `iron_chain` too, which the lowering must not ask.
        let id = match state.id.as_str() {
            "minecraft:chain" => "minecraft:iron_chain",
            "minecraft:iron_chain" => "minecraft:chain",
            _ => return None,
        };
        Some(BlockState {
            id: id.to_owned(),
            properties: state.properties.clone(),
        })
    }
}

#[test]
fn a_canonical_token_the_target_respells_resolves_to_its_spelling() {
    let registry = Respeller::new();
    let chain = resolve_block_state(&token("chain"), Some(&registry)).unwrap();
    assert_eq!(chain, BlockState::bare("minecraft:iron_chain"));
    let iron_chain = resolve_block_state(&token("iron_chain"), Some(&registry)).unwrap();
    assert_eq!(
        iron_chain,
        BlockState::bare("minecraft:iron_chain"),
        "a declared id is written as itself"
    );
}

#[test]
fn a_meaning_token_resolves_to_the_block_it_picks() {
    let registry = Respeller::new();
    let water = resolve_block_state(&token("cauldron[fluid=water]"), Some(&registry)).unwrap();
    assert_eq!(water, BlockState::bare("minecraft:water_cauldron"));
    let err = resolve_block_state(&token("cauldron[fluid=lava]"), Some(&registry)).unwrap_err();
    let MaterialDeferred::UnknownId(unknown) = err else {
        panic!("a value the target has no block for is an unknown id, got {err:?}");
    };
    assert_eq!(unknown.id, "minecraft:cauldron");
}

#[test]
fn an_abstract_token_is_not_respelled() {
    let registry = Respeller::new();
    let err = resolve_block_state(&token("chain.link"), Some(&registry)).unwrap_err();
    assert!(
        matches!(err, MaterialDeferred::UnknownId(_)),
        "the catalog's id is checked as written, got {err:?}"
    );
}

#[test]
fn the_bound_canonical_tokens_are_each_slot_and_path_once_in_source_order() {
    let source = "theme t:\n  slot floor -> @oak_log[axis=x]\n  slot wall -> @floor.stone.smooth\n  \
                  slot roof -> @chain\n\ndef hut size=3x3:\n  floor mat_slot=floor\n  walls \
                  mat_slot=wall height=2\n  door id=entry side=front at=center\n\nstruct s \
                  size=2x2\n  floor mat_slot=floor\n\nsite pair:\n  place id=a use=hut theme=t \
                  at=origin\n  place id=b use=hut theme=t east_of=a gap=4\n  connect a.entry to \
                  b.entry path=@gravel\n";
    let module = crate::parse(source).expect("the source parses");
    let ir = crate::lower(&module);
    let resolution = crate::resolve(&ir, None);
    let tokens: Vec<(&str, BlockState)> = bound_canonical_tokens(&resolution)
        .into_iter()
        .map(|(span, state)| (&source[span], state))
        .collect();
    let mut log = BlockState::bare("minecraft:oak_log");
    log.properties.insert("axis".to_owned(), "x".to_owned());
    assert_eq!(
        tokens,
        [
            ("@oak_log[axis=x]", log),
            ("@gravel", BlockState::bare("minecraft:gravel")),
        ],
        "`roof` binds no member, and the abstract `wall` is the catalog's",
    );
}
