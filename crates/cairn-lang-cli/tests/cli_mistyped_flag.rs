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

mod common;
use common::cairn_argv;

/// Run `argv`, assert clap refused it as a usage error, and return stderr.
fn usage_error(argv: &[&str]) -> String {
    let output = cairn_argv(argv);
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
    let stderr = usage_error(&["check", "x.crn", "--editon", "java"]);
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
    let stderr = usage_error(&["chek", "x.crn"]);
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
    let stderr = usage_error(&["check", "x.crn", "--edition", "jav"]);
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
