use strum::IntoEnumIterator;

use super::{DiagnosticCode, Severity};

/// The prefix is the contract tooling filters on: a CI annotator treats `E_` as failing and `W_`
/// as advisory by the string, not by `severity()`. The walk covers every variant, so the newest
/// one cannot be left out.
#[test]
fn the_code_prefix_agrees_with_the_severity() {
    for code in DiagnosticCode::iter() {
        let expected = match code.severity() {
            Severity::Error => "E_",
            Severity::Warning => "W_",
        };
        assert!(
            code.as_str().starts_with(expected),
            "{code:?} is {:?} but its string form is {}",
            code.severity(),
            code.as_str(),
        );
    }
}

/// Two codes sharing a string would make them indistinguishable to
/// every consumer that matches on it.
#[test]
fn every_code_has_its_own_string() {
    let mut seen: Vec<&str> = DiagnosticCode::iter().map(DiagnosticCode::as_str).collect();
    let total = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), total, "two variants share a string form");
}
