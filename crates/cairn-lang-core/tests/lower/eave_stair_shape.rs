//! An eave `stair` carries the `shape=` it names, any of a stair's five (`spec/compilation` "Gable
//! roof voxel rules").

use cairn_lang_core::BlockState;

use crate::common::{lowered, only_structure};

#[test]
fn an_eave_stair_carries_each_shape_a_stair_has() {
    for shape in [
        "straight",
        "inner_left",
        "inner_right",
        "outer_left",
        "outer_right",
    ] {
        let out = lowered(&format!(
            "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs\n\n\
             struct s size=3x1\n  \
             walls mat_slot=wall height=1\n  \
             roof kind=flat mat_slot=wall overhang=1\n  \
             stair kind=stairs mat_slot=eave side=front half=top facing=out shape={shape}\n"
        ));
        assert_eq!(out.diagnostics, [], "shape={shape}");
        let stairs: Vec<String> = only_structure(&out)
            .palette
            .entries
            .iter()
            .filter(|state| state.id == "minecraft:oak_stairs")
            .map(BlockState::properties_text)
            .collect();
        assert_eq!(
            stairs,
            [format!("facing=south,half=top,shape={shape}")],
            "shape={shape}",
        );
    }
}
