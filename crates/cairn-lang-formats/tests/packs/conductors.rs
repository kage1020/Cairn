//! What the built-in packs' conductor tables say passes redstone power, and the circuits they let
//! meet a component over the floor (`spec/redstone` "Place-and-route").

use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_formats::build::circuits::check_circuits;
use cairn_lang_formats::registry::builtin_pack;

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

fn conducts(edition: Edition, version: Option<&str>, state: &BlockState) -> Option<bool> {
    builtin_pack(edition).view(version).conducts(state)
}

const YES: Option<bool> = Some(true);
const NO: Option<bool> = Some(false);

#[test]
fn full_opaque_blocks_conduct_and_glass_leaves_glowstone_and_part_blocks_do_not() {
    let blocks = [
        ("stone", YES),
        ("oak_planks", YES),
        ("cobblestone", YES),
        ("redstone_lamp", YES),
        ("glass", NO),
        ("white_stained_glass", NO),
        ("oak_leaves", NO),
        ("glowstone", NO),
        ("redstone_block", NO),
        ("observer", NO),
        ("tnt", NO),
        ("oak_stairs", NO),
    ];
    for edition in [Edition::Java, Edition::Bedrock] {
        for version in [
            None,
            Some(builtin_pack(edition).data_versions().latest.as_str()),
        ] {
            for (id, expected) in blocks {
                assert_eq!(
                    conducts(edition, version, &state(id, &[])),
                    expected,
                    "{id} {edition:?} {version:?}",
                );
            }
        }
    }
}

#[test]
fn a_slab_conducts_only_as_a_double_slab() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let slab = |properties: &[(&str, &str)]| {
            conducts(edition, None, &state("smooth_stone_slab", properties))
        };
        assert_eq!(
            [
                slab(&[]),
                slab(&[("type", "bottom")]),
                slab(&[("type", "top")]),
                slab(&[("type", "double")]),
            ],
            [NO, NO, NO, YES],
            "{edition:?}",
        );
    }
}

#[test]
fn the_blocks_bedrock_makes_conduct_otherwise_are_read_at_each_version() {
    for id in ["honey_block", "bell", "big_dripleaf"] {
        assert_eq!(conducts(Edition::Java, None, &state(id, &[])), NO, "{id}");
        assert_eq!(
            conducts(Edition::Bedrock, None, &state(id, &[])),
            YES,
            "{id}"
        );
    }
    let bedrock = |version, id| conducts(Edition::Bedrock, version, &state(id, &[]));
    assert_eq!(
        [
            bedrock(Some("1.21.0"), "sea_lantern"),
            bedrock(Some("1.21.40"), "sea_lantern"),
            bedrock(None, "sea_lantern"),
        ],
        [YES, NO, None],
    );
    assert_eq!(
        [
            bedrock(Some("1.21.0"), "mangrove_roots"),
            bedrock(Some("1.21.40"), "mangrove_roots"),
        ],
        [NO, YES],
    );
}

#[test]
fn a_double_slab_conducts_however_its_bedrock_id_spells_it() {
    let bedrock = |version, id, properties: &[(&str, &str)]| {
        conducts(Edition::Bedrock, version, &state(id, properties))
    };
    for id in [
        "smooth_stone_double_slab",
        "stone_brick_double_slab",
        "oak_double_slab",
    ] {
        assert_eq!(
            [
                bedrock(None, id, &[]),
                bedrock(Some("1.21.60"), id, &[]),
                bedrock(Some("1.26.60"), id, &[("type", "double")]),
                bedrock(Some("1.26.60"), id, &[("type", "bottom")]),
            ],
            [YES, YES, YES, None],
            "{id}",
        );
    }
    assert_eq!(
        [
            bedrock(None, "double_stone_block_slab", &[]),
            bedrock(Some("1.21.0"), "double_stone_block_slab", &[]),
            bedrock(Some("1.21.60"), "double_stone_block_slab", &[]),
        ],
        [YES, YES, None],
    );
}

#[test]
fn a_block_no_source_settles_on_bedrock_is_not_said_to_conduct_or_not() {
    for id in [
        "mud",
        "loom",
        "jukebox",
        "command_block",
        "spawner",
        "netherreactor",
    ] {
        for version in [None, Some("1.21.0"), Some("1.26.60")] {
            assert_eq!(
                conducts(Edition::Bedrock, version, &state(id, &[])),
                None,
                "{id} {version:?}",
            );
        }
    }
    assert_eq!(
        [
            conducts(Edition::Java, None, &state("mud", &[])),
            conducts(
                Edition::Bedrock,
                Some("1.21.0"),
                &state("mangrove_roots", &[])
            ),
        ],
        [YES, NO],
    );
}

#[test]
fn a_state_the_version_does_not_give_the_block_is_not_said_to_conduct_or_not() {
    for edition in [Edition::Java, Edition::Bedrock] {
        for version in [
            None,
            Some(builtin_pack(edition).data_versions().latest.as_str()),
        ] {
            for properties in [
                [("type", "sideways")],
                [("type", "Double")],
                [("kind", "double")],
            ] {
                assert_eq!(
                    conducts(edition, version, &state("oak_slab", &properties)),
                    None,
                    "{properties:?} {edition:?} {version:?}",
                );
            }
        }
    }
}

/// A room floored with `floor`, a plate, a lamp and a door standing on it at `y=1`.
fn floored(floor: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot fl -> {floor}\n\
         struct s size=12x10\n  floor mat_slot=fl\n  walls mat_slot=wall height=4\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         lever id=la at=inside.front offset=2 y=0 -> sig.a\n  \
         pressure_plate id=pp at=inside.front offset=6 y=1 -> sig.b\n  \
         lamp id=o at=inside.back offset=3 y=1 lit_by=sig.o\n  \
         door id=d side=back at=center opened_by=sig.b\n  \
         logic sig.o = sig.a or sig.b\n  circuit region=floor void=4\n"
    )
}

/// A circuit finding as its code, the line its span starts on, its primary and each note's text
/// with the line its span starts on.
type Finding = (&'static str, usize, String, Vec<(Option<usize>, String)>);

/// Each circuit finding `cairn check` reports for `source`.
fn checked(source: &str, edition: Option<Edition>, target: Option<&str>) -> Vec<Finding> {
    let ir = lower(&parse(source).expect("the source parses"));
    let line = |offset: usize| source[..offset].matches('\n').count() + 1;
    check_circuits(&ir, edition, target)
        .into_iter()
        .map(|d| {
            let notes = d
                .notes
                .iter()
                .map(|note| {
                    (
                        note.span.clone().map(|span| line(span.start)),
                        note.message.clone(),
                    )
                })
                .collect();
            (d.code.as_str(), line(d.span.start), d.primary, notes)
        })
        .collect()
}

/// The refusal of `named`, on line `line`, over the floor block `block`, with `fix`.
fn refused(line: usize, named: &str, block: &str, fix: &str) -> Finding {
    (
        "E_ROUTE_UNREACHABLE_COMPONENT",
        line,
        format!(
            "struct=s: {named} at y=1 cannot reach the circuit under the floor: the floor block \
             under it, `{block}`, passes no power between it and the layer under the floor"
        ),
        vec![
            (Some(5), format!("the `{block}` under {named} is laid here")),
            (None, format!("Fix: {fix}")),
        ],
    )
}

/// The refusals of [`floored`]'s plate, lamp and door over the floor block `block`.
fn each_refused(block: &str) -> [Finding; 3] {
    let elsewhere = "set `y=0` on it, or build the floor under it of a block that passes power, \
                     such as `@stone`";
    [
        refused(9, "the pressure_plate `pp`", block, elsewhere),
        refused(10, "the lamp `o`", block, elsewhere),
        refused(
            11,
            "the door `d`",
            block,
            "build the floor under the door of a block that passes power, such as `@stone`",
        ),
    ]
}

#[test]
fn a_component_over_a_floor_block_that_passes_no_power_is_refused_on_every_target() {
    for (floor, block) in [
        ("@glass", "minecraft:glass"),
        ("@oak_slab", "minecraft:oak_slab"),
        ("@oak_slab[type=top]", "minecraft:oak_slab[type=top]"),
    ] {
        let source = floored(floor);
        for (edition, target) in [
            (None, None),
            (Some(Edition::Java), None),
            (Some(Edition::Java), Some("1.20.4")),
            (Some(Edition::Bedrock), None),
            (Some(Edition::Bedrock), Some("1.21.60")),
        ] {
            assert_eq!(
                checked(&source, edition, target),
                each_refused(block),
                "{floor} {edition:?} {target:?}"
            );
        }
    }
}

#[test]
fn a_component_over_a_floor_block_that_passes_power_is_met() {
    for floor in [
        "@stone",
        "@oak_planks",
        "@oak_slab[type=double]",
        "@floor.stone.smooth",
    ] {
        let source = floored(floor);
        for (edition, target) in [
            (None, None),
            (Some(Edition::Java), Some("latest")),
            (Some(Edition::Bedrock), Some("latest")),
        ] {
            assert_eq!(
                checked(&source, edition, target),
                [],
                "{floor} {edition:?} {target:?}"
            );
        }
    }
}

#[test]
fn a_component_over_a_bedrock_double_slab_spelled_as_its_own_block_is_met() {
    for (floor, target) in [
        ("@smooth_stone_double_slab", Some("1.21.60")),
        ("@stone_brick_double_slab", Some("1.26.60")),
        ("@oak_double_slab", Some("1.21.0")),
        ("@double_stone_block_slab", Some("1.21.0")),
        ("@smooth_stone_double_slab", None),
        ("@oak_double_slab", None),
        ("@double_stone_block_slab", None),
    ] {
        assert_eq!(
            checked(&floored(floor), Some(Edition::Bedrock), target),
            [],
            "{floor} {target:?}"
        );
    }
}

#[test]
fn a_component_over_a_floor_block_the_target_cannot_say_of_is_met() {
    for (floor, edition, target) in [
        ("@mud", Some(Edition::Bedrock), Some("1.26.60")),
        ("@mud", Some(Edition::Bedrock), None),
        ("@command_block", Some(Edition::Bedrock), Some("1.21.0")),
        ("@netherreactor", Some(Edition::Bedrock), Some("1.26.60")),
        ("@netherreactor", Some(Edition::Bedrock), None),
        ("@oak_slab[type=sideways]", None, None),
        ("@oak_slab[type=sideways]", Some(Edition::Java), None),
        ("@oak_slab[kind=double]", Some(Edition::Bedrock), None),
    ] {
        assert_eq!(
            checked(&floored(floor), edition, target),
            [],
            "{floor} {edition:?} {target:?}"
        );
    }
}

#[test]
fn a_floor_block_only_some_versions_say_passes_power_is_refused_only_where_it_does_not() {
    let source = floored("@sea_lantern");
    let edition = Some(Edition::Bedrock);
    assert_eq!(checked(&source, edition, Some("1.21.0")), []);
    assert_eq!(checked(&source, edition, None), []);
    assert_eq!(checked(&source, None, None), []);
    assert_eq!(
        checked(&source, edition, Some("1.26.60")),
        each_refused("minecraft:sea_lantern"),
    );
    assert_eq!(
        checked(&source, Some(Edition::Java), None),
        each_refused("minecraft:sea_lantern"),
    );
}

/// A stone-floored room whose piston a lever drives, with an observer, whose signal nothing reads,
/// powering whole a `block` beside the cell above the piston.
fn piston_beside(block: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot fl -> @stone\n\n\
         struct s size=7x5\n  floor mat_slot=fl\n  walls mat_slot=wall height=4\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         lever id=la at=inside.front offset=2 y=0 -> sig.a\n  \
         piston id=p at=inside.back offset=2 y=1 powered_by=sig.p\n  \
         raw_block mat=minecraft:{block} at=3,2,1\n  \
         observer id=sw at=inside.back offset=4 y=2 facing=right -> sig.z\n  \
         logic sig.p = not sig.a\n  circuit region=floor void=3\n"
    )
}

#[test]
fn the_java_order_check_reads_whether_a_block_passes_power_from_the_target_s_table() {
    let unread = (
        "W_LOGIC_UNUSED_SIGNAL",
        12,
        "struct=s: this sensor emits `sig.z`, which is never consumed by an actuator, downstream \
         logic, or an `assert`"
            .to_owned(),
        vec![(
            None,
            "Fix: wire an actuator to this signal, reference it from a `logic` binding or an \
             `assert`, or remove the sensor's `-> sig.<name>` tail."
                .to_owned(),
        )],
    );
    let quasi_connected = (
        "E_NO_PORTABLE_IMPL",
        10,
        "struct=s: on java, the piston `p` also takes power from the observer `sw` through the \
         cell above it, and acts on it only once a block update reaches it: this circuit requires \
         update-order (quasi-connectivity) semantics"
            .to_owned(),
        vec![
            (
                Some(12),
                "the observer `sw` powers the cell above the piston `p` from here".to_owned(),
            ),
            (
                None,
                "bedrock has no quasi-connectivity, so this is no dependence on order there"
                    .to_owned(),
            ),
            (
                None,
                "Fix: move the observer `sw` so that neither it nor a block it powers whole \
                 stands beside the cell above the piston `p`, or redesign the logic to be \
                 order-independent; or build it by hand as raw members under an `@edition java` \
                 guard"
                    .to_owned(),
            ),
        ],
    );
    for target in [None, Some("1.21.4")] {
        let java = Some(Edition::Java);
        assert_eq!(
            checked(&piston_beside("glass"), java, target),
            std::slice::from_ref(&unread),
            "{target:?}"
        );
        assert_eq!(
            checked(&piston_beside("cobblestone"), java, target),
            [unread.clone(), quasi_connected.clone()],
            "{target:?}"
        );
    }
}
