//! `cairn check`: the validation passes and the lowering-stage findings, the ones every target
//! raises unless `--target` pins one.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::{Edition, Severity, diagnose_parse_failure, lower, parse};
use cairn_lang_formats::build::circuits::check_circuits;
use cairn_lang_formats::build::finding::{Finding, Found, render_findings};
use cairn_lang_formats::build::gate::{build_diagnostics, weighed_editions};
use cairn_lang_formats::build::lowering::{CheckLowering, check_lowering};
use cairn_lang_formats::build::run_level::{partial_build, unsupported_target};

use crate::cli::CheckFormat;
use crate::pipeline::read_source;
use crate::report::{print_json, report_diagnostic, report_parse_failure, report_run_level};

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
                        &render_findings(&source, &lines, &parse_failure),
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
    let mut diagnostics: Vec<Found> =
        build_diagnostics(&module, &ir, pin, &weighed_editions(pin), lowering)
            .into_iter()
            .map(Found::Core)
            .collect();
    // A member that failed to lower is not there for the circuit to wire.
    if !diagnostics.iter().any(|d| d.severity() == Severity::Error) {
        diagnostics.extend(
            check_circuits(&ir, edition, target)
                .into_iter()
                .map(Found::Circuit),
        );
    }
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
                &render_findings(&source, &lines, &diagnostics),
            ) {
                return code;
            }
        }
    }

    // Run-level refusals: stderr in both formats, after the file's own findings, as in `compile`.
    if !dropped_scopes.is_empty() {
        let refusal = partial_build(
            &dropped_scopes,
            built_scopes,
            "`cairn compile` at this target would refuse the build, for this or for a \
             `@requires` floor that refuses the target",
        );
        report_run_level(file, &source, &lines, &refusal);
        return ExitCode::from(1);
    }
    // A check that could not check the ids it was asked to must not exit 0.
    if let Some(err) = unsupported {
        report_run_level(file, &source, &lines, &unsupported_target(&err));
        return ExitCode::from(1);
    }

    if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
