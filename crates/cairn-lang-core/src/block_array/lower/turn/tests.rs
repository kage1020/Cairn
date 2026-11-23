use indexmap::IndexMap;

use crate::block_array::BlockState;
use crate::block_array::joins::Compass;

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

fn toward(compass: Compass) -> Turn {
    Turn::toward(compass)
}

#[test]
fn a_front_toward_south_is_not_turned() {
    let stair = state(
        "minecraft:oak_stairs",
        &[("facing", "north"), ("half", "bottom")],
    );
    assert_eq!(toward(Compass::South).state(&stair), stair);
}

#[test]
fn facing_turns_clockwise_with_the_def() {
    let stair = state("minecraft:oak_stairs", &[("facing", "north")]);
    let faced = |face| toward(face).state(&stair).properties["facing"].clone();
    // Toward west is a quarter turn clockwise, so a back toward north turns toward east.
    assert_eq!(faced(Compass::West), "east");
    assert_eq!(faced(Compass::North), "south");
    assert_eq!(faced(Compass::East), "west");
}

#[test]
fn up_and_down_do_not_turn() {
    let barrel = state("minecraft:barrel", &[("facing", "up")]);
    assert_eq!(toward(Compass::East).state(&barrel), barrel);
}

#[test]
fn an_unwritten_facing_stays_unwritten() {
    let stair = state("minecraft:spruce_stairs", &[]);
    assert_eq!(toward(Compass::East).state(&stair), stair);
}

#[test]
fn an_axis_swaps_on_a_quarter_turn_and_keeps_on_a_half() {
    let log = state("minecraft:oak_log", &[("axis", "x")]);
    assert_eq!(toward(Compass::West).state(&log).properties["axis"], "z");
    assert_eq!(toward(Compass::North).state(&log).properties["axis"], "x");
    assert_eq!(toward(Compass::East).state(&log).properties["axis"], "z");
    let upright = state("minecraft:oak_log", &[("axis", "y")]);
    assert_eq!(toward(Compass::West).state(&upright), upright);
}

#[test]
fn a_rotation_moves_four_steps_a_quarter_turn() {
    let sign = state("minecraft:oak_sign", &[("rotation", "14")]);
    let turned = |compass| toward(compass).state(&sign).properties["rotation"].clone();
    assert_eq!(turned(Compass::West), "2");
    assert_eq!(turned(Compass::North), "6");
    assert_eq!(turned(Compass::East), "10");
}

#[test]
fn an_orientation_turns_its_compass_words() {
    let turned = |compass, orientation: &str| {
        toward(compass)
            .state(&state("minecraft:crafter", &[("orientation", orientation)]))
            .properties["orientation"]
            .clone()
    };
    assert_eq!(turned(Compass::West, "north_up"), "east_up");
    assert_eq!(turned(Compass::West, "up_north"), "up_east");
    assert_eq!(turned(Compass::North, "down_south"), "down_north");
    assert_eq!(turned(Compass::East, "west_up"), "south_up");
}

#[test]
fn the_four_sides_trade_their_values() {
    let vine = state(
        "minecraft:vine",
        &[("north", "true"), ("east", "false"), ("up", "false")],
    );
    let turned = toward(Compass::West).state(&vine);
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
    let turned = |compass, shape: &str| {
        toward(compass)
            .state(&state("minecraft:rail", &[("shape", shape)]))
            .properties["shape"]
            .clone()
    };
    assert_eq!(turned(Compass::West, "north_south"), "east_west");
    assert_eq!(turned(Compass::West, "ascending_east"), "ascending_south");
    assert_eq!(turned(Compass::West, "north_east"), "south_east");
    assert_eq!(turned(Compass::West, "south_west"), "north_west");
    assert_eq!(turned(Compass::North, "north_south"), "north_south");
    assert_eq!(turned(Compass::North, "ascending_east"), "ascending_west");
    assert_eq!(turned(Compass::North, "north_east"), "south_west");
    assert_eq!(turned(Compass::East, "ascending_east"), "ascending_north");
    assert_eq!(turned(Compass::East, "north_east"), "north_west");
    assert_eq!(turned(Compass::West, "inner_left"), "inner_left");
    assert_eq!(turned(Compass::West, "straight"), "straight");
}

#[test]
fn a_cell_turns_within_the_turned_footprint() {
    // A 3 by 1 footprint, one cell along x from its low corner.
    let footprint = (3, 1);
    assert_eq!(toward(Compass::South).cell((1, 0, 0), footprint), (1, 0, 0));
    assert_eq!(toward(Compass::South).footprint(footprint), (3, 1));
    assert_eq!(toward(Compass::West).footprint(footprint), (1, 3));
    assert_eq!(toward(Compass::West).cell((0, 2, 0), footprint), (0, 2, 0));
    assert_eq!(toward(Compass::West).cell((2, 0, 0), footprint), (0, 0, 2));
    assert_eq!(toward(Compass::North).cell((0, 0, 0), footprint), (2, 0, 0));
    assert_eq!(toward(Compass::East).cell((0, 0, 0), footprint), (0, 0, 2));
}

#[test]
fn a_cell_turns_by_both_extents_of_a_deep_footprint() {
    // Two wide and three deep: its near and far corners trade places on a half turn.
    let footprint = (2, 3);
    let corners = |compass| {
        let turn = toward(compass);
        (
            turn.footprint(footprint),
            turn.cell((0, 0, 0), footprint),
            turn.cell((1, 0, 2), footprint),
        )
    };
    assert_eq!(corners(Compass::South), ((2, 3), (0, 0, 0), (1, 0, 2)));
    assert_eq!(corners(Compass::West), ((3, 2), (2, 0, 0), (0, 0, 1)));
    assert_eq!(corners(Compass::North), ((2, 3), (1, 0, 2), (0, 0, 0)));
    assert_eq!(corners(Compass::East), ((3, 2), (0, 0, 1), (2, 0, 0)));
}
