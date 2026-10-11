//! A sloped roof's material, end to end. `gable` / `shed` / `hip` lowering
//! derives `facing` / `half` / `shape` for the block the member's
//! `mat_slot=` names, and a whole block has nowhere to carry them, so
//! binding one stops the build on both editions rather than writing a
//! blockstate that does not exist or a fallback species nobody chose. Any
//! *member of the stair family* is a free choice.

use tempfile::TempDir;

use crate::common::{assert_stream, bytes_contain, compile, nbt_body, stderr_of, write_source};

/// A hut whose roof reads `slot roof`, bound to `material`.
fn roofed_source(material: &str) -> String {
    format!(
        "@cairn 2026.06\n\n\
         theme t:\n\
         \x20\x20slot floor -> @oak_planks\n\
         \x20\x20slot wall  -> @cobblestone\n\
         \x20\x20slot roof  -> @{material}\n\n\
         struct hut size=5x3\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20walls class=outer mat_slot=wall height=3\n\
         \x20\x20roof  kind=gable mat_slot=roof overhang=0\n"
    )
}

#[test]
fn a_non_stair_roof_material_stops_the_build_on_both_editions() {
    // Both, because a source's materials are an edition-neutral question:
    // there is no version of either edition where a cobblestone carries a
    // stair's `shape`. Java would write the impossible state and exit 0;
    // Bedrock's state translator would refuse it, which is true of the
    // translator and the wrong layer to be reading about.
    for (edition, target) in [("java", "1.21.4"), ("bedrock", "1.21.60")] {
        let tmp = TempDir::new().expect("tempdir");
        let src = write_source(tmp.path(), "hut.crn", &roofed_source("cobblestone"));
        let out = TempDir::new().expect("out tempdir");
        let run = compile(&src, edition, target, out.path(), None);
        let stderr = stderr_of(&run);
        assert_eq!(
            run.status.code(),
            Some(1),
            "--edition {edition}: stderr={stderr}"
        );
        assert_stream(&stderr, &[(6, 17, "error", "E_INCOMPATIBLE_MATERIAL")]);
        assert!(
            stderr.contains("minecraft:cobblestone") && stderr.contains("is not a stair"),
            "--edition {edition}: stderr={stderr}",
        );
        assert!(
            !stderr.contains("the Bedrock backend cannot map"),
            "the state translator is not the layer at fault; stderr={stderr}",
        );
    }
}

#[test]
fn the_refusal_points_at_the_theme_line_and_names_the_slot() {
    // The member only names a slot; the binding is what needs editing, and
    // every member reading that slot has the same problem. Anchoring on the
    // member line would send the author to the wrong file position and
    // report the same edit once per reader.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", &roofed_source("cobblestone"));
    let out = TempDir::new().expect("out tempdir");
    let stderr = stderr_of(&compile(&src, "java", "1.21.4", out.path(), None));
    assert!(
        stderr.contains("hut.crn:6:"),
        "the theme's `slot roof ->` line is line 6; stderr={stderr}",
    );
    assert!(
        stderr.contains("mat_slot=roof"),
        "the note must name the slot that reaches it; stderr={stderr}",
    );
}

#[test]
fn a_stair_species_reaches_the_artifact_unremarked() {
    // The other half of the contract, and what makes this a family test
    // rather than an equality test against one hardcoded id: choosing a
    // species is what binding a roof slot is for, and the registry pack
    // ships four of them.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "hut.crn", &roofed_source("dark_oak_stairs"));
    let out = TempDir::new().expect("out tempdir");
    let run = compile(&src, "java", "1.21.4", out.path(), None);
    let stderr = stderr_of(&run);
    assert_eq!(run.status.code(), Some(0), "stderr={stderr}");
    // A stair species is not a deviation.
    assert_stream(&stderr, &[]);
    let nbt = nbt_body(&out.path().join("hut.nbt"));
    assert!(
        bytes_contain(&nbt, "minecraft:dark_oak_stairs"),
        "the species the theme chose is the one written",
    );
    assert!(
        !bytes_contain(&nbt, "minecraft:spruce_stairs"),
        "the fallback must not appear when a species was chosen",
    );
}
