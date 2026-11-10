use cairn_lang_core::block_array::Face;
use cairn_lang_core::logic_library::StageCount;

use super::*;
use crate::geometry::{Rotation, Voxel};
use crate::sim::{Order, Sim};

/// Where a bench puts a tile's origin.
const ORIGIN: Voxel = Voxel::new(30, 1, 30);

/// A tile on a floor, each port fed by a source over its dust.
struct Bench {
    sim: Sim,
    template: Template,
    rotation: Rotation,
    origin: Voxel,
}

impl Bench {
    fn new(template: Template, rotation: Rotation, order: Order, origin: Voxel) -> Self {
        let place = |rel: Rel| rotation.place(origin, rel);
        let mut parts: Vec<(Voxel, Part)> = template
            .parts
            .iter()
            .map(|(rel, part)| (place(*rel), part.turned(rotation)))
            .collect();
        let taken: std::collections::HashSet<Voxel> = parts.iter().map(|(v, _)| *v).collect();
        let mut supports: Vec<(Voxel, Part)> = parts
            .iter()
            .filter(|(v, part)| {
                v.y >= 2
                    && !matches!(part, Part::Block | Part::Torch { .. })
                    && !taken.contains(&v.down())
            })
            .map(|(v, _)| (v.down(), Part::Block))
            .collect();
        parts.append(&mut supports);
        for x in origin.x - 12..=origin.x + 12 {
            for z in origin.z - 12..=origin.z + 12 {
                parts.push((Voxel::new(x, 0, z), Part::Block));
            }
        }
        let seeds: Vec<(Voxel, bool)> = template
            .seeds
            .iter()
            .map(|(rel, on)| (place(*rel), *on))
            .collect();
        let mut sim = Sim::new(parts, seeds, order);
        assert!(sim.settle(64), "the tile comes to rest");
        let mut bench = Self {
            sim,
            template,
            rotation,
            origin,
        };
        bench.reset_clock();
        bench
    }

    fn cell(cell: EditionCell, rotation: Rotation, order: Order) -> Self {
        Self::new(cell_template(cell), rotation, order, ORIGIN)
    }

    fn reset_clock(&mut self) {
        assert!(self.sim.settle(64), "the tile comes to rest");
    }

    fn at(&self, rel: Rel) -> Voxel {
        self.rotation.place(self.origin, rel)
    }

    /// Feeds `port` at `level`, from a source over its dust.
    fn feed(&mut self, port: TilePort, level: u8) {
        let at = self
            .template
            .port(port)
            .unwrap_or_else(|| panic!("no port {port:?}"))
            .at;
        let above = self.at(at).up();
        self.sim.set_input(above, level);
    }

    /// Feeds `port` at the weakest strength it is rated for, or nothing.
    fn set(&mut self, port: TilePort, on: bool) {
        let need = self.template.port(port).expect("port").need;
        self.feed(port, if on { need } else { 0 });
    }

    /// Whether the output is on: its dust powered, or a riser's torch lit.
    fn out(&self) -> bool {
        match self.template.probe {
            Some(probe) => self.sim.is_on(self.at(probe)),
            None => self
                .template
                .outs
                .iter()
                .any(|out| self.sim.power(self.at(*out)) > 0),
        }
    }

    /// The output's power at each group's first cell.
    fn group_power(&self) -> Vec<u8> {
        self.template
            .groups
            .iter()
            .map(|group| self.sim.power(self.at(group[0])))
            .collect()
    }

    /// Runs `ticks` ticks and the output after each.
    fn run(&mut self, ticks: u64) -> Vec<bool> {
        (0..ticks)
            .map(|_| {
                self.sim.step();
                self.out()
            })
            .collect()
    }
}

fn stage_count(n: u32) -> StageCount {
    StageCount::new(n).expect("a stage count is at least 1")
}

fn a() -> TilePort {
    TilePort::Cell(PortName::A)
}

fn b() -> TilePort {
    TilePort::Cell(PortName::B)
}

/// Every rotation under both orders.
fn variants() -> impl Iterator<Item = (Rotation, Order)> {
    Rotation::ALL
        .into_iter()
        .flat_map(|rotation| [Order::Forward, Order::Reverse].map(|order| (rotation, order)))
}

/// The tick, counted from the change, of the last change of the output, and the output after.
fn last_change(trace: &[bool], before: bool) -> (u64, bool) {
    let mut last = 0;
    let mut previous = before;
    for (tick, &value) in trace.iter().enumerate() {
        if value != previous {
            last = u64::try_from(tick).expect("tick fits u64") + 1;
            previous = value;
        }
    }
    (last, previous)
}

/// Each two-operand cell computes its logical function, at the weakest rated strength and at
/// full strength, in every rotation and order, and its output settles within its port's ticks.
#[test]
fn every_two_operand_cell_computes_its_truth_table_within_its_port_ticks() {
    type Truth = fn(bool, bool) -> bool;
    let cells: [(EditionCell, Truth); 9] = [
        (EditionCell::JavaComparatorAnd, |x, y| x && y),
        (EditionCell::BedrockTorchAnd, |x, y| x && y),
        (EditionCell::JavaRepeaterOr, |x, y| x || y),
        (EditionCell::BedrockRepeaterOr, |x, y| x || y),
        (EditionCell::JavaTorchXor, |x, y| x != y),
        (EditionCell::JavaTorchNand, |x, y| !(x && y)),
        (EditionCell::BedrockTorchNand, |x, y| !(x && y)),
        (EditionCell::JavaTorchNor, |x, y| !(x || y)),
        (EditionCell::BedrockTorchNor, |x, y| !(x || y)),
    ];
    for (cell, truth) in cells {
        for (rotation, order) in variants() {
            for full in [false, true] {
                let mut bench = Bench::cell(cell, rotation, order);
                let mut state = (false, false);
                assert_eq!(bench.out(), truth(false, false), "{cell:?} at rest");
                for (port, name) in [(a(), PortName::A), (b(), PortName::B)] {
                    for _ in 0..2 {
                        let next = if name == PortName::A {
                            (!state.0, state.1)
                        } else {
                            (state.0, !state.1)
                        };
                        let on = if name == PortName::A { next.0 } else { next.1 };
                        let need = bench.template.port(port).expect("port").need;
                        bench.feed(
                            port,
                            if !on {
                                0
                            } else if full {
                                15
                            } else {
                                need
                            },
                        );
                        let before = truth(state.0, state.1);
                        let trace = bench.run(8);
                        let (tick, value) = last_change(&trace, before);
                        assert_eq!(
                            value,
                            truth(next.0, next.1),
                            "{cell:?} {rotation:?} {order:?} {next:?}"
                        );
                        assert!(
                            tick <= cell.port_delay_ticks(name),
                            "{cell:?} port {name:?} settled at tick {tick}"
                        );
                        state = next;
                    }
                }
            }
        }
    }
}

/// Each cell's slowest port takes exactly its port's ticks, in every rotation and order.
#[test]
fn each_cell_port_takes_the_ticks_the_cell_library_gives_it() {
    let cells = [
        (
            EditionCell::JavaComparatorAnd,
            PortName::A,
            (false, true),
            1,
        ),
        (
            EditionCell::JavaComparatorAnd,
            PortName::B,
            (true, false),
            2,
        ),
        (EditionCell::BedrockTorchAnd, PortName::A, (false, true), 2),
        (EditionCell::JavaRepeaterOr, PortName::B, (false, false), 1),
        (EditionCell::JavaTorchXor, PortName::A, (false, false), 2),
        (EditionCell::JavaTorchXor, PortName::B, (false, false), 2),
        (EditionCell::JavaTorchNand, PortName::A, (false, true), 1),
        (EditionCell::JavaTorchNor, PortName::B, (false, false), 1),
    ];
    for (cell, port, (x, y), ticks) in cells {
        assert_eq!(cell.port_delay_ticks(port), ticks, "{cell:?} {port:?}");
        for (rotation, order) in variants() {
            let mut bench = Bench::cell(cell, rotation, order);
            bench.set(a(), x);
            bench.set(b(), y);
            bench.reset_clock();
            let before = bench.out();
            bench.set(
                TilePort::Cell(port),
                !(if port == PortName::A { x } else { y }),
            );
            let (tick, _) = last_change(&bench.run(8), before);
            assert_eq!(tick, ticks, "{cell:?} {port:?} {rotation:?} {order:?}");
        }
    }
}

#[test]
fn a_macro_of_the_most_stages_counts_its_ticks_exactly() {
    let most = stage_count(u32::MAX);
    assert_eq!(
        [
            EditionCell::JavaRepeaterDelay { stages: most }.port_delay_ticks(PortName::A),
            EditionCell::BedrockTorchPulse { stages: most }.port_delay_ticks(PortName::A),
        ],
        [8_589_934_590, 8_589_934_592],
    );
}

/// `not` inverts within one tick.
#[test]
fn the_inverter_inverts_in_one_tick() {
    for (rotation, order) in variants() {
        let mut bench = Bench::cell(EditionCell::JavaInverterTorch, rotation, order);
        assert!(bench.out());
        bench.set(a(), true);
        assert_eq!(bench.run(3), [false, false, false]);
        bench.set(a(), false);
        assert_eq!(bench.run(3), [true, true, true]);
    }
}

/// `mux` follows `a` while `sel` is off and `b` while it is on, every port settling in two ticks.
#[test]
fn the_mux_follows_a_or_b_by_sel_within_two_ticks() {
    let sel = TilePort::Cell(PortName::Sel);
    for (rotation, order) in variants() {
        for bits in 0..8u8 {
            let (s, x, y) = (bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
            let mut bench = Bench::cell(EditionCell::JavaTorchMux, rotation, order);
            bench.set(sel, s);
            bench.set(a(), x);
            bench.set(b(), y);
            bench.reset_clock();
            assert_eq!(bench.out(), if s { y } else { x }, "{bits:03b}");
            for port in [sel, a(), b()] {
                let before = bench.out();
                let flipped = match port {
                    TilePort::Cell(PortName::Sel) => (!s, x, y),
                    TilePort::Cell(PortName::A) => (s, !x, y),
                    _ => (s, x, !y),
                };
                let on = match port {
                    TilePort::Cell(PortName::Sel) => flipped.0,
                    TilePort::Cell(PortName::A) => flipped.1,
                    _ => flipped.2,
                };
                bench.set(port, on);
                let (tick, value) = last_change(&bench.run(6), before);
                assert_eq!(value, if flipped.0 { flipped.2 } else { flipped.1 });
                assert!(tick <= 2, "{port:?} from {bits:03b} at {tick}");
                bench.set(port, !on);
                bench.reset_clock();
            }
        }
    }
}

/// A merged output is full strength beside the torch driving it and weakens along the strand.
#[test]
fn a_merged_output_weakens_away_from_the_torch_driving_it() {
    let mut bench = Bench::cell(EditionCell::JavaTorchNand, Rotation::East, Order::Forward);
    bench.set(a(), true);
    bench.reset_clock();
    assert_eq!(bench.group_power(), [13, 15]);
    bench.set(a(), false);
    bench.set(b(), true);
    bench.reset_clock();
    assert_eq!(bench.group_power(), [15, 13]);
}

/// The comparator AND passes on what reaches its rear.
#[test]
fn the_comparator_and_passes_its_rear_strength_on() {
    let mut bench = Bench::cell(
        EditionCell::JavaComparatorAnd,
        Rotation::East,
        Order::Forward,
    );
    bench.set(b(), true);
    for level in [1, 7, 15] {
        bench.feed(a(), level);
        bench.reset_clock();
        assert_eq!(bench.group_power(), [level], "rear at {level}");
    }
}

/// `delay(x, n)` follows `x` exactly `2n` ticks later, its repeaters at most four ticks each.
#[test]
fn delay_follows_its_operand_two_ticks_a_stage_later() {
    for stages in 1..=9 {
        for (rotation, order) in variants() {
            let cell = EditionCell::JavaRepeaterDelay {
                stages: stage_count(stages),
            };
            let ticks = cell.port_delay_ticks(PortName::A);
            assert_eq!(ticks, 2 * u64::from(stages));
            let mut bench = Bench::cell(cell, rotation, order);
            bench.set(a(), true);
            let trace = bench.run(ticks + 2);
            assert_eq!(last_change(&trace, false), (ticks, true), "{stages} rising");
            bench.set(a(), false);
            let trace = bench.run(ticks + 2);
            assert_eq!(
                last_change(&trace, true),
                (ticks, false),
                "{stages} falling"
            );
        }
    }
}

/// `pulse(x, n)` is on from two ticks in for its first `n` stages, or until `x` turns off.
#[test]
fn pulse_is_on_for_its_stages_or_until_its_operand_falls() {
    for stages in [1, 2, 3, 4, 5, MAX_PULSE_STAGES] {
        for (rotation, order) in variants() {
            let cell = EditionCell::JavaTorchPulse {
                stages: stage_count(stages),
            };
            let ticks = cell.port_delay_ticks(PortName::A);
            let mut bench = Bench::cell(cell, rotation, order);
            assert!(!bench.out());
            bench.set(a(), true);
            let trace = bench.run(ticks + 4);
            let on: Vec<u64> = (1..)
                .zip(&trace)
                .filter(|(_, on)| **on)
                .map(|(t, _)| t)
                .collect();
            let expected: Vec<u64> = (2..=2 * u64::from(stages) + 1).collect();
            assert_eq!(on, expected, "{stages} held, {rotation:?} {order:?}");
            assert_eq!(last_change(&trace, false).0, ticks);
            bench.set(a(), false);
            bench.reset_clock();
            if stages > 1 {
                bench.set(a(), true);
                let mut trace = bench.run(2);
                bench.set(a(), false);
                trace.extend(bench.run(ticks + 4));
                let on: Vec<u64> = (1..)
                    .zip(&trace)
                    .filter(|(_, on)| **on)
                    .map(|(t, _)| t)
                    .collect();
                assert_eq!(on, [2, 3], "{stages} cut short");
            }
        }
    }
}

/// Asserts that `cell`, fed each of `patterns` as stages `x` is on and off by turns, puts out at
/// every tick what `expected` says of the pattern and the tick, in every rotation and order.
fn assert_follows(
    cell: EditionCell,
    patterns: &[Vec<u64>],
    expected: impl Fn(&[u64], u64) -> bool,
) {
    for pattern in patterns {
        for (rotation, order) in variants() {
            let mut bench = Bench::cell(cell, rotation, order);
            let mut trace = Vec::new();
            for (index, &held) in pattern.iter().chain(&[16]).enumerate() {
                bench.set(a(), index % 2 == 0 && index < pattern.len());
                trace.extend(bench.run(2 * held));
            }
            let wanted: Vec<bool> = (1..=trace.len() as u64)
                .map(|tick| expected(pattern, tick))
                .collect();
            assert_eq!(trace, wanted, "{cell:?} {pattern:?} {rotation:?} {order:?}");
        }
    }
}

/// Whether `x` was on `back` ticks before `tick` under `pattern`, as [`assert_follows`] feeds it.
fn was_on(pattern: &[u64], tick: u64, back: u64) -> bool {
    let Some(at) = tick.checked_sub(back) else {
        return false;
    };
    let mut start = 0;
    for (index, &held) in pattern.iter().enumerate() {
        start += 2 * held;
        if at < start {
            return index % 2 == 0;
        }
    }
    false
}

/// Operands that change every stage, or hold a change for a stage or two before turning back.
fn patterns(stages: u32) -> [Vec<u64>; 4] {
    let n = u64::from(stages);
    [
        vec![1, 1, 1, 1, n + 2],
        vec![n + 2, 1, n + 2],
        vec![1, n, 1, 2, 3],
        vec![2, n + 1, 2],
    ]
}

/// `delay(x, n)` passes every change of an `x` that holds each for a stage, unstretched.
#[test]
fn delay_passes_each_change_held_for_a_stage_as_it_came() {
    for stages in [1, 2, 3, 5] {
        let count = stage_count(stages);
        let ticks = 2 * u64::from(stages);
        for cell in [
            EditionCell::JavaRepeaterDelay { stages: count },
            EditionCell::BedrockRepeaterDelay { stages: count },
        ] {
            assert_follows(cell, &patterns(stages), |pattern, tick| {
                was_on(pattern, tick, ticks)
            });
        }
    }
}

/// `pulse(x, n)` is on two ticks later while `x` is on and was off `n` stages before, so an `x`
/// that turns on again sooner gets only what is left of its pulse.
#[test]
fn pulse_is_on_while_its_operand_is_on_and_was_off_n_stages_before() {
    for stages in [1, 2, 3, 5] {
        let count = stage_count(stages);
        let ticks = 2 * u64::from(stages) + 2;
        for cell in [
            EditionCell::JavaTorchPulse { stages: count },
            EditionCell::BedrockTorchPulse { stages: count },
        ] {
            assert_follows(cell, &patterns(stages), |pattern, tick| {
                was_on(pattern, tick, 2) && !was_on(pattern, tick, ticks)
            });
        }
    }
}

/// `edge_falling(x)` is on a tick later while `x` is off and was on a stage before.
#[test]
fn edge_falling_is_on_while_its_operand_is_off_and_was_on_a_stage_before() {
    for cell in [
        EditionCell::JavaTorchEdgeFalling,
        EditionCell::BedrockTorchEdgeFalling,
    ] {
        assert_follows(cell, &patterns(1), |pattern, tick| {
            !was_on(pattern, tick, 1) && was_on(pattern, tick, 3)
        });
    }
}

/// `edge_rising(x)` is on for a stage from two ticks after `x` turns on, and not when it turns
/// off.
#[test]
fn edge_rising_pulses_once_on_each_rise() {
    let cell = EditionCell::JavaTorchEdgeRising;
    assert_eq!(cell.port_delay_ticks(PortName::A), 4);
    for (rotation, order) in variants() {
        let mut bench = Bench::cell(cell, rotation, order);
        bench.set(a(), true);
        assert_eq!(bench.run(6), [false, true, true, false, false, false]);
        bench.set(a(), false);
        assert_eq!(bench.run(6), [false; 6]);
    }
}

/// `edge_falling(x)` is on for a stage from a tick after `x` turns off, and not when it turns on.
#[test]
fn edge_falling_pulses_once_on_each_fall() {
    let cell = EditionCell::JavaTorchEdgeFalling;
    assert_eq!(cell.port_delay_ticks(PortName::A), 3);
    for (rotation, order) in variants() {
        let mut bench = Bench::cell(cell, rotation, order);
        bench.set(a(), true);
        assert_eq!(bench.run(6), [false; 6]);
        bench.set(a(), false);
        assert_eq!(bench.run(6), [true, true, false, false, false, false]);
    }
}

/// A `latch` sets in four ticks, resets in one, re-arms five ticks after `reset` falls under a
/// held `set`, and stays off when both fall together.
#[test]
fn latch_sets_resets_and_lets_reset_win() {
    let set = TilePort::Cell(PortName::Set);
    let reset = TilePort::Cell(PortName::Reset);
    for (rotation, order) in variants() {
        let mut bench = Bench::cell(EditionCell::JavaTorchLatch, rotation, order);
        assert!(!bench.out());
        bench.set(set, true);
        assert_eq!(last_change(&bench.run(8), false), (4, true));
        bench.set(set, false);
        assert_eq!(last_change(&bench.run(8), true), (0, true), "holds");
        bench.set(reset, true);
        assert_eq!(last_change(&bench.run(8), true), (1, false));
        bench.set(set, true);
        assert_eq!(last_change(&bench.run(8), false), (0, false), "reset wins");
        bench.set(reset, false);
        assert_eq!(last_change(&bench.run(10), false), (5, true), "re-arms");
        bench.set(reset, true);
        bench.run(8);
        bench.set(reset, false);
        bench.set(set, false);
        assert_eq!(last_change(&bench.run(10), false), (0, false), "both fall");
    }
}

/// A `counter` feeding its output back turns on at the `n`-th rise and off at the `2n`-th, three
/// ticks after the rise.
#[test]
fn counter_toggles_every_n_rises_three_ticks_after_each() {
    for stages in 1..=MAX_COUNTER_STAGES {
        for (rotation, order) in variants() {
            let cell = EditionCell::JavaRepeaterCounter {
                stages: stage_count(stages),
            };
            let mut bench = Bench::cell(cell, rotation, order);
            let fed = bench
                .at(bench.template.port(TilePort::Feedback).expect("fb").at)
                .up();
            let mut outs = Vec::new();
            for rise in 1..=(4 * stages) {
                bench.set(a(), true);
                let mut trace = Vec::new();
                for _ in 0..8 {
                    bench.sim.step();
                    let out = bench.out();
                    bench.sim.set_input(fed, if out { 15 } else { 0 });
                    trace.push(out);
                }
                let before = outs.last().copied().unwrap_or(false);
                let (tick, value) = last_change(&trace, before);
                let expected = (rise / stages) % 2 == 1;
                assert_eq!(
                    value, expected,
                    "{stages} stages, rise {rise}, {rotation:?} {order:?}"
                );
                if value != before {
                    assert_eq!(tick, 3, "{stages} stages, rise {rise}");
                }
                outs.push(value);
                bench.set(a(), false);
                for _ in 0..8 {
                    bench.sim.step();
                    let out = bench.out();
                    bench.sim.set_input(fed, if out { 15 } else { 0 });
                }
                assert_eq!(bench.out(), value, "holds while a is off");
            }
        }
    }
}

/// A riser's top torch follows its port two ticks later.
#[test]
fn a_riser_carries_its_port_up_in_two_ticks() {
    for (rotation, order) in variants() {
        let mut bench = Bench::new(riser_template(), rotation, order, Voxel::new(30, 3, 30));
        assert!(!bench.out());
        bench.set(TilePort::Actuator, true);
        assert_eq!(last_change(&bench.run(4), false), (RISER_TICKS, true));
        bench.set(TilePort::Actuator, false);
        assert_eq!(last_change(&bench.run(4), true), (RISER_TICKS, false));
    }
}

/// Each pad's output takes a sensor's power over it at full strength.
#[test]
fn every_pad_carries_its_sensor_at_full_strength() {
    for kind in [PadKind::Dust, PadKind::Block, PadKind::Repeater] {
        for (rotation, order) in variants() {
            let origin = Voxel::new(30, 3, 30);
            let mut bench = Bench::new(pad_template(kind), rotation, order, origin);
            assert_eq!(bench.group_power(), [0]);
            bench.sim.set_input(origin.up(), 1);
            bench.reset_clock();
            let expected = if kind == PadKind::Repeater { 15 } else { 1 };
            assert_eq!(bench.group_power(), [expected], "{kind:?}");
            bench.sim.set_input(origin.up(), 15);
            bench.reset_clock();
            assert_eq!(bench.group_power(), [15], "{kind:?}");
        }
    }
}

/// The other ports a case holds on or off.
type Held<'a> = &'a [(TilePort, bool)];

/// A port fed at its rating drives the tile as full strength does, and one level under does not.
#[test]
fn a_port_rating_is_the_weakest_signal_that_drives_the_tile() {
    let sel = TilePort::Cell(PortName::Sel);
    let set = TilePort::Cell(PortName::Set);
    let cases: [(EditionCell, TilePort, Held<'_>); 12] = [
        (EditionCell::JavaInverterTorch, a(), &[]),
        (EditionCell::JavaTorchNor, a(), &[]),
        (EditionCell::JavaTorchNor, b(), &[]),
        (EditionCell::JavaComparatorAnd, b(), &[(a(), true)]),
        (EditionCell::BedrockTorchAnd, a(), &[(b(), true)]),
        (EditionCell::JavaTorchXor, a(), &[(b(), true)]),
        (EditionCell::JavaTorchXor, b(), &[(a(), true)]),
        (EditionCell::JavaTorchMux, sel, &[(a(), true)]),
        (EditionCell::JavaTorchMux, b(), &[(sel, true)]),
        (
            EditionCell::JavaTorchPulse {
                stages: stage_count(3),
            },
            a(),
            &[],
        ),
        (EditionCell::JavaTorchEdgeFalling, a(), &[]),
        (
            EditionCell::JavaTorchLatch,
            TilePort::Cell(PortName::Reset),
            &[(set, true)],
        ),
    ];
    for (cell, port, context) in cases {
        let need = cell_template(cell).port(port).expect("port").need;
        let trace = |level: u8| {
            let mut bench = Bench::cell(cell, Rotation::East, Order::Forward);
            for &(other, on) in context {
                bench.set(other, on);
            }
            bench.reset_clock();
            bench.feed(port, level);
            let mut trace = bench.run(12);
            bench.feed(port, 0);
            trace.extend(bench.run(12));
            trace
        };
        let full = trace(15);
        assert_eq!(trace(need), full, "{cell:?} {port:?} at {need}");
        assert_ne!(trace(need - 1), full, "{cell:?} {port:?} under {need}");
    }
}

/// Every dust pointing into a tile's block keeps both its sides clear.
#[test]
fn a_tile_keeps_the_sides_of_dust_pointing_into_its_blocks_clear() {
    let keepout = inverter().keepout();
    assert_eq!(
        keepout.no_dust,
        [
            Rel::new(0, -1, -1),
            Rel::new(0, -1, 1),
            Rel::new(0, 0, -1),
            Rel::new(0, 0, 1),
            Rel::new(0, 1, -1),
            Rel::new(0, 1, 1),
            Rel::new(1, 0, -1),
            Rel::new(1, 0, 1),
            Rel::new(1, 1, 0),
            Rel::new(2, -1, 0),
            Rel::new(2, 0, -1),
            Rel::new(2, 0, 1),
        ]
    );
    assert_eq!(keepout.air, [Rel::new(2, 1, 0)]);
    assert_eq!(
        inverter().part(Rel::new(2, 0, 0)),
        Some(Part::Torch { facing: Face::East })
    );
}
