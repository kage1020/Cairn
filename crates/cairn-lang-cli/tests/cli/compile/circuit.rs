//! `cairn compile` lays each struct's `logic` as the circuit in the layers its `circuit` line
//! reserves under it, in the target's spelling (`spec/redstone` "Place-and-route").

use std::fs;

use crate::common::{
    Fixture, Run, WALL_LEVER, assert_stream, bytes_contain, cairn, example_in_tempdir, int_tags,
    nbt_body,
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

/// Two levers each through a `not` into a Java `and`, whose comparator rests lit.
const RESTING_AND: &str = concat!(
    "theme t:\n",
    "  slot wall -> @oak_planks\n",
    "\n",
    "struct cmp size=12x10\n",
    "  floor mat_slot=wall\n",
    "  walls mat_slot=wall height=3\n",
    "  door id=d side=front at=center\n",
    "  lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n",
    "  lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n",
    "  logic sig.na = not sig.a\n",
    "  logic sig.nb = not sig.b\n",
    "  logic sig.f = sig.na and sig.nb\n",
    "  door[id=d] opened_by=sig.f\n",
    "  circuit region=floor void=4\n",
);

#[test]
fn a_comparator_is_written_with_the_strength_it_rests_at() {
    let fixture = Fixture::new(RESTING_AND);
    let run = Run::from(cairn(
        "compile",
        &[
            fixture.source().to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            fixture.out().to_str().unwrap(),
        ],
    ));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    let body = nbt_body(&fixture.out().join("cmp.nbt"));
    assert_eq!(int_tags(&body, "OutputSignal", false), [9]);
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
