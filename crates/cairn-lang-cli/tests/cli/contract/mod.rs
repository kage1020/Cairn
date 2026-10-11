//! What every subcommand promises a caller: `--version`, usage errors,
//! `--format json` on stdout, and the exit code when stdout has closed.

mod closed_stdout;
mod json;
mod usage;
mod version;
