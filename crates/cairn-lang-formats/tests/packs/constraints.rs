//! What the built-in constraint catalog says through a pinned pack view: the blocks the game drops
//! or lets fall, the faces that hold them and keep water in, and the members whose meaning moves at
//! a boundary version (`spec/lint` "Constraint checks").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, Constraint, Face, Support, TargetRegistry};
use cairn_lang_core::resolve::SemanticSensitiveFinding;
use cairn_lang_formats::registry::{PackView, builtin_constraints, builtin_pack};

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
    assert_eq!(constraint("torch", &[]), rests(Face::Down, Support::Center));
    assert_eq!(
        constraint("lantern", &[("hanging", "false")]),
        rests(Face::Down, Support::Center)
    );
    assert_eq!(
        constraint("lantern", &[("hanging", "true")]),
        rests(Face::Up, Support::Center)
    );
    assert_eq!(
        constraint("oak_hanging_sign", &[("attached", "false")]),
        rests(Face::Up, Support::Center)
    );
    assert_eq!(
        constraint("redstone_wire", &[]),
        rests(Face::Down, Support::Sturdy)
    );
    assert_eq!(
        constraint("oak_door", &[("half", "lower")]),
        rests(Face::Down, Support::Sturdy)
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
    }
}

#[test]
fn a_view_without_the_catalog_runs_no_check() {
    let view = builtin_pack(Edition::Java).view(Some("26.3"));
    let lever = state("lever", &[("face", "wall"), ("facing", "south")]);
    assert_eq!(view.constraint(&lever), None);
    assert_eq!(view.holds(&lever, &state("glass_pane", &[])), None);
    assert_eq!(
        view.keeps_water(&state("glass_pane", &[]), Face::North),
        None
    );
}

#[test]
fn the_members_a_java_boundary_moves_are_semantic_sensitive_once_each() {
    let source = "theme t:\n  slot wall -> @cobblestone\n\n\
                  struct s size=7x5\n  \
                  walls mat_slot=wall height=3\n  \
                  raw_block mat=minecraft:cauldron at=1,0,1\n  \
                  raw_fill mat=minecraft:chest from=2,0,1 to=3,0,1\n  \
                  raw_block mat=minecraft:cobblestone_wall at=4,0,1\n  \
                  raw_block mat=minecraft:stone at=5,0,1\n";
    let found = |edition, version| {
        let block_ir = lower_source(source, edition, Some(version));
        builtin_pack(edition)
            .view(None)
            .with_constraints(builtin_constraints())
            .semantic_sensitive(&block_ir)
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
            finding("raw_block", "cauldron split", "1.17"),
            finding("raw_fill", "item format", "1.20.5"),
            finding("raw_block", "wall connections", "1.16"),
        ],
    );
    assert_eq!(found(Edition::Bedrock, "1.26.60"), []);
}
