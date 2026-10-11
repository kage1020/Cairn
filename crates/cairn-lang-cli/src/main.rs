//! Cairn command-line entry point.

// Stdout goes through `outln!`, which keeps the exit code when the reader
// has gone; a bare `println!` would panic there instead.
#![deny(clippy::print_stdout)]

/// `println!` for everything the commands write to stdout, except that a
/// reader who has closed the pipe (`cairn lower f.crn | head -1`) is not
/// a failure: the line is dropped and the command carries on to the exit
/// code it decides, so the process ends with that code rather than a
/// panic (an abort in release builds). Every later line fails the same
/// way and is dropped the same way. Any other write error still panics
/// with `println!`'s message.
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::write_stdout_line(format_args!($($arg)*))
    };
}

mod artifacts;
mod cli;
mod commands;
mod floors;
mod lockfile;
mod pipeline;
mod report;
mod staging;
mod target;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use commands::check::run_check;
use commands::compile::run_compile;
use commands::info::run_info;
use commands::lower::run_lower;
use commands::parse::run_parse;
use commands::synth::run_synth;

/// The body of [`outln!`].
fn write_stdout_line(line: std::fmt::Arguments<'_>) {
    write_line(&mut std::io::stdout().lock(), line);
}

/// Write `line` to `out` as [`outln!`] does: drop it if the reader has
/// gone, panic on any other error.
fn write_line(out: &mut impl std::io::Write, line: std::fmt::Arguments<'_>) {
    match writeln!(out, "{line}") {
        Ok(()) => {}
        // The reader is gone. Drop the line; the command keeps the code it
        // decides.
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(err) => panic!("failed printing to stdout: {err}"),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Parse { file, format }) => run_parse(&file, format),
        Some(Command::Check {
            file,
            edition,
            target,
            format,
        }) => run_check(&file, edition, target.as_deref(), format),
        Some(Command::Info {
            file,
            editions,
            format,
        }) => run_info(&file, &editions, format),
        Some(Command::Lower { file, format }) => run_lower(&file, format),
        Some(Command::Synth {
            file,
            experimental_logic_synth,
            stage,
            edition,
        }) => run_synth(&file, experimental_logic_synth, stage, edition),
        Some(Command::Compile {
            file,
            edition,
            target,
            out,
            lock,
        }) => run_compile(&file, edition, &target, out.as_deref(), lock.as_deref()),
        None => {
            eprintln!("error: a subcommand is required (try `cairn --help`)");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer whose every write fails with `kind`.
    struct Failing(std::io::ErrorKind);

    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_line_writes_the_line() {
        let mut out = Vec::new();
        write_line(&mut out, format_args!("[{:>3}] {}", 7, "x"));
        assert_eq!(out, b"[  7] x\n");
    }

    #[test]
    fn write_line_drops_a_line_whose_reader_has_gone() {
        write_line(
            &mut Failing(std::io::ErrorKind::BrokenPipe),
            format_args!("x"),
        );
    }

    #[test]
    #[should_panic(expected = "failed printing to stdout")]
    fn write_line_panics_on_any_other_write_error() {
        write_line(
            &mut Failing(std::io::ErrorKind::StorageFull),
            format_args!("x"),
        );
    }
}
