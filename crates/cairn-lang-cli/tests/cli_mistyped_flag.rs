//! A mistyped flag, subcommand or value gets the same three-part answer
//! the compiler gives a mistyped `key=`: what is wrong, what would be
//! valid, and the nearest candidate.
//!
//! None of it is Cairn code. clap writes these messages, and two of its
//! cargo features write the parts that matter: `error-context` names the
//! token that was refused and lists the values an argument accepts, and
//! `suggestions` adds the "a similar ... exists" tip. Both are in clap's
//! default feature set, which is the only thing turning them on, so a
//! `default-features = false` in `cairn-lang-cli`'s manifest — the first
//! thing a binary-size pass reaches for — takes them away without touching
//! a line of source. Without `error-context` the first case below reads
//! `error: unexpected argument found`, naming nothing.
//!
//! The assertions match clap's wording verbatim, so a reword inside 4.x
//! turns them red with both features still on. That is the price of
//! pinning what the reader sees rather than that some message appeared;
//! a red build here after a clap bump wants the strings updated, not a
//! feature hunted for.

mod common;
use std::process::Command;

use common::{cargo_bin, example_in_tempdir};

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
    // Written out, where `cli_synth.rs` parses the `--stage` list back
    // out of clap: editions are a fixed two-way axis, not a list that
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
