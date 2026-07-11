//! `cairn-lsp --version` prints the workspace `CAIRN_VERSION` and exits 0.
//!
//! The VS Code extension logs this string at activation to help correlate
//! server-side behaviour with a specific Cairn release when a bug report
//! arrives. Keeping the format aligned with `cairn --version` means one
//! parser on the editor side handles both binaries.

use std::process::Command;

use cairn_lang_core::CAIRN_VERSION;

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
        format!("cairn-lsp {CAIRN_VERSION}"),
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
    assert_eq!(stdout.trim_end(), format!("cairn-lsp {CAIRN_VERSION}"));
}
