//! What the built-in packs make of an eave `stair` at an inner `shape=`, on each edition
//! (`spec/compilation` "Gable roof voxel rules").

use cairn_lang_core::Edition;
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::build_structure_tag;

use crate::common::{
    dropped_shape, lower_source, note_fields, states_for, written_java_palette, written_palette,
};

const INNER: [&str; 2] = ["inner_left", "inner_right"];

/// A 3x1 struct whose front eave is a band of oak stairs at `shape`.
fn eave(shape: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs\n\n\
         struct s size=3x1\n  walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs mat_slot=eave side=front half=top facing=out shape={shape}\n"
    )
}

#[test]
fn java_writes_an_inner_eave_with_its_shape() {
    let version = "1.21.4";
    let target = resolve_java_target(version).expect("known target");
    for shape in INNER {
        let out = lower_source(&eave(shape), Edition::Java, Some(version));
        assert_eq!(out.diagnostics, [], "shape={shape}");
        let root = build_structure_tag(&out.structures["struct::s"], &target).expect("build");
        assert_eq!(
            written_java_palette(&root),
            [
                "minecraft:air[]".to_owned(),
                "minecraft:cobblestone[]".to_owned(),
                format!("minecraft:oak_stairs[facing=south,half=top,shape={shape}]"),
            ],
            "shape={shape}",
        );
    }
}

#[test]
fn bedrock_drops_an_inner_eave_shape_where_stairs_have_none_and_writes_its_corner_where_they_do() {
    for shape in INNER {
        let source = eave(shape);
        let states = format!("facing=south,half=top,shape={shape}");
        let dropped = dropped_shape(shape, &["1.21.60"]);
        for (version, expected_notes, stairs) in [
            (
                "1.21.60",
                vec![("minecraft:oak_stairs", states.as_str(), &dropped)],
                "minecraft:oak_stairs[upside_down_bit=1,weirdo_direction=2]".to_owned(),
            ),
            (
                "1.26.60",
                Vec::new(),
                format!(
                    "minecraft:oak_stairs[minecraft:corner={shape},upside_down_bit=1,\
                     weirdo_direction=2]"
                ),
            ),
        ] {
            let out = lower_source(&source, Edition::Bedrock, Some(version));
            assert_eq!(out.diagnostics, [], "shape={shape} at {version}");
            let target = resolve_bedrock_target(version).expect("known target");
            let (root, notes) =
                build_mcstructure_tag(&out.structures["struct::s"], &target, &states_for(&target))
                    .expect("build");
            assert_eq!(
                note_fields(&notes),
                expected_notes,
                "shape={shape} at {version}"
            );
            assert_eq!(
                written_palette(&root),
                [
                    "minecraft:air[]".to_owned(),
                    "minecraft:cobblestone[]".to_owned(),
                    stairs,
                ],
                "shape={shape} at {version}",
            );
        }
    }
}
