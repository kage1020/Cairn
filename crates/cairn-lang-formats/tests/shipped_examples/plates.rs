//! Every pressure plate a shipped example places is one a player can step on.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockState;

use crate::common::{examples, id_at, latest, lower_source};

#[test]
fn every_plate_a_shipped_example_places_stands_on_something_with_room_above_it() {
    let mut plates = 0;
    for (name, source) in examples() {
        for edition in [Edition::Java, Edition::Bedrock] {
            let out = lower_source(&source, edition, Some(latest(edition)));
            for (scope, ba) in &out.structures {
                for y in 0..ba.dims.y {
                    for z in 0..ba.dims.z {
                        for x in 0..ba.dims.x {
                            let is_plate = id_at(ba, x, y, z)
                                .is_some_and(|id| id.ends_with("_pressure_plate"));
                            if !is_plate {
                                continue;
                            }
                            plates += 1;
                            // A plate has no collision box, so a player stands in its cell.
                            let above = id_at(ba, x, y + 1, z).unwrap_or(BlockState::AIR_ID);
                            assert_eq!(
                                above,
                                BlockState::AIR_ID,
                                "{name} ({edition:?}) {scope}: the plate at ({x}, {y}, {z}) has \
                                 `{above}` above it, so no player reaches it",
                            );
                            // Row 0 stands on the ground the build is placed on.
                            let below = y.checked_sub(1).map(|b| {
                                id_at(ba, x, b, z).expect("the row below is in the array")
                            });
                            assert_ne!(
                                below,
                                Some(BlockState::AIR_ID),
                                "{name} ({edition:?}) {scope}: the plate at ({x}, {y}, {z}) has \
                                 air under it, so nothing holds it up",
                            );
                        }
                    }
                }
            }
        }
    }
    // Guard: a sweep that found no plate would pass as well.
    assert_eq!(
        plates, 8,
        "redstone-door and crossbar each place two, on both editions"
    );
}
