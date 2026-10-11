//! `E_UNKNOWN_ARGUMENT` end to end: what the two commands do with a
//! misspelled `key=`.
//!
//! The severity is what makes the report readable, and it is the ordering
//! rather than a suppression that does it. `run_compile` lowers even when
//! `check` found an error — the lowering warnings are printed after the
//! check findings and the error decides the exit code — so a misspelled
//! argument now reads: the typo first, then the `W_DEFERRED_MEMBER` it
//! caused, which names the argument that is *absent*. Before this pass, the
//! second line was the only line. A misspelled `size=` on a `struct`
//! header reads the same way, with `W_STRUCT_NO_SIZE` as the second line.

use crate::common::{cairn, compile, write_source};

/// The issue's repro: one letter, and the wall is built without the height
/// it asked for.
const TYPO: &str =
    "@cairn 2026.06\n\nstruct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n";

/// The same misspelling one line up: `siz=` on the header, so the struct
/// is also left with no size at all.
const HEADER_TYPO: &str = "@cairn 2026.06\n\nstruct s siz=5x5\n  walls mat_slot=wall height=3\n";

#[test]
fn check_refuses_a_source_carrying_a_key_nothing_reads() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "typo.crn", TYPO);
    let out = cairn("check", &[src.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("E_UNKNOWN_ARGUMENT"), "got: {stderr}");
    assert!(stderr.contains("did you mean `height`?"), "got: {stderr}");
}

#[test]
fn compile_reports_the_misspelling_before_the_absence_it_causes() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "typo.crn", TYPO);
    let out_dir = tmp.path().join("out");
    let out = compile(&src, "java", "1.21", &out_dir, None);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    // The typo and the deferral it causes are both printed, and the order
    // is what makes the pair readable: the repair first, its consequence
    // after. Reversed, the author is told to add a `height=` that is
    // already on the line.
    let typo = stderr
        .find("E_UNKNOWN_ARGUMENT")
        .unwrap_or_else(|| panic!("the misspelling must be reported: {stderr}"));
    let deferral = stderr
        .find("W_DEFERRED_MEMBER")
        .unwrap_or_else(|| panic!("premise: the typo still defers the member: {stderr}"));
    assert!(typo < deferral, "got: {stderr}");
    assert!(
        !out_dir.join("s.nbt").exists(),
        "nothing is written for a source the check gate refused",
    );
}

#[test]
fn check_without_a_target_reports_a_header_misspelling_alone() {
    // A plain `check` lowers nothing, so the missing-size warning the typo
    // causes is not raised: the error is the only line, where before the
    // header had a vocabulary there was no line at all.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "typo.crn", HEADER_TYPO);
    let out = cairn("check", &[src.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("did you mean `size`?"), "got: {stderr}");
    assert!(!stderr.contains("W_STRUCT_NO_SIZE"), "got: {stderr}");
}

#[test]
fn compile_reports_a_header_misspelling_before_the_missing_size_it_causes() {
    // The header twin of the member case above. `W_STRUCT_NO_SIZE` is
    // accurate — the struct has no size — but its note says to add a
    // `size=WxH` header to a line that already carries `siz=5x5`, so it
    // only reads right after the error that names the typo.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "typo.crn", HEADER_TYPO);
    let out_dir = tmp.path().join("out");
    let out = compile(&src, "java", "1.21", &out_dir, None);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let typo = stderr
        .find("E_UNKNOWN_ARGUMENT")
        .unwrap_or_else(|| panic!("the misspelling must be reported: {stderr}"));
    let no_size = stderr
        .find("W_STRUCT_NO_SIZE")
        .unwrap_or_else(|| panic!("premise: the typo leaves the struct sizeless: {stderr}"));
    assert!(typo < no_size, "got: {stderr}");
    assert!(
        !out_dir.join("s.nbt").exists(),
        "nothing is written for a source the check gate refused",
    );
}
