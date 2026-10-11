//! `E_UNKNOWN_ARGUMENT` end to end: a misspelled `key=` refuses `check`
//! and `compile`, and `compile`, which lowers after the check findings,
//! prints the typo ahead of the warning it causes. That warning names the
//! argument that is *absent* (`W_DEFERRED_MEMBER` on a member,
//! `W_STRUCT_NO_SIZE` on a `struct` header), so it reads right only after
//! the typo.

use crate::common::{assert_stream, cairn, compile, write_source};

/// One letter, and the wall is built without the height it asked for.
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
    assert_stream(&stderr, &[(4, 42, "error", "E_UNKNOWN_ARGUMENT")]);
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
    assert_stream(
        &stderr,
        &[
            (4, 42, "error", "E_UNKNOWN_ARGUMENT"),
            (3, 1, "warning", "W_NO_THEME_BOUND"),
            (4, 3, "warning", "W_DEFERRED_MEMBER"),
        ],
    );
    assert!(
        !out_dir.join("s.nbt").exists(),
        "nothing is written for a source the check gate refused",
    );
}

#[test]
fn check_without_a_target_reports_a_header_misspelling_alone() {
    // A plain `check` lowers nothing, so the missing-size warning the typo
    // causes is not raised: the error is the only line.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "typo.crn", HEADER_TYPO);
    let out = cairn("check", &[src.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("did you mean `size`?"), "got: {stderr}");
    assert_stream(&stderr, &[(3, 14, "error", "E_UNKNOWN_ARGUMENT")]);
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
    // The misspelling first, then the sizeless struct it leaves.
    assert_stream(
        &stderr,
        &[
            (3, 14, "error", "E_UNKNOWN_ARGUMENT"),
            (3, 1, "warning", "W_STRUCT_NO_SIZE"),
        ],
    );
    assert!(
        !out_dir.join("s.nbt").exists(),
        "nothing is written for a source the check gate refused",
    );
}
