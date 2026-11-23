use indexmap::IndexMap;

use crate::block_array::BlockState;
use crate::block_array::joins::Face;

use super::Turn;

fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: id.to_owned(),
        properties: properties
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<IndexMap<_, _>>(),
    }
}

fn toward(face: Face) -> Turn {
    Turn::to_face(face).expect("a sideways face")
}

#[test]
fn a_front_toward_south_is_not_turned() {
    let stair = state(
        "minecraft:oak_stairs",
        &[("facing", "north"), ("half", "bottom")],
    );
    assert_eq!(toward(Face::South).state(&stair), stair);
}

#[test]
fn facing_turns_clockwise_with_the_def() {
    let stair = state("minecraft:oak_stairs", &[("facing", "north")]);
    let faced = |face| toward(face).state(&stair).properties["facing"].clone();
    // Toward west is a quarter turn clockwise, so a back toward north turns toward east.
    assert_eq!(faced(Face::West), "east");
    assert_eq!(faced(Face::North), "south");
    assert_eq!(faced(Face::East), "west");
}

#[test]
fn up_and_down_neither_turn_nor_name_a_turn() {
    let barrel = state("minecraft:barrel", &[("facing", "up")]);
    assert_eq!(toward(Face::East).state(&barrel), barrel);
    assert_eq!(Turn::to_face(Face::Up), None);
    assert_eq!(Turn::to_face(Face::Down), None);
}

#[test]
fn an_axis_swaps_on_a_quarter_turn_and_keeps_on_a_half() {
    let log = state("minecraft:oak_log", &[("axis", "x")]);
    assert_eq!(toward(Face::West).state(&log).properties["axis"], "z");
    assert_eq!(toward(Face::North).state(&log).properties["axis"], "x");
    let upright = state("minecraft:oak_log", &[("axis", "y")]);
    assert_eq!(toward(Face::West).state(&upright), upright);
}

#[test]
fn a_rotation_moves_four_steps_a_quarter_turn() {
    let sign = state("minecraft:oak_sign", &[("rotation", "14")]);
    assert_eq!(toward(Face::West).state(&sign).properties["rotation"], "2");
}

#[test]
fn the_four_sides_trade_their_values() {
    let vine = state(
        "minecraft:vine",
        &[("north", "true"), ("east", "false"), ("up", "false")],
    );
    let turned = toward(Face::West).state(&vine);
    assert_eq!(
        turned,
        state(
            "minecraft:vine",
            &[("east", "true"), ("south", "false"), ("up", "false")],
        ),
    );
}

#[test]
fn a_rail_shape_turns_and_a_stair_shape_does_not() {
    let turned = |shape: &str| {
        toward(Face::West)
            .state(&state("minecraft:rail", &[("shape", shape)]))
            .properties["shape"]
            .clone()
    };
    assert_eq!(turned("north_south"), "east_west");
    assert_eq!(turned("ascending_east"), "ascending_south");
    assert_eq!(turned("north_east"), "south_east");
    assert_eq!(turned("south_west"), "north_west");
    assert_eq!(turned("inner_left"), "inner_left");
    assert_eq!(turned("straight"), "straight");
}

#[test]
fn a_cell_turns_within_the_turned_footprint() {
    // A 3 by 1 footprint, one cell along x from its low corner.
    let footprint = (3, 1);
    assert_eq!(toward(Face::South).cell((1, 0, 0), footprint), (1, 0, 0));
    assert_eq!(toward(Face::South).footprint(footprint), (3, 1));
    assert_eq!(toward(Face::West).footprint(footprint), (1, 3));
    assert_eq!(toward(Face::West).cell((0, 2, 0), footprint), (0, 2, 0));
    assert_eq!(toward(Face::West).cell((2, 0, 0), footprint), (0, 0, 2));
    assert_eq!(toward(Face::North).cell((0, 0, 0), footprint), (2, 0, 0));
    assert_eq!(toward(Face::East).cell((0, 0, 0), footprint), (0, 0, 2));
}
