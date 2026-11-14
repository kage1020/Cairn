//! The front half the subcommands share: reading and parsing a source, and lowering it for a
//! build.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Edition, diagnose_parse_failure, parse};
use cairn_lang_formats::build::lowering::{Lowered, lower_for_target};
use cairn_lang_formats::target::ResolvedTarget;

use crate::report::{FailureReport, report_failure_document};

/// Read a source, or report why not: exit 2 for a path that names nothing, 1 for anything else.
pub(crate) fn read_source(file: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(file).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })
}

/// Read and parse a source, reporting a parse failure as `report` asks.
pub(crate) fn load_module(
    file: &Path,
    report: FailureReport,
) -> Result<(String, cairn_lang_core::ast::Module), ExitCode> {
    let source = read_source(file)?;
    let module = parse(&source).map_err(|err| {
        let lines = LineStarts::new(&source);
        let parse_failure = [diagnose_parse_failure(&source, &lines, &err)];
        report_failure_document(file, &source, report, &parse_failure)
    })?;
    Ok((source, module))
}

/// Read, parse, resolve and lower `file` for `edition`'s build, returning the source beside it.
pub(crate) fn load_and_lower(
    file: &Path,
    edition: Edition,
    target: Option<&ResolvedTarget>,
) -> Result<(String, Lowered), ExitCode> {
    let (source, module) = load_module(file, FailureReport::Text)?;
    let lowered = lower_for_target(&module, edition, target);
    Ok((source, lowered))
}

/// A walled three-by-three `struct probe` lowered unpinned, for unit tests to rekey or corrupt.
#[cfg(test)]
pub(crate) fn lowered_probe(edition: Edition) -> cairn_lang_core::block_array::BlockArrayIr {
    use cairn_lang_core::block_array::lower_to_block_array;
    use cairn_lang_core::{lower, resolve::resolve};
    use cairn_lang_formats::registry::builtin_pack;

    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(&ir, &resolution, Some(&builtin_pack(edition).view(None)))
}
