//! `cairn parse`: the AST of a source.

use std::path::Path;
use std::process::ExitCode;

use crate::cli::ParseFormat;
use crate::pipeline::load_module;
use crate::report::print_json;

pub(crate) fn run_parse(file: &Path, format: ParseFormat) -> ExitCode {
    let (_, module) = match load_module(file, format.failure_report()) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    match format {
        ParseFormat::Json => match print_json("AST", &module) {
            Ok(()) => ExitCode::SUCCESS,
            Err(code) => code,
        },
        ParseFormat::Debug => {
            outln!("{module:#?}");
            ExitCode::SUCCESS
        }
    }
}
