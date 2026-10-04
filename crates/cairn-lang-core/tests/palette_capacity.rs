//! The palette capacity `lower_to_block_array` paints against, from outside
//! the crate.
//!
//! `block_array::lower`'s unit tests reach `W_PALETTE_TOO_LARGE` at a
//! capacity of four, and `block_array`'s pin `Palette::try_intern` at
//! `PALETTE_CAPACITY`. Neither lowers at the capacity the public entry point
//! passes down, and painting the 65,536 states that reach it costs seconds a
//! run. This holds a floor under it instead: a body of a thousand distinct
//! states builds, which a capacity cut to a thousand or below would refuse.

use std::fmt::Write as _;

mod common;
use common::lowered;

/// Distinct non-air states the body paints.
const STATES: usize = 1_000;

/// One `level` per state, each a `walls` row of its own block on a 1x1
/// body, so every state lands in a cell no other row paints and the
/// finished body keeps all of them.
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
        .filter(|d| d.code.as_str() == "W_PALETTE_TOO_LARGE")
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
