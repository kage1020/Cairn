//! Compiling a written source at the stamped target, the check every transliteration passes.

use crate::block_array::{BlockArrayIr, TargetRegistry, lower_to_block_array};
use crate::diagnostic::{Diagnostic, Severity};
use crate::{check, lower, parse, resolve};

use super::{Stamp, TransliterateError};

/// The block arrays a source compiles to, with the warnings compiling it raised.
pub(super) struct Built {
    pub(super) ir: BlockArrayIr,
    pub(super) warnings: Vec<Diagnostic>,
}

/// `source` checked and lowered for `stamp`'s edition against `registry`, as `cairn compile`
/// builds it.
///
/// # Errors
///
/// [`TransliterateError::Refused`] when any pass raises an error.
pub(super) fn compile(
    source: &str,
    stamp: &Stamp,
    registry: &dyn TargetRegistry,
) -> Result<Built, TransliterateError> {
    let module = parse(source).map_err(|error| TransliterateError::Malformed {
        unit: "the written source".to_owned(),
        reason: format!("it does not parse: {}", error.user_message()),
    })?;
    let edition = stamp.edition;
    let ir = lower(&module);
    let mut findings = check(&module, &ir, Some(edition));
    let resolution = resolve(&ir, Some(edition));
    findings.extend(resolution.diagnostics.iter().cloned());
    let mut built = lower_to_block_array(&ir, &resolution, Some(registry));
    findings.append(&mut built.diagnostics);
    let (errors, warnings): (Vec<Diagnostic>, Vec<Diagnostic>) = findings
        .into_iter()
        .partition(|finding| finding.severity() == Severity::Error);
    if !errors.is_empty() {
        return Err(TransliterateError::Refused {
            target: format!("{edition} {}", stamp.mc_version),
            source: source.to_owned(),
            errors,
        });
    }
    Ok(Built {
        ir: built,
        warnings,
    })
}
