//! One module per subcommand, each holding its `run_*` entry point and
//! what only that subcommand prints.

pub(crate) mod check;
pub(crate) mod compile;
pub(crate) mod info;
pub(crate) mod lower;
pub(crate) mod parse;
pub(crate) mod synth;
