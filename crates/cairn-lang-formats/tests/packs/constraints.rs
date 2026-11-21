//! What the built-in constraint catalog says through a pinned pack view: the blocks the game drops
//! or lets fall, the faces that hold them and keep water in, and the members whose meaning moves at
//! a boundary version (`spec/lint` "Constraint checks").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockState, Constraint, Contents, Face, Support, TargetRegistry, Vertical,
    lower_to_block_array,
};
use cairn_lang_core::resolve::{SemanticSensitiveFinding, resolve};
use cairn_lang_core::{DiagnosticCode, Edition, lower, parse};
use cairn_lang_formats::registry::{
    PackView, builtin_constraints, builtin_pack, builtin_view, parse_constraints,
};

use crate::common::lower_source;

/// `minecraft:{id}` with `properties`.
fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: format!("minecraft:{id}"),
        properties: properties
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

/// The built-in view of `edition` pinned at `version`, with the built-in catalog's checks.
fn pinned(edition: Edition, version: &'static str) -> PackView<'static> {
    builtin_pack(edition)
        .view(Some(version))
        .with_constraints(builtin_constraints())
}

/// A pinned target of each edition.
const TARGETS: [(Edition, &str); 2] = [(Edition::Java, "26.3"), (Edition::Bedrock, "1.26.60")];

#[test]
fn an_attached_block_hangs_on_the_face_its_state_turns_away_from() {
    let view = pinned(Edition::Java, "26.3");
    let hangs = |id, properties: &[(&str, &str)]| view.constraint(&state(id, properties));
    let host = |host| Some(Constraint::Hangs { host });
    let lever = |face, facing| hangs("lever", &[("face", face), ("facing", facing)]);
    assert_eq!(lever("wall", "south"), host(Face::North));
    assert_eq!(lever("floor", "south"), host(Face::Down));
    assert_eq!(lever("ceiling", "south"), host(Face::Up));
    assert_eq!(hangs("wall_torch", &[("facing", "east")]), host(Face::West));
    assert_eq!(hangs("ladder", &[("facing", "north")]), host(Face::South));
    assert_eq!(
        hangs("stone_button", &[("face", "wall"), ("facing", "west")]),
        host(Face::East)
    );
}

#[test]
fn a_standing_block_rests_on_the_face_its_row_names_and_a_gravity_block_falls() {
    let view = pinned(Edition::Java, "26.3");
    let constraint = |id, properties: &[(&str, &str)]| view.constraint(&state(id, properties));
    let rests = |host, needs| Some(Constraint::Rests { host, needs });
    assert_eq!(
        constraint("torch", &[]),
        rests(Vertical::Below, Support::Center)
    );
    assert_eq!(
        constraint("lantern", &[("hanging", "false")]),
        rests(Vertical::Below, Support::Center)
    );
    assert_eq!(
        constraint("lantern", &[("hanging", "true")]),
        rests(Vertical::Above, Support::Center)
    );
    assert_eq!(
        constraint("oak_hanging_sign", &[("attached", "false")]),
        rests(Vertical::Above, Support::Center)
    );
    assert_eq!(
        constraint("redstone_wire", &[]),
        rests(Vertical::Below, Support::Sturdy)
    );
    assert_eq!(
        constraint("oak_door", &[("half", "lower")]),
        rests(Vertical::Below, Support::Sturdy)
    );
    assert_eq!(constraint("oak_door", &[("half", "upper")]), None);
    assert_eq!(constraint("sand", &[]), Some(Constraint::Falls));
    assert_eq!(constraint("anvil", &[]), Some(Constraint::Falls));
    assert_eq!(
        constraint("oak_sign", &[]),
        None,
        "a sign needs a solid block"
    );
    assert_eq!(constraint("stone", &[]), None);
}

#[test]
fn a_lever_hangs_on_a_full_block_or_glass_and_not_on_a_pane() {
    let lever = state("lever", &[("face", "wall"), ("facing", "south")]);
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        let on = |host| view.holds(&lever, &state(host, &[]));
        assert_eq!(on("cobblestone"), Some(true), "{version}");
        assert_eq!(on("glass"), Some(true), "{version}");
        assert_eq!(on("glass_pane"), Some(false), "{version}");
        assert_eq!(on("oak_fence"), Some(false), "{version}");
    }
}

#[test]
fn a_torch_stands_on_a_post_or_an_upright_chain_and_not_on_a_lying_one() {
    let torch = state("torch", &[]);
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        let on = |host, properties: &[(&str, &str)]| view.holds(&torch, &state(host, properties));
        assert_eq!(on("oak_fence", &[]), Some(true), "{version}");
        assert_eq!(on("cobblestone_wall", &[]), Some(true), "{version}");
        assert_eq!(on("glass_pane", &[]), Some(true), "{version}");
        assert_eq!(on("iron_chain", &[("axis", "y")]), Some(true), "{version}");
        assert_eq!(on("iron_chain", &[("axis", "x")]), Some(false), "{version}");
    }
    let older = pinned(Edition::Java, "1.20.4");
    assert_eq!(
        older.holds(&torch, &state("chain", &[("axis", "y")])),
        Some(true)
    );
}

#[test]
fn a_lantern_hangs_under_an_upright_chain_and_a_repeater_does_not_stand_on_a_post() {
    let lantern = state("lantern", &[("hanging", "true")]);
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        let repeater = match edition {
            Edition::Java => state("repeater", &[]),
            Edition::Bedrock => state("unpowered_repeater", &[]),
        };
        assert_eq!(
            view.holds(&lantern, &state("iron_chain", &[("axis", "y")])),
            Some(true),
            "{version}"
        );
        assert_eq!(
            view.holds(&repeater, &state("oak_fence", &[])),
            Some(false),
            "{version}"
        );
    }
}

#[test]
fn a_closed_fence_gate_gives_a_centre_and_an_open_one_none() {
    let torch = state("torch", &[]);
    let gate = |open| {
        state(
            "spruce_fence_gate",
            &[
                ("facing", "north"),
                ("in_wall", "false"),
                ("open", open),
                ("powered", "false"),
            ],
        )
    };
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        assert_eq!(view.holds(&torch, &gate("false")), Some(true), "{version}");
        assert_eq!(view.holds(&torch, &gate("true")), Some(false), "{version}");
    }
}

#[test]
fn redstone_dust_rests_on_a_hopper_and_a_repeater_does_not() {
    let hopper = state("hopper", &[("enabled", "true"), ("facing", "down")]);
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        let repeater = match edition {
            Edition::Java => state("repeater", &[]),
            Edition::Bedrock => state("unpowered_repeater", &[]),
        };
        assert_eq!(
            view.holds(&state("redstone_wire", &[]), &hopper),
            Some(true),
            "{version}"
        );
        assert_eq!(view.holds(&repeater, &hopper), Some(false), "{version}");
        assert_eq!(
            view.holds(&repeater, &state("stone", &[])),
            Some(true),
            "{version}"
        );
    }
}

#[test]
fn water_is_kept_in_by_a_whole_face_only() {
    for (edition, version) in TARGETS {
        let view = pinned(edition, version);
        let keeps = |id, face| view.keeps_water(&state(id, &[]), face);
        assert_eq!(keeps("stone", Face::North), Some(true), "{version}");
        assert_eq!(keeps("glass_pane", Face::North), Some(false), "{version}");
        assert_eq!(keeps("glass_pane", Face::Down), Some(false), "{version}");
        for face in [Face::Down, Face::North, Face::East, Face::South, Face::West] {
            assert_eq!(keeps("water", face), Some(false), "{version} {face:?}");
        }
    }
}

#[test]
fn a_cell_holds_what_its_canonical_blocks_are() {
    let java = pinned(Edition::Java, "26.3");
    let bedrock = pinned(Edition::Bedrock, "1.26.60");
    let holds = |view: &PackView<'_>, id| view.contents(&state(id, &[]));
    for id in ["air", "cave_air", "void_air"] {
        assert_eq!(holds(&java, id), Some(Contents::Air), "{id}");
    }
    assert_eq!(holds(&java, "water"), Some(Contents::Water));
    assert_eq!(holds(&java, "lava"), Some(Contents::Lava));
    assert_eq!(holds(&java, "stone"), Some(Contents::Block));
    assert_eq!(holds(&bedrock, "air"), Some(Contents::Air));
    for id in ["water", "flowing_water"] {
        assert_eq!(holds(&bedrock, id), Some(Contents::Water), "{id}");
    }
    for id in ["lava", "flowing_lava"] {
        assert_eq!(holds(&bedrock, id), Some(Contents::Lava), "{id}");
    }
    assert_eq!(holds(&bedrock, "stone"), Some(Contents::Block));
}

/// `source` lowered at `version` of `edition` through [`builtin_view`].
fn judged(source: &str, edition: Edition, version: &str) -> BlockArrayIr {
    let module = parse(source).expect("the source parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_view(edition, Some(version))),
    )
}

#[test]
fn bedrock_flowing_water_and_lava_are_judged_as_water_and_lava() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=5x4\n  \
                  walls mat_slot=wall height=3\n  \
                  raw_block mat=minecraft:flowing_water at=2,1,1\n  \
                  raw_block mat=minecraft:flowing_lava at=1,0,1\n  \
                  raw_block mat=minecraft:sand at=1,1,1\n";
    let found: Vec<(DiagnosticCode, String)> = judged(source, Edition::Bedrock, "1.26.60")
        .diagnostics
        .into_iter()
        .map(|d| (d.code, d.primary))
        .collect();
    assert_eq!(
        found,
        [
            (
                DiagnosticCode::WaterSpill,
                "`raw_block` holds water that flows into 3 cells of air: the first, at 2,0,1, from \
                 `minecraft:flowing_water` at 2,1,1"
                    .to_owned(),
            ),
            (
                DiagnosticCode::UnsupportedBlock,
                "`raw_block` builds a block that falls: `minecraft:sand` at 1,1,1 stands on \
                 `minecraft:flowing_lava`"
                    .to_owned(),
            ),
        ],
    );
}

#[test]
fn an_unpinned_view_runs_no_check_and_a_view_without_the_catalog_none() {
    let lever = state("lever", &[("face", "wall"), ("facing", "south")]);
    let unpinned = builtin_view(Edition::Java, None);
    let bare = builtin_pack(Edition::Java).view(Some("26.3"));
    for view in [unpinned, bare] {
        assert_eq!(view.constraint(&lever), None);
        assert_eq!(view.holds(&lever, &state("glass_pane", &[])), None);
        assert_eq!(
            view.keeps_water(&state("glass_pane", &[]), Face::North),
            None
        );
    }
}

#[test]
fn the_members_a_java_boundary_moves_are_semantic_sensitive_once_each() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  \
                  walls mat_slot=wall height=3\n  \
                  raw_block mat=minecraft:cauldron at=1,0,1\n  \
                  raw_fill mat=minecraft:chest from=2,0,1 to=3,0,1\n  \
                  raw_block mat=minecraft:cobblestone_wall at=4,0,1\n  \
                  raw_block mat=minecraft:stone at=5,0,1\n  \
                  raw_block mat=minecraft:chest at=1,0,3\n  \
                  raw_block id=box mat=minecraft:chest at=3,0,3\n";
    let found = |edition, version| {
        let block_ir = lower_source(source, edition, Some(version));
        builtin_pack(edition)
            .view(None)
            .semantic_sensitive(builtin_constraints(), &block_ir)
    };
    let finding = |member: &str, reason: &str, boundary: &str| SemanticSensitiveFinding {
        edition: Edition::Java,
        member: member.to_owned(),
        reason: reason.to_owned(),
        boundary_version: boundary.to_owned(),
    };
    assert_eq!(
        found(Edition::Java, "26.3"),
        [
            vec![finding("raw_block[0]", "cauldron split", "1.17")],
            vec![finding("raw_fill[0]", "item format", "1.20.5")],
            vec![finding("raw_block[1]", "wall connections", "1.16")],
            vec![finding("raw_block[3]", "item format", "1.20.5")],
            vec![finding("box", "item format", "1.20.5")],
        ],
    );
    assert_eq!(found(Edition::Bedrock, "1.26.60"), Vec::<Vec<_>>::new());
}

#[test]
fn a_bedrock_row_names_the_members_whose_canonical_blocks_it_lists_at_the_view_s_versions() {
    let java = builtin_pack(Edition::Java);
    let bedrock = builtin_pack(Edition::Bedrock);
    let catalog = parse_constraints(
        r#"{"schema_version": 1, "name": "t", "description": "d", "namespace": "minecraft",
            "semantic_sensitivity": [{"edition": "bedrock", "boundary": "1.21.40",
                "reason": "chain split", "ids": ["chain"]}]}"#,
        java,
        bedrock,
    )
    .expect("the catalog loads");
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=5x4\n  \
                  walls mat_slot=wall height=3\n  \
                  raw_block id=links mat=minecraft:chain at=2,0,1\n";
    let block_ir = lower_source(source, Edition::Bedrock, Some("1.21.60"));
    let found = |version| {
        bedrock
            .view(version)
            .semantic_sensitive(&catalog, &block_ir)
    };
    let links = vec![vec![SemanticSensitiveFinding {
        edition: Edition::Bedrock,
        member: "links".to_owned(),
        reason: "chain split".to_owned(),
        boundary_version: "1.21.40".to_owned(),
    }]];
    assert_eq!(found(Some("1.21.60")), links);
    assert_eq!(
        found(None),
        links,
        "a version that maps `chain` onto `chain` is enough"
    );
    assert_eq!(
        found(Some("1.26.60")),
        Vec::<Vec<_>>::new(),
        "1.26.60's map gives `chain` no canonical block"
    );
    assert_eq!(
        java.view(None).semantic_sensitive(&catalog, &block_ir),
        Vec::<Vec<_>>::new(),
        "the row is Bedrock's"
    );
}
