use cairn_lang_core::Edition;

use super::{Checked, Circuit};
use crate::fixtures;
use crate::legalize::rest;

/// A lever through a `not` to a door, with an assertion of each kind that every run meets.
const NOT_GATE: &str = "theme t:\n  slot wall -> @oak_planks\n\n\
                        struct s size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                        door id=d side=front at=center\n  \
                        lever id=l at=inside.back offset=2 y=0 -> sig.a\n  \
                        logic sig.n = not sig.a\n  \
                        door[id=d] opened_by=sig.n\n  \
                        circuit region=floor void=3\n  \
                        assert truth(sig.a -> sig.n) { 0 -> 1; 1 -> 0 }\n  \
                        assert always(sig.a -> eventually sig.a within 0)\n";

#[test]
fn an_assertion_no_run_breaks_is_left_unchecked_when_a_run_outlasts_its_ticks() {
    let module = fixtures::module(NOT_GATE);
    for edition in [Edition::Java, Edition::Bedrock] {
        let legalized = fixtures::legalized(NOT_GATE, edition);
        let layout = &legalized.scopes[0].ir;
        let rest = rest(layout).expect("the circuit rests with its sensors off");
        let mut circuit = Circuit::of(layout, rest, "struct=s:", &[]);
        let met: Vec<_> = module.structs[0]
            .asserts
            .iter()
            .filter_map(Checked::of)
            .map(|assertion| circuit.check(assertion))
            .collect();
        assert_eq!(met, [None, None], "{edition:?}");

        // Switching on the lever takes the circuit more than a tick to answer.
        circuit.budget = 1;
        let found: Vec<(&str, String, Vec<String>)> = module.structs[0]
            .asserts
            .iter()
            .filter_map(Checked::of)
            .filter_map(|assertion| circuit.check(assertion))
            .map(|d| {
                (
                    d.code.as_str(),
                    d.primary,
                    d.notes.into_iter().map(|note| note.message).collect(),
                )
            })
            .collect();
        let fix = "Fix: report this as a compiler bug, with the struct; meanwhile widen the \
                   struct's `size=WxH` so the circuit is laid differently"
            .to_owned();
        let edition = edition.as_str();
        assert_eq!(
            found,
            [
                (
                    "W_SIM_ASSERTION_UNCHECKED",
                    format!(
                        "struct=s: `assert truth(sig.a -> sig.n)` is not checked on {edition}: \
                         with sig.a switched on, the circuit is still changing after 1 tick, the \
                         most a run is given"
                    ),
                    vec![fix.clone()],
                ),
                (
                    "W_SIM_ASSERTION_UNCHECKED",
                    format!(
                        "struct=s: `assert always(sig.a -> eventually sig.a within 0)` is not \
                         checked on {edition}: with sig.a switched on, the circuit is still \
                         changing after 1 tick, the most a run is given"
                    ),
                    vec![fix],
                ),
            ],
        );
    }
}
