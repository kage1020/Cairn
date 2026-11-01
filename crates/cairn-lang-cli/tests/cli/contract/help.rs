//! `cairn <subcommand> --help` states the subcommand's exit codes.

use crate::common::{Run, cairn};

#[test]
fn long_help_states_each_subcommands_exit_codes() {
    for (subcommand, exit_two) in [
        (
            "check",
            "2 when the file cannot be located or `--target` is given without `--edition`",
        ),
        (
            "info",
            "2 when the file cannot be located, and rejects an empty `--editions` value with exit 2",
        ),
        ("lower", "and 2 when the file cannot be located"),
        ("compile", "and 2 when the source file cannot be located"),
        ("synth", "with exit 2, before the file is read"),
        (
            "docs",
            "and 2 when PAGE or TITLE names nothing this binary carries",
        ),
    ] {
        let run = Run::from(cairn(subcommand, &["--help"]));
        assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
        assert!(
            run.stdout.contains("Exits 0") && run.stdout.contains(exit_two),
            "`cairn {subcommand} --help` should state its exit codes, got:\n{}",
            run.stdout,
        );
    }
}
