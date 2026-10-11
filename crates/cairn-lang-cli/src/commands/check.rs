//! `cairn check`: the validation passes, and with `--target` the
//! lowering-stage findings a compile at that pin would refuse on.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Severity, diagnose_parse_failure, lower, parse};

use crate::cli::{CheckFormat, EditionArg};
use crate::pipeline::{
    CheckLowering, build_diagnostics, check_lowering, read_source, report_partial_build,
    weighed_editions,
};
use crate::report::{print_json, render_diagnostics, report_diagnostic, report_parse_failure};

pub(crate) fn run_check(
    file: &Path,
    edition: Option<EditionArg>,
    target: Option<&str>,
    format: CheckFormat,
) -> ExitCode {
    let source = match read_source(file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    // A parse failure pre-empts any check pass — the AST/IR has to be
    // well-formed before invariant-collecting can run. Surface it under the
    // same exit code as a check-level error so a CI pipeline gating on
    // `cairn check` does not silently pass a file that the parser rejected.
    let module = match parse(&source) {
        Ok(m) => m,
        Err(err) => {
            // The product of `check --format json` is the diagnostics
            // array, so a parse failure is that array with one element in
            // it. Nothing about the shape changes; what changes is that
            // the most common way a source fails stops being the one input
            // the flag answers with an empty stream.
            match format {
                CheckFormat::Text => report_parse_failure(file, &source, &err),
                CheckFormat::Json => {
                    let lines = LineStarts::new(&source);
                    let parse_failure = [diagnose_parse_failure(&source, &lines, &err)];
                    if let Err(code) = print_json(
                        "diagnostics",
                        &render_diagnostics(&source, &lines, &parse_failure),
                    ) {
                        return code;
                    }
                }
            }
            return ExitCode::from(1);
        }
    };
    let ir = lower(&module);
    // Through `build_diagnostics` rather than `check` directly: `cairn
    // check` sees both version headers, and the `@intended_targets`
    // findings are the one part of its report that `check` cannot produce
    // on its own. The lowering list it merges is empty unless `--target`
    // pinned a version to lower against.
    let pin = edition.map(EditionArg::as_edition);
    let CheckLowering {
        diagnostics: lowering,
        dropped_scopes,
        built_scopes,
        unsupported,
    } = check_lowering(&ir, edition, target);
    let diagnostics = build_diagnostics(&module, &ir, pin, &weighed_editions(pin), lowering);
    let has_error = diagnostics.iter().any(|d| d.severity() == Severity::Error);
    // Build the line-start index once and reuse it for every diagnostic /
    // note position lookup. Without this we'd re-walk the entire source for
    // each position computation, which gets expensive when a single file
    // produces many diagnostics (e.g. a registry pack ingest run).
    let lines = LineStarts::new(&source);

    match format {
        // Diagnostics are the report, not the product. Every other
        // subcommand sends theirs to stderr, and `check` sending its text
        // form to stdout meant `cairn check f.crn > out` swallowed them
        // while still exiting 1 — a CI step that captured stdout for
        // something else saw a bare exit code and no reason.
        //
        // `--format json` stays on stdout: that one *is* the product, and a
        // consumer redirects it deliberately.
        CheckFormat::Text => {
            for d in &diagnostics {
                report_diagnostic(file, &source, &lines, d);
            }
        }
        CheckFormat::Json => {
            if let Err(code) = print_json(
                "diagnostics",
                &render_diagnostics(&source, &lines, &diagnostics),
            ) {
                return code;
            }
        }
    }

    // Both refusals come after the report, not before it: the file's own
    // findings are what the author edits, and a run-level failure printed
    // above them would bury the line that has to change. Neither joins the
    // `--format json` array — see the `Check` doc — so both are stderr in
    // both formats, which is the shape `compile` gives them.
    //
    // A lost scope first, in `run_compile`'s order: it is a fact about the
    // file, and the target that could not be resolved is a fact about the
    // command line.
    //
    // The verdict, and not the reason, is `compile`'s: this run is not held
    // to the floors, and a `compile` whose floors refuse the target checks
    // no id, so it can refuse on the floor without losing this scope.
    if !dropped_scopes.is_empty() {
        report_partial_build(
            file,
            &dropped_scopes,
            built_scopes,
            "`cairn compile` at this target would refuse the build, for this or for a \
             `@requires` floor that refuses the target",
        );
        return ExitCode::from(1);
    }
    // A check that could not check the ids it was asked to must not exit 0.
    if let Some(err) = unsupported {
        eprintln!("error: {err}");
        return ExitCode::from(1);
    }

    if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
