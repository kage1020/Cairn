use cairn_lang_core::Edition;

use crate::fixtures;

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
