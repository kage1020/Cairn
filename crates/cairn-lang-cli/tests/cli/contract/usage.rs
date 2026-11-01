//! A mistyped flag, subcommand or value gets what is wrong, what is valid and the nearest
//! candidate. The strings are clap's own; update them after a clap bump.

use std::process::Command;

use crate::common::{cargo_bin, example_in_tempdir};

/// Run `argv` with `SOURCE` swapped for a real file, since a missing one also exits 2, and
/// return stderr once the exit says usage error.
///
/// `NO_COLOR`: clap's escape codes would split the strings looked for.
fn usage_error(argv: &[&str]) -> String {
    let (_dir, source) = example_in_tempdir("cottage.crn");
    let source = source.to_str().expect("temp path is utf-8");
    let argv: Vec<&str> = argv
        .iter()
        .map(|&arg| if arg == "SOURCE" { source } else { arg })
        .collect();
    let output = Command::new(cargo_bin())
        .args(&argv)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(
        output.status.code(),
        Some(2),
        "`cairn {}` should be a usage error; stderr={stderr}",
        argv.join(" "),
    );
    stderr
}

#[test]
fn a_mistyped_flag_is_named_and_the_nearest_flag_suggested() {
    let stderr = usage_error(&["check", "SOURCE", "--editon", "java"]);
    assert!(
        stderr.contains("unexpected argument '--editon' found"),
        "the refusal should name the token it refused: {stderr}",
    );
    assert!(
        stderr.contains("a similar argument exists: '--edition'"),
        "the refusal should suggest the nearest flag: {stderr}",
    );
}

#[test]
fn a_mistyped_subcommand_is_named_and_the_nearest_subcommand_suggested() {
    let stderr = usage_error(&["chek", "SOURCE"]);
    assert!(
        stderr.contains("unrecognized subcommand 'chek'"),
        "the refusal should name the token it refused: {stderr}",
    );
    assert!(
        stderr.contains("a similar subcommand exists: 'check'"),
        "the refusal should suggest the nearest subcommand: {stderr}",
    );
}

#[test]
fn a_mistyped_value_lists_the_valid_ones_and_suggests_the_nearest() {
    let stderr = usage_error(&["check", "SOURCE", "--edition", "jav"]);
    assert!(
        stderr.contains("invalid value 'jav' for '--edition <EDITION>'"),
        "the refusal should name the value and the flag it was given to: {stderr}",
    );
    assert!(
        stderr.contains("[possible values: java, bedrock]"),
        "the refusal should list the values the flag accepts: {stderr}",
    );
    assert!(
        stderr.contains("a similar value exists: 'java'"),
        "the refusal should suggest the nearest value: {stderr}",
    );
}
