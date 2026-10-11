//! `lower_to_block_array` paints against the full palette capacity. Painting all 65,536 states
//! costs seconds a run, so this holds a floor of a thousand under it instead.

use std::fmt::Write as _;

use cairn_lang_core::DiagnosticCode;

use crate::common::lowered;

/// Distinct non-air states the body paints.
const STATES: usize = 1_000;

/// One `level` per state, each a 1x1 `walls` row of its own block that no other row paints over.
fn one_state_per_level() -> String {
    let mut source = String::from("theme t:\n");
    for i in 0..STATES {
        writeln!(source, "  slot s{i} -> @b{i}").expect("writing to a String");
    }
    source.push_str("\nstruct s size=1x1\n");
    for i in 0..STATES {
        writeln!(source, "  level y={i}\n    walls mat_slot=s{i} height=1")
            .expect("writing to a String");
    }
    source
}

#[test]
fn a_thousand_distinct_states_build_at_the_real_capacity() {
    let out = lowered(&one_state_per_level());
    let refused: Vec<&str> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::PaletteTooLarge)
        .map(|d| d.primary.as_str())
        .collect();
    assert!(refused.is_empty(), "{refused:?}");
    let built = out
        .structures
        .get("struct::s")
        .expect("a thousand states build");
    assert_eq!(
        built.palette.entries.len(),
        STATES + 1,
        "air and every state: none was refused, and none was painted over",
    );
}
