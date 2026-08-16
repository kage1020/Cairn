//! A sloped roof's material, end to end.
//!
//! The geometry attaches `facing` / `half` / `shape` to whatever the roof's
//! `mat_slot=` resolves to. Binding a whole block therefore produced
//! `minecraft:cobblestone[facing=south,half=bottom,shape=straight]` — a
//! blockstate no version of the game has. Java wrote it into a `.nbt` at
//! exit 0 and said nothing; Bedrock refused it in the state translator and
//! blamed its own mapping table, one layer below where the mistake was made.
//!
//! These pin both halves from the CLI: the refusal happens where the choice
//! is made, and neither writer is asked to represent something invalid.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

use flate2::read::GzDecoder;
use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// A cottage whose roof reads `slot roof`, bound to `material`.
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
/// absence assertions below mean something.
fn nbt_body(path: &std::path::Path) -> Vec<u8> {
    let bytes = fs::read(path).expect("read artifact");
    let mut out = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut out)
        .expect("gzip decode");
    out
}

#[test]
fn a_non_stair_roof_material_is_refused_where_it_is_chosen() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "cobblestone");
    let out = TempDir::new().expect("out tempdir");
    let (code, stderr) = compile(&src, "java", "1.21.4", out.path());
    assert_eq!(code, Some(0), "the build still completes; stderr={stderr}");
    assert!(
        stderr.contains("W_DEFERRED_MEMBER")
            && stderr.contains("minecraft:cobblestone")
            && stderr.contains("is not a stair"),
        "the roof must say what it could not do; stderr={stderr}",
    );
    let nbt = nbt_body(&out.path().join("hut.nbt"));
    assert!(
        bytes_contain(&nbt, "minecraft:spruce_stairs"),
        "the roof is still built, out of the fallback species",
    );
    // Which entry carries which properties is a question about the palette,
    // not about the bytes — a `.nbt` stores `Name` and `Properties` as
    // separate tags, so no substring search can pair them. The palette-level
    // assertion lives in `lower.rs`'s
    // `a_sloped_roof_refuses_a_material_that_is_not_a_stair`; this test's
    // job is to show the same thing survives the writer and the CLI.
}

#[test]
fn bedrock_no_longer_blames_its_own_state_mapping_for_the_roof() {
    // Before, this failed with "block `minecraft:cobblestone[...]` carries
    // blockstate properties the Bedrock backend cannot map (only the stair
    // family is mapped so far)" — true of the backend, and the wrong layer
    // to be reading about. The material never should have been accepted.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "cobblestone");
    let out = TempDir::new().expect("out tempdir");
    let (code, stderr) = compile(&src, "bedrock", "1.21.60", out.path());
    assert_eq!(code, Some(0), "stderr={stderr}");
    assert!(
        !stderr.contains("the Bedrock backend cannot map"),
        "the backend is not the layer at fault here; stderr={stderr}",
    );
    assert!(
        stderr.contains("is not a stair"),
        "the roof is; stderr={stderr}",
    );
    assert!(
        out.path().join("hut.mcstructure").exists(),
        "the build completes on the fallback species",
    );
}

#[test]
fn a_stair_species_reaches_the_artifact_unremarked() {
    // The other half of the contract, and the one that makes the check a
    // family test rather than an equality test against one hardcoded id:
    // choosing a species is what binding a roof slot is *for*, and the
    // registry pack ships four of them.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "dark_oak_stairs");
    let out = TempDir::new().expect("out tempdir");
    let (code, stderr) = compile(&src, "java", "1.21.4", out.path());
    assert_eq!(code, Some(0), "stderr={stderr}");
    assert!(
        !stderr.contains("W_DEFERRED_MEMBER"),
        "a stair species is not a deviation; stderr={stderr}",
    );
    let nbt = nbt_body(&out.path().join("hut.nbt"));
    assert!(
        bytes_contain(&nbt, "minecraft:dark_oak_stairs"),
        "the species the theme chose is the one written",
    );
    assert!(
        !bytes_contain(&nbt, "minecraft:spruce_stairs"),
        "the hardcoded fallback must not appear when a species was chosen",
    );
}
