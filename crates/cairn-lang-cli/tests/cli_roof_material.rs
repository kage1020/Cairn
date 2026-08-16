//! A sloped roof's material, end to end.
//!
//! `gable` / `shed` / `hip` lowering derives `facing` / `half` / `shape` and
//! attaches them to the block the member's `mat_slot=` names. A whole block
//! has nowhere to carry them, so binding one asks for a blockstate that does
//! not exist — and neither answer available without the author is
//! acceptable: attaching the states anyway writes that blockstate into the
//! artifact, and substituting the fallback species builds the roof out of a
//! material nobody chose. The build stops instead, on both editions.
//!
//! Any *member of the stair family* is a free choice, which is what the
//! registry pack's four roof species are for.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

use flate2::read::GzDecoder;
use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

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

fn write_source(dir: &std::path::Path, material: &str) -> PathBuf {
    let path = dir.join("hut.crn");
    fs::write(&path, roofed_source(material)).expect("write source");
    path
}

fn compile(
    src: &std::path::Path,
    edition: &str,
    target: &str,
    out: &std::path::Path,
) -> (Option<i32>, String) {
    let result = Command::new(cargo_bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "--edition",
            edition,
            "--target",
            target,
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    (
        result.status.code(),
        String::from_utf8_lossy(&result.stderr).into_owned(),
    )
}

fn bytes_contain(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The Java writer gzips, so a search over the file as written finds
/// nothing whatever it is asked for. Decompressing first is what makes the
/// assertions below mean something.
fn nbt_body(path: &std::path::Path) -> Vec<u8> {
    let bytes = fs::read(path).expect("read artifact");
    let mut out = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut out)
        .expect("gzip decode");
    out
}

#[test]
fn a_non_stair_roof_material_stops_the_build_on_both_editions() {
    // Both, because a source's materials are an edition-neutral question:
    // there is no version of either edition where a cobblestone carries a
    // stair's `shape`. Java used to write the impossible state and exit 0;
    // Bedrock used to fail in its state translator, which is true of the
    // translator and the wrong layer to be reading about.
    for (edition, target) in [("java", "1.21.4"), ("bedrock", "1.21.60")] {
        let tmp = TempDir::new().expect("tempdir");
        let src = write_source(tmp.path(), "cobblestone");
        let out = TempDir::new().expect("out tempdir");
        let (code, stderr) = compile(&src, edition, target, out.path());
        assert_eq!(code, Some(1), "--edition {edition}: stderr={stderr}");
        assert!(
            stderr.contains("E_INCOMPATIBLE_MATERIAL")
                && stderr.contains("minecraft:cobblestone")
                && stderr.contains("is not a stair"),
            "--edition {edition}: stderr={stderr}",
        );
        assert!(
            !stderr.contains("the Bedrock backend cannot map"),
            "the state translator is not the layer at fault; stderr={stderr}",
        );
        assert!(
            fs::read_dir(out.path())
                .expect("out dir readable")
                .next()
                .is_none(),
            "--edition {edition}: no artifact may be written",
        );
        assert!(
            !tmp.path().join("hut.crn.lock").exists(),
            "--edition {edition}: no lockfile may certify a build that did not happen",
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
    let src = write_source(tmp.path(), "cobblestone");
    let out = TempDir::new().expect("out tempdir");
    let (_, stderr) = compile(&src, "java", "1.21.4", out.path());
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
    let src = write_source(tmp.path(), "dark_oak_stairs");
    let out = TempDir::new().expect("out tempdir");
    let (code, stderr) = compile(&src, "java", "1.21.4", out.path());
    assert_eq!(code, Some(0), "stderr={stderr}");
    assert!(
        !stderr.contains("E_INCOMPATIBLE_MATERIAL") && !stderr.contains("W_DEFERRED_MEMBER"),
        "a stair species is not a deviation; stderr={stderr}",
    );
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
