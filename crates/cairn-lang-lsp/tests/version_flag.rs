//! Contract tests for the `cairn-lsp` command-line surface.
//!
//! The binary is spawned by editors (no arguments — LSP over stdio) and by
//! users on the command line for support triage. The surface it exposes to
//! that second audience is: `--version`/`-V` → `cairn-lsp <version>` and
//! exit 0; `--help`/`-h` → usage string and exit 0; anything else → exit 2
//! with a message that lists the valid flags. These tests pin that contract so
//! a refactor of `main.rs` cannot silently reshape it.
//!
//! The version is compared against the number cargo derived for this crate
//! rather than against `cairn-lang-core`'s `CAIRN_VERSION`, which is where the
//! binary reads it from. Both resolve to `[workspace.package] version`, by two
//! independent routes, so a constant that stops tracking the workspace shows up
//! here instead of being restated.

use std::process::Command;

/// The version cargo derived for this crate from `[workspace.package]`.
const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn version_flag_prints_cairn_version_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .arg("--version")
        .output()
        .expect("spawn cairn-lsp --version");

    assert!(
        output.status.success(),
        "cairn-lsp --version exited non-zero: {:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert_eq!(
        stdout.trim_end(),
        format!("cairn-lsp {WORKSPACE_VERSION}"),
        "unexpected --version output",
    );
}

#[test]
fn short_version_flag_matches_long() {
    let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .arg("-V")
        .output()
        .expect("spawn cairn-lsp -V");

    assert!(output.status.success(), "cairn-lsp -V exited non-zero");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert_eq!(stdout.trim_end(), format!("cairn-lsp {WORKSPACE_VERSION}"));
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    for flag in ["-h", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
            .arg(flag)
            .output()
            .expect("spawn cairn-lsp with help flag");
        assert!(
            output.status.success(),
            "{flag} exited non-zero: {:?}",
            output.status,
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
        assert!(
            stdout.contains("USAGE:"),
            "{flag} output missing USAGE section: {stdout}",
        );
        assert!(
            stdout.contains("--version") && stdout.contains("--help"),
            "{flag} output missing flag documentation: {stdout}",
        );
    }
}

#[test]
fn unknown_flag_exits_with_code_two_and_names_the_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .arg("--nope")
        .output()
        .expect("spawn cairn-lsp --nope");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unknown flag should exit 2, got {:?}",
        output.status,
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains("--nope"),
        "stderr should name the offending flag: {stderr}",
    );
    assert!(
        stderr.contains("--version") && stderr.contains("--help"),
        "stderr should list valid flags: {stderr}",
    );
}

#[test]
fn extra_arguments_after_version_are_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
        .args(["--version", "garbage"])
        .output()
        .expect("spawn cairn-lsp --version garbage");

    assert_eq!(
        output.status.code(),
        Some(2),
        "extra args after --version should exit 2, got {:?}",
        output.status,
    );
}
