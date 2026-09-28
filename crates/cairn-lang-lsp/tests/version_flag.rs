//! Contract tests for the `cairn-lsp` command-line surface.
//!
//! The binary is spawned by editors (no arguments, or `--stdio`, which an LSP
//! client appends when told to use the stdio transport — both mean LSP over
//! stdio, and `lsp_stdio.rs` drives a session through each) and by users on
//! the command line for support triage. The surface it exposes to that second
//! audience is: `--version`/`-V` → `cairn-lsp <version>` and exit 0;
//! `--help`/`-h` → usage string and exit 0; an unknown argument → exit 2
//! with a message that lists the valid flags; a second argument after any
//! flag → exit 2 naming that argument and the flag it followed. These tests
//! pin that contract so a refactor of `main.rs` cannot silently reshape it.
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
        // `--stdio` also appears in prose; the row under `OPTIONS:` is the
        // one a reader scans, so pin that the flag has a row of its own
        // with a description beside it.
        let options: Vec<&str> = stdout
            .lines()
            .skip_while(|line| *line != "OPTIONS:")
            .skip(1)
            .take_while(|line| !line.trim().is_empty())
            .collect();
        assert!(
            options.iter().any(|row| {
                row.trim_start()
                    .strip_prefix("--stdio")
                    .is_some_and(|description| {
                        description.starts_with("  ") && !description.trim().is_empty()
                    })
            }),
            "{flag} output should document `--stdio` in its OPTIONS rows: {options:?}",
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
    for valid in ["--stdio", "-V/--version", "-h/--help"] {
        assert!(
            stderr.contains(valid),
            "stderr should list `{valid}` among the valid flags: {stderr}",
        );
    }
}

#[test]
fn extra_arguments_after_any_flag_are_rejected_naming_both() {
    // Every flag takes no further argument, `--stdio` included: it is
    // accepted because it names the only transport the server speaks, not
    // as a gate that lets anything after it through. An argument after any
    // flag is refused rather than silently dropped, and the message names
    // both the leftover and the flag it followed, so `--help garbage` is
    // not told that `--help` itself is unknown.
    for flag in ["-V", "--version", "-h", "--help", "--stdio"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cairn-lsp"))
            .args([flag, "garbage"])
            .output()
            .expect("spawn cairn-lsp with an extra argument");

        assert_eq!(
            output.status.code(),
            Some(2),
            "an extra argument after {flag} should exit 2, got {:?}",
            output.status,
        );
        let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
        assert!(
            stderr.contains(&format!("`garbage` after `{flag}`")),
            "stderr should name the extra argument and {flag}: {stderr}",
        );
    }
}
