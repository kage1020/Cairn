//! `cairn compile`: the structure artifacts and the lockfile for one `(edition, version)` pin.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_formats::build::lowering::BuildRefusal;
use cairn_lang_formats::build::run_level::floor_refusal_message;
use cairn_lang_formats::data_version::{ResolvedTarget, resolve_target};
use cairn_lang_formats::file_identity::Reach;
use cairn_lang_formats::lock_path::{
    LockPathRefusal, default_lock_path, default_lock_refusal, lock_path_refusal,
    suggested_lock_path,
};
use cairn_lang_formats::write_path::WritePath;

use crate::artifacts::{
    Backend, PrepareRefusal, check_lock_path_is_free, check_scratch_is_free,
    check_source_is_left_alone, prepare_artifacts, prepare_out_dir, report_unidentified,
    write_artifacts_and_lock,
};
use crate::cli::StructureFormat;
use crate::lockfile::{report_previous_target, sensitive_members};
use crate::name_case::NameCase;
use crate::pipeline::load_and_lower;
use crate::report::{report_diagnostics, report_run_level};

pub(crate) fn run_compile(
    file: &Path,
    edition: Edition,
    target: &str,
    structure_format: Option<StructureFormat>,
    out: Option<&Path>,
    lock: Option<&Path>,
) -> ExitCode {
    if edition == Edition::Bedrock && structure_format.is_some() {
        eprintln!(
            "error: `--structure-format` picks a Java format; `--edition bedrock` writes only \
             `.mcstructure`"
        );
        eprintln!("  note: drop `--structure-format`, or compile with `--edition java`");
        return ExitCode::from(2);
    }
    if let Some(lock) = lock {
        match lock_path_refusal(file, lock) {
            Ok(None) => {}
            Ok(Some(refusal)) => {
                report_lock_path_refusal(file, lock, refusal);
                return ExitCode::from(2);
            }
            Err(unidentified) => {
                report_unidentified(
                    &format!("lockfile `{}`", lock.display()),
                    file,
                    &unidentified,
                    ", or give `--lock` a `.lock` path elsewhere",
                );
                return ExitCode::from(1);
            }
        }
    }
    let structure_format = structure_format.unwrap_or_default();
    // Resolved before lowering, which checks ids against it, but reported after the source's
    // own findings.
    let (source, lowered) = match load_and_lower(file, edition, resolve_target(edition, target)) {
        Ok(lowered) => lowered,
        Err(code) => return code,
    };
    let lines = LineStarts::new(&source);
    report_diagnostics(file, &source, &lines, &lowered.block_ir.diagnostics);
    report_diagnostics(file, &source, &lines, &lowered.circuits);
    for conflict in &lowered.conflicts {
        report_run_level(file, &source, &lines, conflict);
    }
    let target = match lowered.verdict() {
        Ok(target) => target,
        Err(BuildRefusal::Findings | BuildRefusal::Conflicts) => return ExitCode::from(1),
        Err(BuildRefusal::RunLevel(refusal)) => {
            report_run_level(file, &source, &lines, &refusal);
            return ExitCode::from(1);
        }
        Err(BuildRefusal::Floor(refusal)) => {
            report_run_level(file, &source, &lines, &floor_refusal_message(refusal));
            return ExitCode::from(1);
        }
    };

    let out_dir = match prepare_out_dir(file, out) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    check_and_write(
        file,
        &lowered.block_ir,
        &source,
        target,
        structure_format,
        &out_dir,
        &lock_path,
    )
}

/// Say why `--lock lock` cannot be the lockfile of `file`, and which paths it can be.
fn report_lock_path_refusal(file: &Path, lock: &Path, refusal: LockPathRefusal) {
    let (shown, source) = (lock.display(), file.display());
    let fix = match refusal {
        LockPathRefusal::Source(reached) => {
            match reached.path {
                WritePath::Final => {
                    eprintln!(
                        "error: `--lock {shown}` names `{source}`, the source being compiled"
                    );
                }
                WritePath::Scratch(scratch) => eprintln!(
                    "error: `--lock {shown}` would put its scratch file `{}` over `{source}`, the \
                     source being compiled",
                    scratch.of(lock).display(),
                ),
            }
            match reached.by {
                Reach::Name => "give `--lock` a `.lock` path".to_owned(),
                Reach::Link => {
                    "a link makes them one file; give `--lock` a `.lock` path elsewhere".to_owned()
                }
            }
        }
        LockPathRefusal::SourceName => {
            eprintln!(
                "error: `--lock {shown}` names a `.crn` file, and a lockfile is never a Cairn \
                 source"
            );
            format!(
                "give `--lock` a `.lock` path, such as `{}`",
                suggested_lock_path(lock).display(),
            )
        }
    };
    let leave_out = match default_lock_refusal(file) {
        Ok(None) => format!(
            ", or leave it out to write `{}`",
            default_lock_path(file).display()
        ),
        Ok(Some(_)) | Err(_) => String::new(),
    };
    eprintln!("  note: {fix}{leave_out}");
}

/// Check every structure, then write them all and the lockfile of `file`.
fn check_and_write(
    file: &Path,
    block_ir: &BlockArrayIr,
    source: &str,
    target: &ResolvedTarget,
    structure_format: StructureFormat,
    out_dir: &Path,
    lock_path: &Path,
) -> ExitCode {
    let edition = target.edition();
    // A lock can share an entry with an artifact only inside `--out`, so `--out` alone is probed.
    let case = NameCase::of_dir(out_dir);
    let backend = Backend::new(target, structure_format);
    let prepared = match prepare_artifacts(block_ir, backend, out_dir, case) {
        Ok(p) => p,
        Err(PrepareRefusal::Collision(collision)) => {
            collision.report();
            return ExitCode::from(1);
        }
        Err(PrepareRefusal::Other(code)) => return code,
    };
    if let Err(code) = check_source_is_left_alone(file, &prepared, lock_path) {
        return code;
    }
    if let Err(code) = check_lock_path_is_free(&prepared, lock_path, case) {
        return code;
    }
    if let Err(code) = check_scratch_is_free(&prepared, lock_path) {
        return code;
    }
    // Before the lockfile is replaced: it is the only record of what was verified before.
    report_previous_target(
        lock_path,
        edition,
        target,
        &sensitive_members(block_ir, edition, target),
    );
    write_artifacts_and_lock(&prepared, source, block_ir, edition, target, lock_path)
}

#[cfg(test)]
mod tests {
    use cairn_lang_core::Edition;

    use super::*;
    use crate::pipeline::lowered_probe;

    #[test]
    fn a_structure_the_backend_refuses_leaves_out_dir_and_lockfile_untouched() {
        let mut block_ir = lowered_probe(Edition::Java);
        let target = resolve_target(Edition::Java, "latest").expect("the latest target resolves");
        let array = block_ir
            .structures
            .values()
            .next()
            .expect("the probe lowers to one structure")
            .clone();
        let mut short = array.clone();
        short.voxels.pop();
        block_ir.structures = [
            ("struct::a".to_owned(), array),
            ("struct::b".to_owned(), short),
        ]
        .into_iter()
        .collect();
        let dir = tempfile::tempdir().expect("temp dir");
        let lock_path = dir.path().join("probe.crn.lock");

        let code = check_and_write(
            &dir.path().join("probe.crn"),
            &block_ir,
            "",
            &target,
            StructureFormat::Nbt,
            dir.path(),
            &lock_path,
        );

        assert_eq!(code, ExitCode::from(1));
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read temp dir")
            .map(|entry| entry.expect("dir entry").file_name())
            .collect();
        assert!(left.is_empty(), "a refused build wrote {left:?}");
    }
}
