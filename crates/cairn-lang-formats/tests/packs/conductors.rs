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
    assert_eq!(conducts(Edition::Java, None, &state("mud", &[])), YES);
    assert_eq!(bedrock(None, "mud"), NO);
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

#[test]
fn a_component_over_a_floor_block_that_passes_no_power_is_refused_on_every_target() {
    let refused = |line, named: &str, block: &str, fix: &str| {
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
    };
    let elsewhere = "set `y=0` on it, or build the floor under it of a block that passes power, \
                     such as `@stone`";
    let door = "build the floor under the door of a block that passes power, such as `@stone`";
    for (floor, block) in [
        ("@glass", "minecraft:glass"),
        ("@oak_slab", "minecraft:oak_slab"),
        ("@oak_slab[type=top]", "minecraft:oak_slab[type=top]"),
    ] {
        let source = floored(floor);
        let expected = [
            refused(9, "the pressure_plate `pp`", block, elsewhere),
            refused(10, "the lamp `o`", block, elsewhere),
            refused(11, "the door `d`", block, door),
        ];
        for (edition, target) in [
            (None, None),
            (Some(Edition::Java), None),
            (Some(Edition::Java), Some("1.20.4")),
            (Some(Edition::Bedrock), None),
            (Some(Edition::Bedrock), Some("1.21.60")),
        ] {
            assert_eq!(
                checked(&source, edition, target),
                expected,
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
fn a_floor_block_only_some_versions_say_passes_power_is_refused_only_where_it_does_not() {
    let source = floored("@sea_lantern");
    let edition = Some(Edition::Bedrock);
    assert_eq!(checked(&source, edition, Some("1.21.0")), []);
    assert_eq!(checked(&source, edition, None), []);
    assert_eq!(checked(&source, None, None), []);
    assert_eq!(
        checked(&source, edition, Some("1.26.60"))
            .into_iter()
            .map(|(code, line, _, _)| (code, line))
            .collect::<Vec<_>>(),
        [
            ("E_ROUTE_UNREACHABLE_COMPONENT", 9),
            ("E_ROUTE_UNREACHABLE_COMPONENT", 10),
            ("E_ROUTE_UNREACHABLE_COMPONENT", 11),
        ],
    );
    assert_eq!(
        checked(&source, Some(Edition::Java), None)
            .into_iter()
            .map(|(code, line, _, _)| (code, line))
            .collect::<Vec<_>>(),
        [
            ("E_ROUTE_UNREACHABLE_COMPONENT", 9),
            ("E_ROUTE_UNREACHABLE_COMPONENT", 10),
            ("E_ROUTE_UNREACHABLE_COMPONENT", 11),
        ],
    );
}
