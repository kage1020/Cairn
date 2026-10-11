//! The `cairn` binary, run as a subprocess against sources on disk: exit
//! codes, the streams each subcommand writes, and the files `compile`
//! leaves behind. One test binary, so [`common`] is compiled once.

mod common;

mod check;
mod compile;
mod contract;
mod info;
mod limits;
mod lower;
mod parse;
mod synth;
