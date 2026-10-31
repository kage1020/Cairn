//! Every pressure plate a shipped example places is one a player can step on.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState};

use crate::common::{examples, latest, lower_source};

/// The id at `(x, y, z)`, or `None` past the array, where the world around the build is.
fn id_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> Option<&str> {
    let i = ba.dims.index(x, y, z)?;
    Some(&ba.palette.entries[usize::from(ba.voxels[i].0)].id)
}

#[test]
fn every_plate_a_shipped_example_places_has_room_above_it_for_a_player() {
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
                            for above in [y + 1, y + 2] {
                                let id = id_at(ba, x, above, z).unwrap_or(BlockState::AIR_ID);
                                assert_eq!(
                                    id,
                                    BlockState::AIR_ID,
                                    "{name} ({edition:?}) {scope}: the plate at ({x}, {y}, {z}) \
                                     has `{id}` at y={above}, so no player reaches it",
                                );
                            }
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
