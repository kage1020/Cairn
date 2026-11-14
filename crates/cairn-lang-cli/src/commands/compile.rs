//! `cairn compile`: the structure artifacts and the lockfile for one `(edition, version)` pin.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_formats::info::default_lock_path;
use cairn_lang_formats::target::{ResolvedTarget, resolve_target};

use crate::artifacts::{
    PrepareRefusal, check_lock_path_is_free, prepare_artifacts, prepare_out_dir,
    write_artifacts_and_lock,
};
use crate::lockfile::report_previous_target;
use crate::name_case::NameCase;
use crate::pipeline::load_and_lower;
use crate::report::{report_diagnostics, report_run_level};

pub(crate) fn run_compile(
    file: &Path,
    edition: Edition,
    target: &str,
    out: Option<&Path>,
    lock: Option<&Path>,
) -> ExitCode {
    // Resolved before lowering, which checks ids against it, but reported after the source's
    // own findings.
    let resolved_target = resolve_target(edition, target);
    let (source, lowered) = match load_and_lower(file, edition, resolved_target.as_ref().ok()) {
        Ok(lowered) => lowered,
        Err(code) => return code,
    };
    let lines = LineStarts::new(&source);
    let refused = report_diagnostics(file, &source, &lines, &lowered.block_ir.diagnostics)
        | report_diagnostics(file, &source, &lines, &lowered.circuits);
    for conflict in &lowered.conflicts {
        report_run_level(file, &source, &lines, conflict);
    }
    // A conflict holds at every target, so it stops the build ahead of the target's floor check.
    if refused || !lowered.conflicts.is_empty() {
        return ExitCode::from(1);
    }
    let target = match lowered.writable_target(edition, resolved_target) {
        Ok(target) => target,
        Err(refusal) => {
            report_run_level(file, &source, &lines, &refusal);
            return ExitCode::from(1);
        }
    };

    let out_dir = match prepare_out_dir(file, out) {
        Ok(d) => d,
        Err(code) => return code,
    };

    let lock_path = lock.map_or_else(|| default_lock_path(file), Path::to_path_buf);
    check_and_write(
        &lowered.block_ir,
        &source,
        edition,
        &target,
        &out_dir,
        &lock_path,
    )
}

/// Check every structure, then write them all and the lockfile.
fn check_and_write(
    block_ir: &BlockArrayIr,
    source: &str,
    edition: Edition,
    target: &ResolvedTarget,
    out_dir: &Path,
    lock_path: &Path,
) -> ExitCode {
    // A lock can share an entry with an artifact only inside `--out`, so `--out` alone is probed.
    let case = NameCase::of_dir(out_dir);
    let prepared = match prepare_artifacts(block_ir, target, out_dir, case) {
        Ok(p) => p,
        Err(PrepareRefusal::Collision(collision)) => {
            collision.report();
            return ExitCode::from(1);
        }
        Err(PrepareRefusal::Other(code)) => return code,
    };
    if let Err(code) = check_lock_path_is_free(&prepared, lock_path, case) {
        return code;
    }
    // Before the lockfile is replaced: it is the only record of what was verified before.
    report_previous_target(lock_path, edition, target);
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
            &block_ir,
            "",
            Edition::Java,
            &target,
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
