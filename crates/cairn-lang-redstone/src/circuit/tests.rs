use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{AttachError, BlockState};

use super::{laid, refused};
use crate::diagnostic::{DiagnosticCode, compiler_bug, error_with_footer};
use crate::fixtures;
use crate::geometry::Voxel;
use crate::placement_ir::CircuitBlock;

fn example(name: &str) -> String {
    let path = format!("{}/../../examples/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).expect("the example reads")
}

/// Every row of `inputs`' truth table, each signal on as its bit says, the first the high bit.
fn rows<'a>(inputs: &[&'a str]) -> Vec<Vec<&'a str>> {
    (0..1u32 << inputs.len())
        .map(|row| {
            inputs
                .iter()
                .enumerate()
                .filter(|(bit, _)| row >> (inputs.len() - 1 - bit) & 1 == 1)
                .map(|(_, input)| *input)
                .collect()
        })
        .collect()
}

/// Each example's circuit, driven through every row of its inputs, lights each actuator's torch
/// exactly when its `logic` holds.
#[test]
fn each_example_computes_its_logic_on_both_editions() {
    type Truth = fn(&[&str]) -> Vec<(&'static str, bool)>;
    let cases: [(&str, &[&str], Truth); 3] = [
        ("redstone-door.crn", &["sig.step", "sig.exit"], |on| {
            vec![(
                "sig.open",
                on.contains(&"sig.step") || on.contains(&"sig.exit"),
            )]
        }),
        (
            "control-room.crn",
            &["sig.power", "sig.day", "sig.ring", "sig.tick"],
            |on| {
                vec![
                    (
                        "sig.light",
                        on.contains(&"sig.power") && !on.contains(&"sig.day"),
                    ),
                    ("sig.ring", on.contains(&"sig.ring")),
                    ("sig.tick", on.contains(&"sig.tick")),
                ]
            },
        ),
        ("crossbar.crn", &["sig.a", "sig.b"], |on| {
            let (a, b) = (on.contains(&"sig.a"), on.contains(&"sig.b"));
            vec![("sig.f1", a && b), ("sig.f2", a || b)]
        }),
    ];
    for (name, inputs, truth) in cases {
        let source = example(name);
        for edition in [Edition::Java, Edition::Bedrock] {
            let legalized = fixtures::legalized(&source, edition);
            let layout = &legalized.scopes[0].ir;
            for on in rows(inputs) {
                let lit = fixtures::drive(layout, &on);
                let expected: Vec<(String, bool)> = truth(&on)
                    .into_iter()
                    .map(|(signal, value)| (signal.to_owned(), value))
                    .collect();
                assert_eq!(
                    lit,
                    expected,
                    "{name} on {edition:?} with {on:?}\n{}",
                    fixtures::render(layout),
                );
            }
        }
    }
}

/// The circuit as legalized is the state it rests in: driving nothing changes no torch.
#[test]
fn each_example_rests_with_every_actuator_off() {
    for name in ["redstone-door.crn", "control-room.crn", "crossbar.crn"] {
        for edition in [Edition::Java, Edition::Bedrock] {
            let legalized = fixtures::legalized(&example(name), edition);
            let lit = fixtures::drive(&legalized.scopes[0].ir, &[]);
            assert!(
                lit.iter().all(|(_, on)| !on),
                "{name} on {edition:?}: {lit:?}"
            );
        }
    }
}

#[test]
fn a_block_legalization_left_outside_the_reservation_is_refused_as_a_compiler_bug() {
    let legalized = fixtures::legalized(&example("redstone-door.crn"), Edition::Java);
    let layout = &legalized.scopes[0].ir;
    let block = CircuitBlock {
        at: Voxel::new(-1, 0, 2),
        state: BlockState::bare("minecraft:stone"),
        entity: None,
    };
    assert_eq!(
        laid("struct=gatehouse:", layout, &block, 0),
        Err(compiler_bug(
            layout.region.span.clone(),
            "struct=gatehouse: a block of the circuit under this struct lies at (-1, 0, 2), \
             outside the reservation"
                .to_owned(),
        )),
    );
}

#[test]
fn an_attach_refusal_asks_for_fewer_layers_only_when_the_struct_outgrows_a_structure() {
    let legalized = fixtures::legalized(&example("redstone-door.crn"), Edition::Java);
    let layout = &legalized.scopes[0].ir;
    let span = layout.region.span.clone();
    let primary = |why: &str| {
        format!("struct=gatehouse: the circuit cannot be laid under this struct: {why}")
    };
    assert_eq!(
        refused("struct=gatehouse:", layout, &AttachError::TooLarge),
        error_with_footer(
            DiagnosticCode::RouteCongestion,
            span.clone(),
            primary(&AttachError::TooLarge.to_string()),
            "Fix: lower `void`, or split the struct's members across more than one `struct`",
        ),
    );
    for err in [
        AttachError::NoStructure,
        AttachError::OutOfBounds { at: (1, 5, 2) },
        AttachError::Overlap { at: (1, 0, 2) },
        AttachError::PositionedField {
            at: (1, 0, 2),
            field: "x".to_owned(),
        },
    ] {
        assert_eq!(
            refused("struct=gatehouse:", layout, &err),
            compiler_bug(span.clone(), primary(&err.to_string())),
        );
    }
}
