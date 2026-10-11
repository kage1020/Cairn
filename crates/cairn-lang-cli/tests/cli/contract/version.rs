//! `cairn --version` prints `cairn-lang-core`'s `CAIRN_VERSION`, compared
//! against the version cargo derived for this crate, a second route out of
//! `[workspace.package] version`. It cannot see that version drift from the
//! released tag, nor the constant reset to a literal of the current number.

use crate::common::cairn_argv;

/// The version cargo derived for this crate from `[workspace.package]`.
const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn version_flag_prints_the_workspace_version_on_stdout() {
    let output = cairn_argv(&["--version"]);
    assert!(
        output.status.success(),
        "cairn --version exited non-zero: {:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert_eq!(
        stdout.trim_end(),
        format!("cairn {WORKSPACE_VERSION}"),
        "the version the binary reports is not the one cargo built it with",
    );
    assert!(
        output.stderr.is_empty(),
        "--version is not a diagnostic: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn short_version_flag_matches_long() {
    let short = cairn_argv(&["-V"]);
    assert!(short.status.success(), "cairn -V exited non-zero");
    assert_eq!(
        String::from_utf8(short.stdout).expect("stdout is utf-8"),
        String::from_utf8(cairn_argv(&["--version"]).stdout).expect("stdout is utf-8"),
    );
}
