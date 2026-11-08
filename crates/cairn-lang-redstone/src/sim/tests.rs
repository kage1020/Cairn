use cairn_lang_core::block_array::Face;

use super::*;

fn v(x: i32, y: i32, z: i32) -> Voxel {
    Voxel::new(x, y, z)
}

/// A floor of blocks at `y = 0` under `x` and `z` from 0 to 9, and `parts` on it.
fn world(parts: &[(Voxel, Part)], seeds: &[(Voxel, bool)]) -> Sim {
    let mut all: Vec<(Voxel, Part)> = parts.to_vec();
    for x in 0..10 {
        for z in 0..10 {
            all.push((v(x, 0, z), Part::Block));
        }
    }
    let mut sim = Sim::new(all, seeds.iter().copied(), Order::Forward);
    assert!(sim.settle(32));
    sim
}

fn dust_row(from: i32, to: i32, z: i32) -> Vec<(Voxel, Part)> {
    (from..=to).map(|x| (v(x, 1, z), Part::Dust)).collect()
}

/// Dust loses one level a block, and a source over it drives it at its own level.
#[test]
fn dust_loses_a_level_each_block_from_its_source() {
    let mut sim = world(&dust_row(0, 9, 5), &[]);
    sim.set_input(v(0, 2, 5), 15);
    let levels: Vec<u8> = (0..10).map(|x| sim.power(v(x, 1, 5))).collect();
    assert_eq!(levels, [15, 14, 13, 12, 11, 10, 9, 8, 7, 6]);
}

/// Dust climbing onto a block links unless a block over the lower dust cuts it.
#[test]
fn a_block_over_the_lower_dust_cuts_a_climb() {
    let climb = [
        (v(1, 1, 5), Part::Dust),
        (v(2, 1, 5), Part::Block),
        (v(2, 2, 5), Part::Dust),
    ];
    let mut sim = world(&climb, &[]);
    sim.set_input(v(1, 2, 5), 15);
    assert_eq!(sim.power(v(2, 2, 5)), 14);

    let mut cut = climb.to_vec();
    cut.push((v(0, 1, 5), Part::Dust));
    cut.push((v(1, 2, 5), Part::Block));
    let mut sim = world(&cut, &[]);
    sim.set_input(v(0, 2, 5), 15);
    assert_eq!(sim.power(v(1, 1, 5)), 14);
    assert_eq!(sim.power(v(2, 2, 5)), 0);
}

/// Dust joined on one side runs on through the other, and powers the block it points into.
#[test]
fn dust_joined_on_one_side_points_on_into_the_block_past_it() {
    let parts = [
        (v(1, 1, 5), Part::Dust),
        (v(2, 1, 5), Part::Dust),
        (v(3, 1, 5), Part::Block),
        (v(4, 1, 5), Part::Torch { facing: Face::East }),
    ];
    let mut sim = world(&parts, &[]);
    assert_eq!(
        sim.sides(v(2, 1, 5)),
        [
            Connection::Side,
            Connection::None,
            Connection::Side,
            Connection::None
        ]
    );
    assert!(sim.is_on(v(4, 1, 5)));
    sim.set_input(v(1, 2, 5), 15);
    sim.step();
    assert!(!sim.is_on(v(4, 1, 5)));
}

/// Dust beside a block it does not point into leaves the block unpowered.
#[test]
fn dust_running_past_a_block_leaves_it_unpowered() {
    let mut parts = dust_row(1, 5, 5);
    parts.push((v(3, 1, 6), Part::Block));
    parts.push((
        v(3, 1, 7),
        Part::Torch {
            facing: Face::South,
        },
    ));
    let mut sim = world(&parts, &[]);
    sim.set_input(v(1, 2, 5), 15);
    assert!(sim.settle(8));
    assert!(sim.is_on(v(3, 1, 7)));
}

/// A repeater turned on stays on for its delay however short its input.
#[test]
fn a_repeater_stretches_a_short_pulse_to_its_delay() {
    let parts = [
        (v(1, 1, 5), Part::Dust),
        (
            v(2, 1, 5),
            Part::Repeater {
                facing: Face::East,
                delay: 3,
            },
        ),
        (v(3, 1, 5), Part::Dust),
    ];
    let mut sim = world(&parts, &[]);
    sim.set_input(v(1, 2, 5), 15);
    sim.step();
    sim.set_input(v(1, 2, 5), 0);
    let trace: Vec<u8> = (0..8)
        .map(|_| {
            sim.step();
            sim.power(v(3, 1, 5))
        })
        .collect();
    assert_eq!(trace, [0, 15, 15, 15, 0, 0, 0, 0]);
}

/// A repeater driven into another's side locks it, and it holds its output.
#[test]
fn a_repeater_driven_into_from_the_side_holds_its_output() {
    let parts = [
        (v(1, 1, 5), Part::Dust),
        (
            v(2, 1, 5),
            Part::Repeater {
                facing: Face::East,
                delay: 1,
            },
        ),
        (v(3, 1, 5), Part::Dust),
        (v(2, 1, 3), Part::Dust),
        (
            v(2, 1, 4),
            Part::Repeater {
                facing: Face::South,
                delay: 1,
            },
        ),
    ];
    let mut sim = world(&parts, &[]);
    sim.set_input(v(1, 2, 5), 15);
    assert!(sim.settle(4));
    assert_eq!(sim.power(v(3, 1, 5)), 15);
    sim.set_input(v(2, 2, 3), 15);
    assert!(sim.settle(4));
    assert!(sim.is_locked(v(2, 1, 5)));
    sim.set_input(v(1, 2, 5), 0);
    assert!(sim.settle(4));
    assert_eq!(sim.power(v(3, 1, 5)), 15);
    sim.set_input(v(2, 2, 3), 0);
    assert!(sim.settle(4));
    assert_eq!(sim.power(v(3, 1, 5)), 0);
}

/// A comparator in subtract mode outputs its rear less its side.
#[test]
fn a_subtracting_comparator_takes_its_side_from_its_rear() {
    let parts = [
        (v(1, 1, 5), Part::Dust),
        (
            v(2, 1, 5),
            Part::Comparator {
                facing: Face::East,
                subtract: true,
            },
        ),
        (v(3, 1, 5), Part::Dust),
        (v(2, 1, 6), Part::Dust),
    ];
    let mut sim = world(&parts, &[]);
    sim.set_input(v(1, 2, 5), 12);
    sim.set_input(v(2, 2, 6), 5);
    assert!(sim.settle(4));
    assert_eq!(sim.output(v(2, 1, 5)), 7);
    assert_eq!(sim.power(v(3, 1, 5)), 7);
    sim.set_input(v(2, 2, 6), 13);
    assert!(sim.settle(4));
    assert_eq!(sim.power(v(3, 1, 5)), 0);
}

/// A lit torch powers the block over it, which drives dust on it at full strength.
#[test]
fn a_torch_powers_the_block_over_it_whole() {
    let parts = [
        (v(1, 1, 5), Part::Block),
        (v(2, 1, 5), Part::Torch { facing: Face::East }),
        (v(2, 2, 5), Part::Block),
        (v(2, 3, 5), Part::Dust),
    ];
    let mut sim = world(&parts, &[]);
    assert_eq!(sim.power(v(2, 3, 5)), 15);
    sim.set_input(v(1, 2, 5), 15);
    assert!(sim.settle(4));
    assert!(!sim.is_on(v(2, 1, 5)));
    assert_eq!(sim.power(v(2, 3, 5)), 0);
}
