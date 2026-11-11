//! `cairn compile` lays each struct's `logic` as the circuit in the layers its `circuit` line
//! reserves under it, in the target's spelling (`spec/redstone` "Place-and-route").

use std::fs;

use crate::common::{
    Fixture, Run, WALL_LEVER, assert_stream, bytes_contain, cairn, example_in_tempdir, nbt_body,
};

/// `example` compiled for `edition` and `flags` into a fresh directory, with the bytes of
/// `artifact`.
fn compiled(example: &str, edition: &str, flags: &[&str], artifact: &str) -> (Run, Vec<u8>) {
    let (tmp, source) = example_in_tempdir(example);
    let out = tmp.path().join("out");
    let mut args = vec![
        source.to_str().unwrap(),
        "--edition",
        edition,
        "--out",
        out.to_str().unwrap(),
    ];
    args.extend_from_slice(flags);
    let run = Run::from(cairn("compile", &args));
    let path = out.join(artifact);
    let bytes = if edition == "java" {
        nbt_body(&path)
    } else {
        fs::read(&path).expect("read artifact")
    };
    (run, bytes)
}

#[test]
fn each_edition_writes_the_circuit_in_its_own_spelling() {
    let cases = [
        (
            "java",
            "gatehouse.nbt",
            ["minecraft:redstone_wall_torch", "minecraft:repeater"],
            "minecraft:unlit_redstone_torch",
        ),
        (
            "bedrock",
            "gatehouse.mcstructure",
            [
                "minecraft:unlit_redstone_torch",
                "minecraft:unpowered_repeater",
            ],
            "minecraft:redstone_wall_torch",
        ),
    ];
    for (edition, artifact, spelled, foreign) in cases {
        let (run, bytes) = compiled("redstone-door.crn", edition, &[], artifact);
        assert_eq!(run.code, Some(0), "{edition}: stderr={}", run.stderr);
        assert_eq!(run.stderr, "", "{edition}");
        for id in spelled
            .into_iter()
            .chain(["minecraft:redstone_wire", "minecraft:stone"])
        {
            assert!(
                bytes_contain(&bytes, id),
                "{edition}: {artifact} holds {id}"
            );
        }
        assert!(
            !bytes_contain(&bytes, foreign),
            "{edition}: {artifact} spells no {foreign}"
        );
    }
}

#[test]
fn a_bedrock_version_that_works_out_dust_itself_drops_the_circuits_sides_without_a_warning() {
    let (run, bytes) = compiled(
        "redstone-door.crn",
        "bedrock",
        &["--target", "1.21.60"],
        "gatehouse.mcstructure",
    );
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert_eq!(run.stderr, "");
    assert!(bytes_contain(&bytes, "minecraft:redstone_wire"));
}

#[test]
fn a_comparator_is_written_with_the_strength_it_rests_at() {
    let (run, bytes) = compiled("crossbar.crn", "java", &[], "crossbar.nbt");
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    for tag in ["minecraft:comparator", "OutputSignal"] {
        assert!(bytes_contain(&bytes, tag), "crossbar.nbt holds {tag}");
    }
}

#[test]
fn a_circuit_finding_refuses_the_compile_and_writes_nothing() {
    let fixture = Fixture::new(WALL_LEVER);
    for edition in ["java", "bedrock"] {
        let run = Run::from(cairn(
            "compile",
            &[
                fixture.source().to_str().unwrap(),
                "--edition",
                edition,
                "--out",
                fixture.out().to_str().unwrap(),
            ],
        ));
        assert_eq!(run.code, Some(1), "{edition}: stderr={}", run.stderr);
        assert_stream(
            &run.stderr,
            &[(8, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")],
        );
        assert!(
            fixture.artifacts().is_empty(),
            "{edition}: {:?}",
            fixture.artifacts()
        );
    }
}
