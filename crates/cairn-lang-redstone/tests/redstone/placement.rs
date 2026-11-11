//! `compile_placement`: each sensor and actuator coupled to the layers under its struct's base
//! plane, and each cell laid there as a tile, or the finding that says why they cannot be
//! (`spec/redstone` "Place-and-route").

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_redstone::geometry::Voxel;
use cairn_lang_redstone::{EditionCell, LayoutStage, PadKind, PlacementOutput, TileKind};

use crate::common::{EXAMPLES, Finding, findings, load_example, placed, placement_of};

/// A floored and walled struct `room size=15x11`, `body` after its walls.
fn room(body: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct room size=15x11\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n\
         {body}"
    )
}

/// A plate opening a door through an `or` with a lever, `circuit` the reservation line.
fn gate(circuit: &str) -> String {
    room(&format!(
        "  door id=d side=front at=center\n  \
         pressure_plate id=p at=inside.front offset=3 y=0 -> sig.a\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.b\n  \
         logic sig.open = sig.a or sig.b\n  \
         door[id=d] opened_by=sig.open\n{circuit}"
    ))
}

fn java(source: &str) -> PlacementOutput {
    placement_of(source, Edition::Java)
}

/// The one finding `out` holds, failing on none or several.
fn only_finding<'a>(source: &'a str, out: &PlacementOutput) -> Finding<'a> {
    let [finding] = findings(source, &out.diagnostics)
        .try_into()
        .unwrap_or_else(|found: Vec<Finding<'_>>| panic!("one finding: {found:#?}"));
    finding
}

#[test]
fn each_example_lays_a_pad_per_sensor_a_riser_per_actuator_and_a_tile_per_cell() {
    type Example = (&'static str, &'static [TileKind], (u32, u32, u32));
    let expected: [Example; 3] = [
        (
            "redstone-door.crn",
            &[
                TileKind::Pad {
                    pad: PadKind::Block,
                },
                TileKind::Pad { pad: PadKind::Dust },
                TileKind::Riser,
            ],
            (3, 9, 7),
        ),
        (
            "control-room.crn",
            &[
                TileKind::Pad { pad: PadKind::Dust },
                TileKind::Pad {
                    pad: PadKind::Repeater,
                },
                TileKind::Pad { pad: PadKind::Dust },
                TileKind::Pad {
                    pad: PadKind::Observer,
                },
                TileKind::Riser,
                TileKind::Riser,
                TileKind::Riser,
            ],
            (4, 19, 15),
        ),
        (
            "crossbar.crn",
            &[
                TileKind::Pad {
                    pad: PadKind::Block,
                },
                TileKind::Pad {
                    pad: PadKind::Block,
                },
                TileKind::Riser,
                TileKind::Riser,
            ],
            (4, 14, 12),
        ),
    ];
    let cells = |name: &str, edition: Edition| -> Vec<EditionCell> {
        match (name, edition) {
            ("redstone-door.crn", Edition::Java) => vec![EditionCell::JavaRepeaterOr],
            ("redstone-door.crn", _) => vec![EditionCell::BedrockRepeaterOr],
            ("control-room.crn", Edition::Java) => {
                vec![
                    EditionCell::JavaInverterTorch,
                    EditionCell::JavaComparatorAnd,
                ]
            }
            ("control-room.crn", _) => {
                vec![
                    EditionCell::BedrockInverterTorch,
                    EditionCell::BedrockTorchAnd,
                ]
            }
            (_, Edition::Java) => vec![EditionCell::JavaComparatorAnd, EditionCell::JavaRepeaterOr],
            _ => vec![EditionCell::BedrockTorchAnd, EditionCell::BedrockRepeaterOr],
        }
    };
    for ((name, structure), (example, fixed, (void, width, depth))) in
        EXAMPLES.into_iter().zip(expected)
    {
        assert_eq!(name, example);
        for edition in [Edition::Java, Edition::Bedrock] {
            let scoped = placed(&load_example(name), edition);
            let [entry] = scoped.scopes.as_slice() else {
                panic!("{name}: one circuit: {:#?}", scoped.scopes);
            };
            let layout = &entry.ir;
            assert_eq!(layout.structure, format!("struct::{structure}"), "{name}");
            assert_eq!(layout.stage(), LayoutStage::Placed, "{name}");
            assert_eq!(
                (
                    layout.region.label.as_str(),
                    layout.region.void(),
                    layout.region.width,
                    layout.region.depth
                ),
                ("floor", void, width, depth),
                "{name}: the reservation spans the built struct",
            );
            let mut kinds: Vec<TileKind> = cells(name, edition)
                .into_iter()
                .map(|cell| TileKind::Cell { cell })
                .collect();
            kinds.extend_from_slice(fixed);
            let laid: Vec<TileKind> = layout.tiles.iter().map(|tile| tile.kind).collect();
            assert_eq!(laid, kinds, "{name} on {edition:?}");
        }
    }
}

#[test]
fn a_pad_lies_in_the_top_layer_under_its_sensor_and_a_riser_under_its_actuator() {
    let source = load_example("redstone-door.crn");
    let scoped = placed(&source, Edition::Java);
    let layout = &scoped.scopes[0].ir;
    let fixed: Vec<(TileKind, Voxel)> = layout
        .tiles
        .iter()
        .filter(|tile| !matches!(tile.kind, TileKind::Cell { .. }))
        .map(|tile| (tile.kind, tile.origin))
        .collect();
    assert_eq!(
        fixed,
        [
            (
                TileKind::Pad {
                    pad: PadKind::Block
                },
                Voxel::new(4, 2, 6)
            ),
            (TileKind::Pad { pad: PadKind::Dust }, Voxel::new(3, 2, 4)),
            (TileKind::Riser, Voxel::new(4, 2, 5)),
        ],
    );
}

#[test]
fn a_sensor_nothing_reads_has_no_pad() {
    let source = room(
        "  door id=d side=front at=center\n  \
         pressure_plate id=p at=inside.front offset=3 y=0 -> sig.a\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.unused\n  \
         door[id=d] opened_by=sig.a\n  \
         circuit region=floor void=3\n",
    );
    let scoped = placed(&source, Edition::Java);
    let kinds: Vec<TileKind> = scoped.scopes[0]
        .ir
        .tiles
        .iter()
        .map(|tile| tile.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            TileKind::Pad {
                pad: PadKind::Block
            },
            TileKind::Riser
        ]
    );
}

#[test]
fn a_def_lays_its_circuit_under_each_place_of_it() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  def booth size=9x7:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n  \
                  pressure_plate id=p at=inside.front offset=2 y=0 -> sig.a\n  \
                  logic sig.n = not sig.a\n  door[id=d] opened_by=sig.n\n  \
                  circuit region=floor void=3\n\n\
                  site row:\n  place id=one use=booth theme=t at=origin\n  \
                  place id=two use=booth theme=t east_of=one gap=3\n";
    let scoped = placed(source, Edition::Bedrock);
    let structures: Vec<(&str, &str)> = scoped
        .scopes
        .iter()
        .map(|entry| (entry.name.as_str(), entry.ir.structure.as_str()))
        .collect();
    assert_eq!(
        structures,
        [("booth", "site::row::one"), ("booth", "site::row::two")]
    );
}

#[test]
fn a_reservation_too_shallow_for_its_layers_is_refused_on_its_circuit_line() {
    let source = gate("  circuit region=floor void=2\n");
    assert_eq!(
        only_finding(&source, &java(&source)),
        (
            "E_ROUTE_CONGESTION",
            "circuit region=floor void=2",
            "struct=room: `void=2` reserves too few layers: a circuit needs the bottom layer, a layer of cells and the layer under the floor that reaches its sensors and actuators, 3 in all".to_owned(),
            vec![(None, "Fix: set `void=3` or more".to_owned())],
        ),
    );
    assert_eq!(
        java(&gate("  circuit region=floor void=3\n")).diagnostics,
        []
    );
}

#[test]
fn a_counter_needs_a_layer_more_for_its_feedback() {
    let source = room(
        "  door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
         logic sig.c = counter(sig.a, 2)\n  \
         door[id=d] opened_by=sig.c\n  \
         circuit region=floor void=3\n",
    );
    assert_eq!(
        only_finding(&source, &java(&source)),
        (
            "E_ROUTE_CONGESTION",
            "circuit region=floor void=3",
            "struct=room: `void=3` reserves too few layers: a `counter` crosses its feedback over its own wire a layer up, so its circuit needs the bottom layer, two layers of cells and the layer under the floor, 4 in all".to_owned(),
            vec![(None, "Fix: set `void=4` or more".to_owned())],
        ),
    );
}

#[test]
fn a_reservation_past_the_structure_volume_is_refused() {
    let source = gate("  circuit region=floor void=200000\n");
    assert_eq!(
        only_finding(&source, &java(&source)),
        (
            "E_ROUTE_CONGESTION",
            "circuit region=floor void=200000",
            "struct=room: `void=200000` takes the struct past 16777216 blocks, the most one structure holds".to_owned(),
            vec![(None, "Fix: lower `void`, or narrow the struct's `size=WxH`".to_owned())],
        ),
    );
}

#[test]
fn a_cell_longer_than_the_struct_is_refused_at_that_cell() {
    let source = room(
        "  door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
         logic sig.p = pulse(sig.a, 6)\n  \
         door[id=d] opened_by=sig.p\n  \
         circuit region=floor void=3\n",
    );
    let out = java(&source);
    assert!(out.scoped.scopes.is_empty(), "a refused scope is left out");
    assert_eq!(
        only_finding(&source, &out),
        (
            "E_ROUTE_CONGESTION",
            "logic sig.p = pulse(sig.a, 6)",
            "struct=room: the reservation has no room left for this `TorchPulse` cell: its tile needs 19x3 columns clear of the other tiles' (region 15x11, void=3)".to_owned(),
            vec![(
                None,
                "Fix: widen the struct's `size=WxH`, increase `void`, or split the logic across several scopes, each with its own `circuit` line".to_owned(),
            )],
        ),
    );
}

/// A lever through `logic` to a door in [`room`], over a reservation of `void=3`.
fn lever_through(logic: &str) -> String {
    room(&format!(
        "  door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.a\n  {logic}\n  \
         door[id=d] opened_by=sig.p\n  circuit region=floor void=3\n"
    ))
}

#[test]
fn a_delay_whose_row_outruns_the_struct_is_refused_before_it_is_laid() {
    let fits = lever_through("logic sig.p = delay(sig.a, 6)");
    assert_eq!(java(&fits).diagnostics, []);
    for (stages, columns) in [(7, 16), (u32::MAX, 8_589_934_592_u64)] {
        let line = format!("logic sig.p = delay(sig.a, {stages})");
        let source = lever_through(&line);
        assert_eq!(
            only_finding(&source, &java(&source)),
            (
                "E_ROUTE_CONGESTION",
                line.as_str(),
                format!(
                    "struct=room: a `RepeaterDelay` of {stages} stages cannot be laid: its repeaters \
                     run in one row of {columns} columns, longer than the 15x11 struct it lies under"
                ),
                vec![(
                    None,
                    "Fix: use fewer stages, or widen the struct's `size=WxH`".to_owned()
                )],
            ),
        );
    }
}

#[test]
fn a_macro_of_more_stages_than_its_dust_carries_is_refused_at_its_line() {
    for (macro_, line, what, max, instead) in [
        (
            "pulse",
            "logic sig.p = pulse(sig.a, 7)",
            "a `pulse`'s inverted operand runs one block of dust past each of its repeaters",
            6,
            ", or write a longer pulse of `x` as `x and not delay(x, n)`",
        ),
        (
            "counter",
            "logic sig.p = counter(sig.a, 5)",
            "a `counter`'s operand and the operand inverted run past every stage on one strand of dust",
            4,
            "",
        ),
    ] {
        let source = room(&format!(
            "  door id=d side=front at=center\n  \
             lever id=l at=inside.back offset=4 y=0 -> sig.a\n  {line}\n  \
             door[id=d] opened_by=sig.p\n  circuit region=floor void=4\n"
        ));
        let name = if macro_ == "pulse" {
            "TorchPulse"
        } else {
            "RepeaterCounter"
        };
        let stages = max + 1;
        assert_eq!(
            only_finding(&source, &java(&source)),
            (
                "E_ATTENUATION_LIMIT",
                line,
                format!(
                    "struct=room: a {name} of {stages} stages cannot be built: {what}, so it holds at most {max} stages before the dust goes dark"
                ),
                vec![(None, format!("Fix: use at most {max} stages{instead}"))],
            ),
        );
    }
}

/// Components written where the circuit under the floor cannot reach them, and why.
struct Unreachable {
    /// The members, after the door.
    body: &'static str,
    /// Each refused component's line, the finding's text after `struct=room: `, and the lines its
    /// notes point at.
    refused: &'static [(&'static str, &'static str, &'static [&'static str])],
    fix: &'static str,
}

/// Sensors that stand where no pad under the floor reaches them.
fn sensors_out_of_reach() -> [Unreachable; 5] {
    [
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=2 -> sig.a\n  door[id=d] opened_by=sig.a\n",
            refused: &[(
                "lever id=l at=inside.back offset=4 y=2 -> sig.a",
                "the lever `l` at y=2 cannot reach the circuit under the floor: a lever or button reaches the layer under the floor only from the floor row `y = 0`, the row over it",
                &[],
            )],
            fix: "Fix: set `y=0` on it",
        },
        Unreachable {
            body: "  button id=b at=inside.back offset=4 y=1 -> sig.a\n  door[id=d] opened_by=sig.a\n",
            refused: &[(
                "button id=b at=inside.back offset=4 y=1 -> sig.a",
                "the button `b` at y=1 cannot reach the circuit under the floor: a lever or button reaches the layer under the floor only from the floor row `y = 0`, the row over it",
                &[],
            )],
            fix: "Fix: set `y=0` on it",
        },
        Unreachable {
            body: "  daylight id=s at=inside.back offset=4 y=1 -> sig.a\n  door[id=d] opened_by=sig.a\n",
            refused: &[(
                "daylight id=s at=inside.back offset=4 y=1 -> sig.a",
                "the daylight `s` at y=1 cannot reach the circuit under the floor: a daylight detector reaches the layer under the floor only from the floor row `y = 0`, the row over it",
                &[],
            )],
            fix: "Fix: set `y=0` on it",
        },
        Unreachable {
            body: "  pressure_plate id=p at=inside.back offset=4 y=2 -> sig.a\n  door[id=d] opened_by=sig.a\n",
            refused: &[(
                "pressure_plate id=p at=inside.back offset=4 y=2 -> sig.a",
                "the pressure_plate `p` at y=2 cannot reach the circuit under the floor: a pressure plate reaches the layer under the floor from the floor row `y = 0`, or from `y = 1` standing on a block, which it powers whole",
                &[],
            )],
            fix: "Fix: set `y=0` on the plate, or `y=1` over a floor block",
        },
        Unreachable {
            body: "  observer id=o at=inside.back offset=4 y=1 facing=out -> sig.a\n  door[id=d] opened_by=sig.a\n",
            refused: &[(
                "observer id=o at=inside.back offset=4 y=1 facing=out -> sig.a",
                "the observer `o` at y=1 cannot reach the circuit under the floor: an observer reaches the layer under the floor only looking up, so that its back powers the cell under it, from the floor row `y = 0` or from `y = 1` over a block",
                &[],
            )],
            fix: "Fix: write `facing=up` on the observer, and set `y=0`, or `y=1` over a floor block",
        },
    ]
}

/// Actuators that no riser under the floor can drive as they are written.
fn actuators_out_of_reach() -> [Unreachable; 7] {
    [
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  lamp id=m at=inside.left offset=4 y=3 lit_by=sig.a\n",
            refused: &[(
                "lamp id=m at=inside.left offset=4 y=3 lit_by=sig.a",
                "the lamp `m` at y=3 cannot reach the circuit under the floor: an actuator is driven from the layer under the floor, so it must stand in the floor row `y = 0`, or at `y = 1` over a floor block",
                &[],
            )],
            fix: "Fix: set `y=0` on it, or `y=1` over a floor block",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
                   lamp id=m at=inside.left offset=4 y=1 lit_by=sig.a\n  \
                   raw_block mat=minecraft:air at=1,0,4\n",
            refused: &[(
                "lamp id=m at=inside.left offset=4 y=1 lit_by=sig.a",
                "the lamp `m` at y=1 cannot reach the circuit under the floor: an actuator is driven from the layer under the floor, so it must stand in the floor row `y = 0`, or at `y = 1` over a floor block",
                &[],
            )],
            fix: "Fix: set `y=0` on it, or `y=1` over a floor block",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  door[id=d] opened_by=sig.a\n  \
                   raw_block mat=minecraft:air at=7,0,10\n",
            refused: &[(
                "door id=d side=front at=center",
                "the door `d` at y=1 cannot reach the circuit under the floor: a door is driven through the block under its lower half, so it must stand at `y = 1` over a floor block",
                &[],
            )],
            fix: "Fix: stand the door on the floor row's blocks",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  piston id=r at=inside.left offset=4 y=2 facing=down powered_by=sig.a\n",
            refused: &[(
                "piston id=r at=inside.left offset=4 y=2 facing=down powered_by=sig.a",
                "the piston `r` at y=2 cannot reach the circuit under the floor: a piston looking down would push into the torch that drives it from under the floor",
                &[],
            )],
            fix: "Fix: turn the piston with another `facing=`",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
                   lever id=k at=inside.back offset=8 y=0 -> sig.b\n  \
                   lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a\n  \
                   lamp id=n at=inside.left offset=5 y=0 lit_by=sig.b\n",
            refused: &[
                (
                    "lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a",
                    "the lamp `m` at y=0 cannot reach the circuit under the floor: the block that drives it from under the floor would power the component beside that block too",
                    &["lamp id=n at=inside.left offset=5 y=0 lit_by=sig.b"],
                ),
                (
                    "lamp id=n at=inside.left offset=5 y=0 lit_by=sig.b",
                    "the lamp `n` at y=0 cannot reach the circuit under the floor: the block that drives it from under the floor would power the component beside that block too",
                    &["lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a"],
                ),
            ],
            fix: "Fix: move it, or the component beside the block under it, so that no other actuator, Tier 0 part or raw member touches that block",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
                   lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a\n  \
                   raw_block mat=minecraft:stone at=2,0,4\n",
            refused: &[(
                "lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a",
                "the lamp `m` at y=0 cannot reach the circuit under the floor: the block that drives it from under the floor would power the component beside that block too",
                &["raw_block mat=minecraft:stone at=2,0,4"],
            )],
            fix: "Fix: move it, or the component beside the block under it, so that no other actuator, Tier 0 part or raw member touches that block",
        },
        Unreachable {
            body: "  lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
                   lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a\n  \
                   redstone_dust at=2,0,4\n",
            refused: &[(
                "lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a",
                "the lamp `m` at y=0 cannot reach the circuit under the floor: the block that drives it from under the floor would power the component beside that block too",
                &["redstone_dust at=2,0,4"],
            )],
            fix: "Fix: move it, or the component beside the block under it, so that no other actuator, Tier 0 part or raw member touches that block",
        },
    ]
}

#[test]
fn a_component_the_circuit_cannot_reach_is_refused_on_its_line_with_its_reason() {
    for case in sensors_out_of_reach()
        .into_iter()
        .chain(actuators_out_of_reach())
    {
        let source = room(&format!(
            "  door id=d side=front at=center\n{body}  circuit region=floor void=3\n",
            body = case.body,
        ));
        for edition in [Edition::Java, Edition::Bedrock] {
            let out = placement_of(&source, edition);
            assert!(out.scoped.scopes.is_empty(), "{source}");
            let expected: Vec<_> = case
                .refused
                .iter()
                .map(|(line, primary, beside)| {
                    let notes = beside.iter().map(|line| {
                        (
                            Some(*line),
                            "the block that drives it touches this member, which it would power too"
                                .to_owned(),
                        )
                    });
                    (
                        "E_ROUTE_UNREACHABLE_COMPONENT",
                        *line,
                        format!("struct=room: {primary}"),
                        notes
                            .chain([(None, case.fix.to_owned())])
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            assert_eq!(findings(&source, &out.diagnostics), expected, "{source}");
        }
    }
}

/// The lever beside the lamp takes no power from the block the riser lights, so the lamp is not
/// refused for it, but the lever's pad and the riser's torches have no room side by side.
#[test]
fn a_pad_and_a_riser_side_by_side_have_no_room_under_the_floor() {
    let source = room(
        "  door id=d side=front at=center\n  \
         lever id=l at=inside.left offset=5 y=0 -> sig.a\n  \
         lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a\n  \
         circuit region=floor void=3\n",
    );
    for edition in [Edition::Java, Edition::Bedrock] {
        let out = placement_of(&source, edition);
        assert!(out.scoped.scopes.is_empty(), "{edition:?}");
        assert_eq!(
            findings(&source, &out.diagnostics),
            [(
                "E_ROUTE_CONGESTION",
                "lamp id=m at=inside.left offset=4 y=0 lit_by=sig.a",
                "struct=room: the layers under this component have no room for the torches that \
                 carry its signal up to it: each way of laying them leaves the reservation or \
                 meets another component's"
                    .to_owned(),
                vec![(
                    None,
                    "Fix: move the component away from the struct's edge and from the other \
                     components beside it"
                        .to_owned(),
                )],
            )],
            "{edition:?}",
        );
    }
}

/// One `circuit` line that reserves nothing, and the finding the pass makes of it.
struct RejectedLine {
    /// `size=` header of the scope, or empty for none.
    size: &'static str,
    /// How many `level` blocks the line is nested under.
    levels: usize,
    line: &'static str,
    /// The primary after `this ``circuit`` line reserves no room ...: `.
    reason: &'static str,
    fix: &'static str,
}

/// One case per `CircuitRegionDefect` variant, plus a second `level` for the nesting one.
fn rejected_lines() -> [RejectedLine; 10] {
    let sized = " size=15x11";
    let nested = RejectedLine {
        size: sized,
        levels: 1,
        line: "circuit region=floor void=3",
        reason: "it is written under a `level`, and only a `circuit` line at the scope's top level is read as a reservation",
        fix: "Fix: move the `circuit` line out of the `level` to the scope's top level. That changes nothing else about it: a reservation lies under the scope's base plane, and no pass reads a `level`'s `y=` for a `circuit` line",
    };
    [
        RejectedLine {
            levels: 2,
            ..nested
        },
        nested,
        RejectedLine {
            size: "",
            levels: 0,
            line: "circuit region=floor void=3",
            reason: "the enclosing scope has no `size=WxH` header for it to reserve within",
            fix: "Fix: give the enclosing scope a `size=WxH` header",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit void=3",
            reason: "it has no `region=`",
            fix: "Fix: add `region=<label>` naming the reservation (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=3 void=3",
            reason: "its `region=` must be an identifier or string label, got integer",
            fix: "Fix: write `region=` as an identifier or a string (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=\"\" void=3",
            reason: "its `region=` is an empty label",
            fix: "Fix: give `region=` a non-empty label (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor",
            reason: "it has no `void=`",
            fix: "Fix: add `void=<N>`, the layers under the base plane, an integer >= 3",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=deep",
            reason: "its `void=` must be an integer, got identifier",
            fix: "Fix: write `void=` as an integer >= 3",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=0",
            reason: "its `void=0` reserves no layer under the base plane",
            fix: "Fix: set `void=` to an integer >= 3",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=4294967296",
            reason: "its `void=4294967296` is over the limit of 4294967295",
            fix: "Fix: set `void=` to an integer from 3 to 4294967295",
        },
    ]
}

/// [`gate`]'s members in `keyword s{size}`, then `lines` at the body's top level.
fn gate_with_lines(keyword: &str, size: &str, lines: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         {keyword} s{size}\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n  \
         pressure_plate id=p at=inside.front offset=3 y=0 -> sig.a\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.b\n  \
         logic sig.open = sig.a or sig.b\n  door[id=d] opened_by=sig.open\n{lines}"
    )
}

/// Each case runs under a `struct` and a `def`, since the walk reads the two in separate loops.
#[test]
fn each_circuit_line_that_reserves_nothing_is_refused_on_that_line_with_its_reason() {
    for case in &rejected_lines() {
        for keyword in ["struct", "def"] {
            let mut lines = String::new();
            let mut indent = String::from("  ");
            for y in 0..case.levels {
                let _ = writeln!(lines, "{indent}level y={y}");
                indent.push_str("  ");
            }
            let _ = writeln!(lines, "{indent}{}", case.line);
            let source = gate_with_lines(keyword, case.size, &lines);
            let out = java(&source);
            assert!(out.scoped.scopes.is_empty(), "{source}");
            assert_eq!(
                only_finding(&source, &out),
                (
                    "E_NO_CIRCUIT_REGION",
                    case.line,
                    format!(
                        "this `circuit` line reserves no room for the scope's redstone cells or actuators: {}",
                        case.reason,
                    ),
                    vec![(None, case.fix.to_owned())],
                ),
                "{source}",
            );
        }
    }
}

#[test]
fn the_first_of_two_rejected_lines_is_the_one_reported() {
    let source = gate_with_lines(
        "struct",
        " size=15x11",
        "  circuit void=3\n  circuit region=floor void=0\n",
    );
    let (code, line, primary, _) = only_finding(&source, &java(&source));
    assert_eq!(
        (code, line, primary.as_str()),
        (
            "E_NO_CIRCUIT_REGION",
            "circuit void=3",
            "this `circuit` line reserves no room for the scope's redstone cells or actuators: it has no `region=`",
        ),
    );
}

/// Refusing the second line is `check`'s `E_DUPLICATE_CIRCUIT`.
#[test]
fn a_usable_circuit_line_beside_a_rejected_one_places_with_no_finding() {
    for lines in [
        "  circuit region=floor void=0\n  circuit region=basement void=3\n",
        "  circuit region=basement void=3\n  circuit region=3 void=0\n",
        "  circuit region=basement void=3\n  circuit region=floor void=4\n",
    ] {
        let out = java(&gate_with_lines("struct", " size=15x11", lines));
        assert_eq!(out.diagnostics, [], "{lines}");
        let [scope] = out.scoped.scopes.as_slice() else {
            panic!("the scope places: {lines}{:?}", out.scoped.scopes);
        };
        assert_eq!(
            (scope.ir.region.label.as_str(), scope.ir.region.void()),
            ("basement", 3),
            "{lines}"
        );
    }
}

#[test]
fn a_scope_with_no_circuit_line_is_told_what_its_kind_can_do() {
    let structure = gate_with_lines("struct", " size=15x11", "");
    assert_eq!(
        only_finding(&structure, &java(&structure)),
        (
            "E_NO_CIRCUIT_REGION",
            "logic sig.open = sig.a or sig.b",
            "this scope has redstone cells or actuators to build but no `circuit` line at its top level to reserve room for them".to_owned(),
            vec![(
                None,
                "Fix: add a `circuit region=<label> void=<N>` line at the top level of the scope, with a non-empty label naming the reservation (`region=floor`, `region=basement`) and an integer `void=` >= 3. It reserves the layers under the scope's base plane".to_owned(),
            )],
        ),
    );

    let wired = room(
        "  door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=4 y=0 -> sig.a\n  \
         door[id=d] opened_by=sig.a\n",
    );
    let found = java(&wired).diagnostics;
    let read = wired.find("opened_by=sig.a").expect("the binding") + "opened_by=".len();
    assert_eq!(
        found
            .iter()
            .map(|d| (d.code.as_str(), d.span.clone()))
            .collect::<Vec<_>>(),
        [("E_NO_CIRCUIT_REGION", read..read + "sig.a".len())],
        "a scope with no cell is told at the signal its actuator reads",
    );

    // `check` refuses these rows in a `site`; a library caller reaches placement without it.
    let site = "theme t:\n  slot wall -> @oak_planks\n\n\
                site s\n  \
                pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
                pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
                logic sig.open = sig.a or sig.b\n  \
                door id=d side=front at=center opened_by=sig.open\n  \
                circuit region=floor void=3\n";
    assert_eq!(
        only_finding(site, &java(site)),
        (
            "E_NO_CIRCUIT_REGION",
            "logic sig.open = sig.a or sig.b",
            "this `site` has redstone cells or actuators to build, and a `site` has no base plane of its own to reserve room under".to_owned(),
            vec![(
                None,
                "Fix: move the redstone into a `struct` or `def` with a `size=WxH` header and a `circuit region=<label> void=<N>` line at its top level".to_owned(),
            )],
        ),
    );
}

#[test]
fn multiple_scopes_place_independently() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  struct alpha size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n  \
                  pressure_plate id=p at=inside.front offset=2 y=0 -> sig.a\n  \
                  logic sig.na = not sig.a\n  door[id=d] opened_by=sig.na\n  \
                  circuit region=floor void=3\n\n\
                  struct beta size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=e side=front at=center\n  \
                  pressure_plate id=q at=inside.front offset=2 y=0 -> sig.b\n  \
                  logic sig.nb = not sig.b\n  door[id=e] opened_by=sig.nb\n";
    let out = placement_of(source, Edition::Bedrock);
    let names: Vec<&str> = out.scoped.scopes.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["alpha"], "beta declares no reservation");
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E_NO_CIRCUIT_REGION"]);
}
