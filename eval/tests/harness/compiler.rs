//! What the harness makes of `cairn check`, `cairn compile` and `cairn lower` on one program.

use eval::compiler::Outcome;

use crate::common::{self, HUT_THEME_ONLY, check_report};

#[test]
fn a_refusal_of_the_whole_build_is_a_failed_check_with_no_diagnostic() {
    let program = "struct a size=3x3\n";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let evaluation = common::compiler()
        .evaluate(program, dir.path())
        .expect("the compiler runs");
    let report = check_report(program);
    assert!(
        report.starts_with("error[E_PARTIAL_BUILD]: main.crn: "),
        "{report}"
    );
    assert_eq!(evaluation.diagnostics, []);
    assert_eq!(evaluation.outcome, Outcome::Check(report));
}

#[test]
fn a_build_of_no_block_is_empty() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let evaluation = common::compiler()
        .evaluate(HUT_THEME_ONLY, dir.path())
        .expect("the compiler runs");
    assert_eq!(evaluation.diagnostics, []);
    assert_eq!(evaluation.outcome, Outcome::Empty);
}
