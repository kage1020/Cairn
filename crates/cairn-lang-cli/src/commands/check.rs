//! `cairn check`: the validation passes, and with `--target` the lowering-stage findings too.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Edition, Severity, diagnose_parse_failure, lower, parse};

use crate::cli::CheckFormat;
use crate::pipeline::{
    CheckLowering, build_diagnostics, check_lowering, read_source, report_partial_build,
    weighed_editions,
};
use crate::report::{print_json, render_diagnostics, report_diagnostic, report_parse_failure};

pub(crate) fn run_check(
    file: &Path,
    edition: Option<Edition>,
    target: Option<&str>,
    format: CheckFormat,
) -> ExitCode {
    let source = match read_source(file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let module = match parse(&source) {
        Ok(m) => m,
        Err(err) => {
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
    // Not `check`, which cannot produce the `@intended_targets` findings.
    let pin = edition;
    let CheckLowering {
        diagnostics: lowering,
        dropped_scopes,
        built_scopes,
        unsupported,
    } = check_lowering(&ir, edition, target);
    let diagnostics = build_diagnostics(&module, &ir, pin, &weighed_editions(pin), lowering);
    let has_error = diagnostics.iter().any(|d| d.severity() == Severity::Error);
    let lines = LineStarts::new(&source);

    match format {
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

    // Run-level refusals: stderr in both formats, after the file's own findings, as in `compile`.
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
