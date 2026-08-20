//! Contract tests for `cairn --version`.
//!
//! The number the binary prints travels from `cairn-lang-core`'s
//! `CAIRN_VERSION` through clap. The comparison here is against the version
//! cargo derived for *this* crate, which is a second, independent route out of
//! the same `[workspace.package] version`. That is the point: a constant that
//! stops tracking the workspace — which is how it came to be a release behind,
//! with every `--version` line and every lockfile recording the wrong compiler
//! — disagrees with this assertion from the other side of a crate boundary.
//! Comparing against `CAIRN_VERSION` itself would restate the constant instead
//! of checking it.

use std::process::Command;

/// The version cargo derived for this crate from `[workspace.package]`.
const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn run(flag: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cairn"))
        .arg(flag)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn version_flag_prints_the_workspace_version_on_stdout() {
    let output = run("--version");
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
    let short = run("-V");
    assert!(short.status.success(), "cairn -V exited non-zero");
    assert_eq!(
        String::from_utf8(short.stdout).expect("stdout is utf-8"),
        String::from_utf8(run("--version").stdout).expect("stdout is utf-8"),
    );
}
