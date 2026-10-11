//! A mistyped flag, subcommand or value gets the same three-part answer
//! the compiler gives a mistyped `key=`: what is wrong, what would be
//! valid, and the nearest candidate. clap writes these messages, through
//! its default `error-context` and `suggestions` features, so a
//! `default-features = false` in the manifest would take the answer away
//! without touching a line of source. The assertions match clap's wording
//! verbatim: a red build after a clap bump wants the strings updated, not
//! a feature hunted for.

use std::process::Command;

use crate::common::{cargo_bin, example_in_tempdir};

/// Run `argv` with the placeholder `SOURCE` swapped for a real file,
/// assert clap refused it as a usage error, and return stderr.
///
/// The file is real so the exit code can tell: `cairn` also exits 2 for a
/// path that names nothing, and with a missing file a clap that accepted
/// the typo would still exit 2. Here it would check the file and exit 0.
///
/// `NO_COLOR` because clap styles the token it refused, and a
/// `CLICOLOR_FORCE` in the environment colours stderr even though it is
/// not a terminal; the escape codes would split the strings the tests
/// look for. `NO_COLOR` wins over `CLICOLOR_FORCE`.
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
    // Written out: editions are a fixed two-way axis, not a list that
    // grows a member with each new pass.
    assert!(
        stderr.contains("[possible values: java, bedrock]"),
        "the refusal should list the values the flag accepts: {stderr}",
    );
    assert!(
        stderr.contains("a similar value exists: 'java'"),
        "the refusal should suggest the nearest value: {stderr}",
    );
}
