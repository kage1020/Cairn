use cairn_lang_core::block_array::Face;

use super::{Rel, Rotation, Voxel};

#[test]
fn a_turn_carries_a_tile_offset_and_its_faces_clockwise() {
    let origin = Voxel::new(5, 1, 5);
    let ahead = Rel::flat(2, 0);
    let beside = Rel::flat(0, 1);
    let placed: Vec<(Voxel, Voxel, Face)> = Rotation::ALL
        .iter()
        .map(|turn| {
            (
                turn.place(origin, ahead),
                turn.place(origin, beside),
                turn.face(Face::East),
            )
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            (Voxel::new(7, 1, 5), Voxel::new(5, 1, 6), Face::East),
            (Voxel::new(5, 1, 7), Voxel::new(4, 1, 5), Face::South),
            (Voxel::new(3, 1, 5), Voxel::new(5, 1, 4), Face::West),
            (Voxel::new(5, 1, 3), Voxel::new(6, 1, 5), Face::North),
        ],
    );
}

#[test]
fn a_turn_leaves_up_and_down_and_names_its_forward_face() {
    for turn in Rotation::ALL {
        assert_eq!(turn.face(Face::Up), Face::Up);
        assert_eq!(turn.face(Face::Down), Face::Down);
        assert_eq!(Rotation::toward(turn.forward()), Some(turn));
    }
}

#[test]
fn side_toward_names_a_neighbour_at_any_height_and_nothing_else() {
    let at = Voxel::new(0, 2, 0);
    assert_eq!(at.side_toward(Voxel::new(1, 3, 0)), Some(Face::East));
    assert_eq!(at.side_toward(Voxel::new(0, 1, -1)), Some(Face::North));
    assert_eq!(at.side_toward(Voxel::new(1, 2, 1)), None);
    assert_eq!(at.side_toward(at.up()), None);
}
